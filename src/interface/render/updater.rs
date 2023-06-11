use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::thread;

use cosmic_text::{fontdb, FontSystem, SwashCache};
use crossbeam::channel::{self, Receiver, Sender, TryRecvError};

use crate::interface::bin::{Bin, BinID};
use crate::interface::render::ImageKey;
use crate::interface::{DefaultFont, ItfVertInfo};
use crate::BstOptions;

#[derive(Clone)]
enum Cmd {
    Extent([u32; 2]),
    Scale(f32),
    DefaultFont(DefaultFont),
    Perform,
}

struct BinState {
    weak: Weak<Bin>,
    version: u64,
    outdated: bool,
    vertex_data: HashMap<ImageKey, Vec<ItfVertInfo>>,
}

pub(crate) struct UpdateContext {
    pub extent: [f32; 2],
    pub scale: f32,
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub default_font: DefaultFont,
}

pub struct Updater {
    version: u64,
    state: HashMap<BinID, BinState>,
    cmd_send: Vec<Sender<Cmd>>,
    bin_send: Sender<Arc<Bin>>,
    data_recv: Receiver<(BinID, HashMap<ImageKey, Vec<ItfVertInfo>>)>,
}

impl Updater {
    pub fn new(mut options: BstOptions) -> Self {
        let mut cmd_send = Vec::with_capacity(options.bin_parallel_threads.get());
        let (bin_send, bin_recv) = channel::unbounded();
        let (data_send, data_recv) = channel::unbounded();

        let additional_fonts = options
            .additional_fonts
            .drain(..)
            .map(|font| fontdb::Source::Binary(font))
            .collect::<Vec<_>>();

        let extent: [f32; 2] = [options.window_size[0] as f32, options.window_size[1] as f32];
        let scale = options.scale;

        for _ in 0..options.bin_parallel_threads.get() {
            let (thrd_cmd_send, cmd_recv) = channel::unbounded();
            cmd_send.push(thrd_cmd_send);
            let bin_recv = bin_recv.clone();
            let data_send = data_send.clone();
            let additional_fonts = additional_fonts.clone();

            thread::spawn(move || {
                let mut update_context = UpdateContext {
                    extent,
                    scale,
                    font_system: FontSystem::new_with_fonts(additional_fonts.into_iter()),
                    swash_cache: SwashCache::new(),
                    default_font: DefaultFont::default(),
                };

                loop {
                    loop {
                        match cmd_recv.recv() {
                            Ok(cmd) => {
                                match cmd {
                                    Cmd::Extent(extent) => {
                                        update_context.extent =
                                            [extent[0] as f32, extent[1] as f32];
                                    },
                                    Cmd::Scale(scale) => {
                                        update_context.scale = scale;
                                    },
                                    Cmd::DefaultFont(default_font) => {
                                        update_context.default_font = default_font;
                                    },
                                    Cmd::Perform => break,
                                }
                            },
                            Err(_) => return,
                        }
                    }

                    loop {
                        let bin: Arc<Bin> = match bin_recv.try_recv() {
                            Ok(ok) => ok,
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => return,
                        };

                        let vertex_map = bin
                            .do_update(&mut update_context)
                            .unwrap_or_else(HashMap::new);

                        if let Err(_) = data_send.send((bin.id(), vertex_map)) {
                            return;
                        }
                    }
                }
            });
        }

        Self {
            version: 1,
            state: HashMap::new(),
            cmd_send,
            bin_send,
            data_recv,
        }
    }

    pub fn set_extent(&mut self, extent: [u32; 2]) {
        self.send_cmd(Cmd::Extent(extent));
        self.mark_all_outdated();
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.send_cmd(Cmd::Scale(scale));
        self.mark_all_outdated();
    }

    pub fn set_default_font(&mut self, default_font: DefaultFont) {
        self.send_cmd(Cmd::DefaultFont(default_font));
        self.mark_all_outdated();
    }

    pub fn track_bins(&mut self, bins: Vec<Weak<Bin>>) {
        for bin_wk in bins {
            let bin = match bin_wk.upgrade() {
                Some(some) => some,
                None => continue,
            };

            self.state.insert(
                bin.id(),
                BinState {
                    weak: bin_wk,
                    version: 0,
                    outdated: true,
                    vertex_data: HashMap::new(),
                },
            );
        }
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub(super) fn all_vertex_data(
        &self,
    ) -> impl Iterator<Item = (BinID, &HashMap<ImageKey, Vec<ItfVertInfo>>)> + '_ {
        self.state
            .iter()
            .map(|(id, state)| (*id, &state.vertex_data))
    }

    fn send_cmd(&self, cmd: Cmd) {
        for cmd_send in self.cmd_send.iter() {
            cmd_send.send(cmd.clone()).unwrap();
        }
    }

    fn mark_all_outdated(&mut self) {
        for (_, state) in self.state.iter_mut() {
            state.outdated = true;
        }
    }

    pub fn perform(&mut self) {
        let mut queued = 0;

        self.state.retain(|_, state| {
            let bin = match state.weak.upgrade() {
                Some(some) => some,
                None => return false,
            };

            if state.outdated || bin.wants_update() {
                self.bin_send.send(bin).unwrap();
                state.outdated = true;
                queued += 1;
            }

            true
        });

        if queued == 0 {
            return;
        }

        self.send_cmd(Cmd::Perform);
        self.version += 1;

        // TODO: catch panics: if a thread panics decrease queued by one. Afterwards a bin that is
        // marked outdated still is the cause of the panic.

        while queued > 0 {
            let (bin_id, vertex_data) = self.data_recv.recv().unwrap();
            let state = self.state.get_mut(&bin_id).unwrap();
            state.version = self.version;
            state.outdated = false;
            state.vertex_data = vertex_data;
            queued -= 1;
        }
    }
}
