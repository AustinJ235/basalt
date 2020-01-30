pub mod style;
pub use self::style::{BinPosition, BinStyle, BinVert, Color, ImageEffect};

use super::{super::atlas, interface::ItfVertInfo};
use arc_swap::ArcSwapAny;
use atlas::{Image, ImageData, ImageDims, ImageType, SubImageCacheID};
use ilmenite::*;
use input::*;
use interface::{
    hook::{BinHook, BinHookData, BinHookFn, BinHookID},
    interface::scale_verts,
};
use misc;
use ordered_float::OrderedFloat;
use parking_lot::{Mutex, RwLock};
use std::{
    f32::consts::PI,
    sync::{
        atomic::{self, AtomicBool},
        Arc,
        Barrier,
        Weak,
    },
    thread,
    time::{Duration, Instant},
};
use vulkano::{
    self,
    image::{immutable::ImmutableImage, traits::ImageViewAccess},
};
use Basalt;

pub trait KeepAlive {}
impl KeepAlive for Arc<Bin> {}
impl KeepAlive for Bin {}
impl<T: KeepAlive> KeepAlive for Vec<T> {}

struct ImageInfo {
    image: Option<Arc<dyn ImageViewAccess + Send + Sync>>,
    coords: atlas::Coords,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct BinUpdateStats {
    pub t_total: Duration,
    pub t_hidden: Duration,
    pub t_ancestors: Duration,
    pub t_position: Duration,
    pub t_zindex: Duration,
    pub t_image: Duration,
    pub t_opacity: Duration,
    pub t_verts: Duration,
    pub t_overflow: Duration,
    pub t_scale: Duration,
    pub t_callbacks: Duration,
    pub t_style_obtain: Duration,
    pub t_upcheck: Duration,
    pub t_postset: Duration,
    pub t_locks: Duration,
}

impl BinUpdateStats {
    pub fn divide(self, amt: f32) -> Self {
        BinUpdateStats {
            t_total: self.t_total.div_f32(amt as f32),
            t_hidden: self.t_hidden.div_f32(amt as f32),
            t_ancestors: self.t_ancestors.div_f32(amt as f32),
            t_position: self.t_position.div_f32(amt as f32),
            t_zindex: self.t_zindex.div_f32(amt as f32),
            t_image: self.t_image.div_f32(amt as f32),
            t_opacity: self.t_opacity.div_f32(amt as f32),
            t_verts: self.t_verts.div_f32(amt as f32),
            t_overflow: self.t_overflow.div_f32(amt as f32),
            t_scale: self.t_scale.div_f32(amt as f32),
            t_callbacks: self.t_callbacks.div_f32(amt as f32),
            t_style_obtain: self.t_style_obtain.div_f32(amt as f32),
            t_upcheck: self.t_upcheck.div_f32(amt as f32),
            t_postset: self.t_postset.div_f32(amt as f32),
            t_locks: self.t_postset.div_f32(amt as f32),
        }
    }

    pub fn average(stats: &Vec<BinUpdateStats>) -> BinUpdateStats {
        let len = stats.len();
        Self::sum(stats).divide(len as f32)
    }

    pub fn sum(stats: &Vec<BinUpdateStats>) -> BinUpdateStats {
        let mut t_total = Duration::new(0, 0);
        let mut t_hidden = Duration::new(0, 0);
        let mut t_ancestors = Duration::new(0, 0);
        let mut t_position = Duration::new(0, 0);
        let mut t_zindex = Duration::new(0, 0);
        let mut t_image = Duration::new(0, 0);
        let mut t_opacity = Duration::new(0, 0);
        let mut t_verts = Duration::new(0, 0);
        let mut t_overflow = Duration::new(0, 0);
        let mut t_scale = Duration::new(0, 0);
        let mut t_callbacks = Duration::new(0, 0);
        let mut t_style_obtain = Duration::new(0, 0);
        let mut t_upcheck = Duration::new(0, 0);
        let mut t_postset = Duration::new(0, 0);
        let mut t_locks = Duration::new(0, 0);

        for stat in stats {
            t_total += stat.t_total;
            t_hidden += stat.t_hidden;
            t_ancestors += stat.t_ancestors;
            t_position += stat.t_position;
            t_zindex += stat.t_zindex;
            t_image += stat.t_image;
            t_opacity += stat.t_opacity;
            t_verts += stat.t_verts;
            t_overflow += stat.t_overflow;
            t_scale += stat.t_scale;
            t_callbacks += stat.t_callbacks;
            t_style_obtain += stat.t_style_obtain;
            t_upcheck += stat.t_upcheck;
            t_postset += stat.t_postset;
            t_locks += stat.t_locks;
        }

        BinUpdateStats {
            t_total,
            t_hidden,
            t_ancestors,
            t_position,
            t_zindex,
            t_image,
            t_opacity,
            t_verts,
            t_overflow,
            t_scale,
            t_callbacks,
            t_style_obtain,
            t_upcheck,
            t_postset,
            t_locks,
        }
    }
}

#[derive(Default)]
struct BinHrchy {
    parent: Option<Weak<Bin>>,
    children: Vec<Weak<Bin>>,
    glyph_children: Vec<Arc<Bin>>,
}

pub struct Bin {
    basalt: Arc<Basalt>,
    id: u64,
    hrchy: ArcSwapAny<Arc<BinHrchy>>,
    style: ArcSwapAny<Arc<BinStyle>>,
    initial: Mutex<bool>,
    update: AtomicBool,
    verts: Mutex<
        Vec<(
            Vec<ItfVertInfo>,
            Option<Arc<dyn vulkano::image::traits::ImageViewAccess + Send + Sync>>,
            u64,
        )>,
    >,
    current_text: Mutex<String>,
    is_glyph: AtomicBool,
    back_image: Mutex<Option<ImageInfo>>,
    post_update: RwLock<PostUpdate>,
    on_update: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>,
    on_update_once: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>,
    input_hook_ids: Mutex<Vec<InputHookID>>,
    keep_alive: Mutex<Vec<Arc<dyn KeepAlive + Send + Sync>>>,
    last_update: Mutex<Instant>,
    hook_ids: Mutex<Vec<BinHookID>>,
    used_by_basalt: AtomicBool,
    update_stats: Mutex<BinUpdateStats>,
}

#[derive(Clone, Default, Debug)]
pub struct PostUpdate {
    pub tlo: [f32; 2],
    pub tli: [f32; 2],
    pub blo: [f32; 2],
    pub bli: [f32; 2],
    pub tro: [f32; 2],
    pub tri: [f32; 2],
    pub bro: [f32; 2],
    pub bri: [f32; 2],
    pub z_index: i16,
    pub pre_bound_min_y: f32,
    pub pre_bound_max_y: f32,
}

impl Drop for Bin {
    fn drop(&mut self) {
        for hook in self.input_hook_ids.lock().split_off(0) {
            self.basalt.input_ref().remove_hook(hook);
        }

        self.basalt
            .interface_ref()
            .hook_manager
            .remove_hooks(self.hook_ids.lock().split_off(0));
    }
}

impl Bin {
    pub(crate) fn new(id: u64, basalt: Arc<Basalt>) -> Arc<Self> {
        Arc::new(Bin {
            id,
            basalt,
            hrchy: ArcSwapAny::from(Arc::new(BinHrchy::default())),
            style: ArcSwapAny::new(Arc::new(BinStyle::default())),

            initial: Mutex::new(true),
            update: AtomicBool::new(false),
            verts: Mutex::new(Vec::new()),
            current_text: Mutex::new(String::new()),
            is_glyph: AtomicBool::new(false),
            back_image: Mutex::new(None),
            post_update: RwLock::new(PostUpdate::default()),
            on_update: Mutex::new(Vec::new()),
            on_update_once: Mutex::new(Vec::new()),
            input_hook_ids: Mutex::new(Vec::new()),
            keep_alive: Mutex::new(Vec::new()),
            last_update: Mutex::new(Instant::now()),
            hook_ids: Mutex::new(Vec::new()),
            used_by_basalt: AtomicBool::new(false),
            update_stats: Mutex::new(BinUpdateStats::default()),
        })
    }

    pub fn update_stats(&self) -> BinUpdateStats {
        self.update_stats.lock().clone()
    }

    pub fn is_glyph(&self) -> bool {
        self.is_glyph.load(atomic::Ordering::SeqCst)
    }

    pub fn basalt_use(&self) {
        self.used_by_basalt.store(true, atomic::Ordering::Relaxed);
    }

    pub fn attach_input_hook(&self, id: InputHookID) {
        self.input_hook_ids.lock().push(id);
    }

    pub fn ancestors(&self) -> Vec<Arc<Bin>> {
        let mut out = Vec::new();
        let mut check_wk_op = self.hrchy.load_full().parent.clone();

        while let Some(check_wk) = check_wk_op.take() {
            if let Some(check) = check_wk.upgrade() {
                out.push(check.clone());
                check_wk_op = check.hrchy.load_full().parent.clone();
            }
        }

        out
    }

    pub fn add_hook_raw(self: &Arc<Self>, hook: BinHook, func: BinHookFn) -> BinHookID {
        let id = self.basalt.interface_ref().hook_manager.add_hook(self.clone(), hook, func);
        self.hook_ids.lock().push(id);
        id
    }

    pub fn remove_hook(self: &Arc<Self>, hook_id: BinHookID) {
        self.basalt.interface_ref().hook_manager.remove_hook(hook_id);
        let mut hook_ids = self.hook_ids.lock();

        for i in 0..hook_ids.len() {
            if hook_ids[i] == hook_id {
                hook_ids.swap_remove(i);
                break;
            }
        }
    }

    pub fn on_key_press(self: &Arc<Self>, key: Qwery, func: BinHookFn) -> BinHookID {
        let id = self.basalt.interface_ref().hook_manager.add_hook(
            self.clone(),
            BinHook::Press {
                keys: vec![key],
                mouse_buttons: Vec::new(),
            },
            func,
        );
        self.hook_ids.lock().push(id);
        id
    }

    pub fn on_key_release(self: &Arc<Self>, key: Qwery, func: BinHookFn) -> BinHookID {
        let id = self.basalt.interface_ref().hook_manager.add_hook(
            self.clone(),
            BinHook::Release {
                keys: vec![key],
                mouse_buttons: Vec::new(),
            },
            func,
        );
        self.hook_ids.lock().push(id);
        id
    }

    pub fn on_key_hold(self: &Arc<Self>, key: Qwery, func: BinHookFn) -> BinHookID {
        let id = self.basalt.interface_ref().hook_manager.add_hook(
            self.clone(),
            BinHook::Hold {
                keys: vec![key],
                mouse_buttons: Vec::new(),
                initial_delay: Duration::from_millis(1000),
                interval: Duration::from_millis(100),
                accel: 1.0,
            },
            func,
        );
        self.hook_ids.lock().push(id);
        id
    }

    pub fn on_mouse_press(self: &Arc<Self>, button: MouseButton, func: BinHookFn) -> BinHookID {
        let id = self.basalt.interface_ref().hook_manager.add_hook(
            self.clone(),
            BinHook::Press {
                keys: Vec::new(),
                mouse_buttons: vec![button],
            },
            func,
        );
        self.hook_ids.lock().push(id);
        id
    }

    pub fn on_mouse_release(
        self: &Arc<Self>,
        button: MouseButton,
        func: BinHookFn,
    ) -> BinHookID {
        let id = self.basalt.interface_ref().hook_manager.add_hook(
            self.clone(),
            BinHook::Release {
                keys: Vec::new(),
                mouse_buttons: vec![button],
            },
            func,
        );
        self.hook_ids.lock().push(id);
        id
    }

    pub fn on_mouse_hold(self: &Arc<Self>, button: MouseButton, func: BinHookFn) -> BinHookID {
        let id = self.basalt.interface_ref().hook_manager.add_hook(
            self.clone(),
            BinHook::Hold {
                keys: Vec::new(),
                mouse_buttons: vec![button],
                initial_delay: Duration::from_millis(1000),
                interval: Duration::from_millis(100),
                accel: 1.0,
            },
            func,
        );
        self.hook_ids.lock().push(id);
        id
    }

    pub fn last_update(&self) -> Instant {
        self.last_update.lock().clone()
    }

    pub fn keep_alive(&self, thing: Arc<dyn KeepAlive + Send + Sync>) {
        self.keep_alive.lock().push(thing);
    }

    pub fn parent(&self) -> Option<Arc<Bin>> {
        self.hrchy.load_full().parent.as_ref().and_then(|v| v.upgrade())
    }

    pub fn children(&self) -> Vec<Arc<Bin>> {
        self.hrchy.load_full().children.iter().filter_map(|wk| wk.upgrade()).collect()
    }

    pub fn children_recursive(self: &Arc<Bin>) -> Vec<Arc<Bin>> {
        let mut out = Vec::new();
        let mut to_check = vec![self.clone()];

        while to_check.len() > 0 {
            let child = to_check.pop().unwrap();
            to_check.append(&mut child.children());
            out.push(child);
        }

        out
    }

    pub fn add_child(self: &Arc<Self>, child: Arc<Bin>) {
        let child_hrchy = child.hrchy.load_full();

        child.hrchy.store(Arc::new(BinHrchy {
            parent: Some(Arc::downgrade(self)),
            children: child_hrchy.children.clone(),
            glyph_children: child_hrchy.glyph_children.clone(),
        }));

        let this_hrchy = self.hrchy.load_full();
        let mut children = this_hrchy.children.clone();
        children.push(Arc::downgrade(&child));

        self.hrchy.store(Arc::new(BinHrchy {
            children,
            parent: this_hrchy.parent.clone(),
            glyph_children: this_hrchy.glyph_children.clone(),
        }));
    }

    pub fn add_children(self: &Arc<Self>, children: Vec<Arc<Bin>>) {
        let this_hrchy = self.hrchy.load_full();
        let mut this_children = this_hrchy.children.clone();

        for child in children {
            this_children.push(Arc::downgrade(&child));
            let child_hrchy = child.hrchy.load_full();

            child.hrchy.store(Arc::new(BinHrchy {
                parent: Some(Arc::downgrade(self)),
                children: child_hrchy.children.clone(),
                glyph_children: child_hrchy.glyph_children.clone(),
            }));
        }

        self.hrchy.store(Arc::new(BinHrchy {
            children: this_children,
            parent: this_hrchy.parent.clone(),
            glyph_children: this_hrchy.glyph_children.clone(),
        }));
    }

    pub fn take_children(&self) -> Vec<Arc<Bin>> {
        let this_hrchy = self.hrchy.load_full();
        let mut ret = Vec::new();

        for child in this_hrchy.children.clone() {
            if let Some(child) = child.upgrade() {
                let child_hrchy = child.hrchy.load_full();

                child.hrchy.store(Arc::new(BinHrchy {
                    parent: None,
                    children: child_hrchy.children.clone(),
                    glyph_children: child_hrchy.glyph_children.clone(),
                }));

                ret.push(child);
            }
        }

        self.hrchy.store(Arc::new(BinHrchy {
            children: Vec::new(),
            parent: this_hrchy.parent.clone(),
            glyph_children: this_hrchy.glyph_children.clone(),
        }));

        ret
    }

    pub fn add_drag_events(self: &Arc<Self>, target_op: Option<Arc<Bin>>) {
        #[derive(Default)]
        struct Data {
            target: Weak<Bin>,
            mouse_x: f32,
            mouse_y: f32,
            pos_from_t: Option<f32>,
            pos_from_b: Option<f32>,
            pos_from_l: Option<f32>,
            pos_from_r: Option<f32>,
        }

        let data = Arc::new(Mutex::new(None));
        let target_wk = target_op.map(|v| Arc::downgrade(&v)).unwrap_or(Arc::downgrade(self));
        let data_cp = data.clone();

        self.input_hook_ids.lock().push(self.basalt.input_ref().on_mouse_press(
            MouseButton::Middle,
            Arc::new(move |data| {
                if let InputHookData::Press {
                    mouse_x,
                    mouse_y,
                    ..
                } = data
                {
                    let style = match target_wk.upgrade() {
                        Some(bin) => bin.style_copy(),
                        None => return InputHookRes::Remove,
                    };

                    *data_cp.lock() = Some(Data {
                        target: target_wk.clone(),
                        mouse_x: *mouse_x,
                        mouse_y: *mouse_y,
                        pos_from_t: style.pos_from_t,
                        pos_from_b: style.pos_from_b,
                        pos_from_l: style.pos_from_l,
                        pos_from_r: style.pos_from_r,
                    });
                }

                InputHookRes::Success
            }),
        ));

        let data_cp = data.clone();

        self.input_hook_ids.lock().push(self.basalt.input_ref().add_hook(
            InputHook::MouseMove,
            Arc::new(move |data| {
                if let InputHookData::MouseMove {
                    mouse_x,
                    mouse_y,
                    ..
                } = data
                {
                    let mut data_op = data_cp.lock();
                    let data = match &mut *data_op {
                        Some(some) => some,
                        None => return InputHookRes::Success,
                    };

                    let target = match data.target.upgrade() {
                        Some(some) => some,
                        None => return InputHookRes::Remove,
                    };

                    let dx = mouse_x - data.mouse_x;
                    let dy = mouse_y - data.mouse_y;

                    target.style_update(BinStyle {
                        pos_from_t: data.pos_from_t.as_ref().map(|v| *v + dy),
                        pos_from_b: data.pos_from_b.as_ref().map(|v| *v - dy),
                        pos_from_l: data.pos_from_l.as_ref().map(|v| *v + dx),
                        pos_from_r: data.pos_from_r.as_ref().map(|v| *v - dx),
                        ..target.style_copy()
                    });

                    target.update_children();
                }

                InputHookRes::Success
            }),
        ));

        let data_cp = data.clone();

        self.input_hook_ids.lock().push(self.basalt.input_ref().on_mouse_release(
            MouseButton::Middle,
            Arc::new(move |_| {
                *data_cp.lock() = None;
                InputHookRes::Success
            }),
        ));
    }

    pub fn add_enter_text_events(self: &Arc<Self>) {
        self.add_hook_raw(
            BinHook::Character,
            Arc::new(move |bin, data| {
                if let BinHookData::Character {
                    char_ty,
                    ..
                } = data
                {
                    let mut style = bin.style_copy();

                    match char_ty {
                        Character::Backspace => {
                            style.text.pop();
                        },
                        Character::Value(c) => {
                            style.text.push(*c);
                        },
                    }

                    bin.style_update(style);
                }
            }),
        );
    }

    // TODO: Use Bin Hooks
    pub fn add_button_fade_events(self: &Arc<Self>) {
        let bin = Arc::downgrade(self);
        let focused = Arc::new(AtomicBool::new(false));
        let _focused = focused.clone();
        let previous = Arc::new(Mutex::new(None));
        let _previous = previous.clone();

        self.input_hook_ids.lock().push(self.basalt.input_ref().on_mouse_press(
            MouseButton::Left,
            Arc::new(move |data| {
                if let InputHookData::Press {
                    mouse_x,
                    mouse_y,
                    ..
                } = data
                {
                    let bin = match bin.upgrade() {
                        Some(some) => some,
                        None => return InputHookRes::Remove,
                    };

                    if bin.mouse_inside(*mouse_x, *mouse_y) {
                        if !_focused.swap(true, atomic::Ordering::Relaxed) {
                            let mut copy = bin.style_copy();
                            *_previous.lock() = copy.opacity;
                            copy.opacity = Some(0.5);
                            bin.style_update(copy);
                            bin.update_children();
                        }
                    }
                }

                InputHookRes::Success
            }),
        ));

        let bin = Arc::downgrade(self);

        self.input_hook_ids.lock().push(self.basalt.input_ref().on_mouse_release(
            MouseButton::Left,
            Arc::new(move |_| {
                let bin = match bin.upgrade() {
                    Some(some) => some,
                    None => return InputHookRes::Remove,
                };

                if focused.swap(false, atomic::Ordering::Relaxed) {
                    let mut copy = bin.style_copy();
                    copy.opacity = *previous.lock();
                    bin.style_update(copy);
                    bin.update_children();
                }

                InputHookRes::Success
            }),
        ));
    }

    pub fn fade_out(self: &Arc<Self>, millis: u64) {
        let bin = self.clone();
        let start_opacity = self.style_copy().opacity.unwrap_or(1.0);
        let steps = (millis / 10) as i64;
        let step_size = start_opacity / steps as f32;
        let mut step_i = 0;

        thread::spawn(move || {
            loop {
                if step_i > steps {
                    break;
                }

                let opacity = start_opacity - (step_i as f32 * step_size);
                let mut copy = bin.style_copy();
                copy.opacity = Some(opacity);

                if step_i == steps {
                    copy.hidden = Some(true);
                }

                bin.style_update(copy);
                bin.update_children();
                step_i += 1;
                thread::sleep(Duration::from_millis(10));
            }
        });
    }

    pub fn fade_in(self: &Arc<Self>, millis: u64, target: f32) {
        let bin = self.clone();
        let start_opacity = bin.style_copy().opacity.unwrap_or(1.0);
        let steps = (millis / 10) as i64;
        let step_size = (target - start_opacity) / steps as f32;
        let mut step_i = 0;

        thread::spawn(move || {
            loop {
                if step_i > steps {
                    break;
                }

                let opacity = (step_i as f32 * step_size) + start_opacity;
                let mut copy = bin.style_copy();
                copy.opacity = Some(opacity);
                copy.hidden = Some(false);
                bin.style_update(copy);
                bin.update_children();
                step_i += 1;
                thread::sleep(Duration::from_millis(10));
            }
        });
    }

    pub fn calc_overflow(self: &Arc<Bin>) -> f32 {
        let mut min_y = 0.0;
        let mut max_y = 0.0;

        for child in self.children() {
            let post = child.post_update.read();

            if post.pre_bound_min_y < min_y {
                min_y = post.pre_bound_min_y;
            }

            if post.pre_bound_max_y > max_y {
                max_y = post.pre_bound_max_y;
            }
        }

        let style = self.style();
        let pad_t = style.pad_t.clone().unwrap_or(0.0);
        let pad_b = style.pad_b.clone().unwrap_or(0.0);
        let content_height = max_y - min_y + pad_b + pad_t;
        let self_post = self.post_update.read();

        // For some reason tli[1] doesn't need to be subtracted
        let height = self_post.bli[1]; // 
                                       // - self_post.tli[1];

        if content_height > height {
            content_height - height
        } else {
            0.0
        }
    }

    pub fn on_update(&self, func: Arc<dyn Fn() + Send + Sync>) {
        self.on_update.lock().push(func);
    }

    pub fn on_update_once(&self, func: Arc<dyn Fn() + Send + Sync>) {
        self.on_update_once.lock().push(func);
    }

    pub fn wait_for_update(&self) {
        let barrier = Arc::new(Barrier::new(2));
        let barrier_copy = barrier.clone();

        self.on_update_once(Arc::new(move || {
            barrier_copy.wait();
        }));

        barrier.wait();
    }

    pub fn post_update(&self) -> PostUpdate {
        self.post_update.read().clone()
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn mouse_inside(&self, mouse_x: f32, mouse_y: f32) -> bool {
        if self.is_hidden(None) {
            return false;
        }

        let post = self.post_update.read();

        if mouse_x >= post.tlo[0]
            && mouse_x <= post.tro[0]
            && mouse_y >= post.tlo[1]
            && mouse_y <= post.blo[1]
        {
            return true;
        }

        false
    }

    fn pos_size_tlwh(&self, win_size_: Option<[f32; 2]>) -> (f32, f32, f32, f32) {
        let win_size = win_size_.unwrap_or([0.0, 0.0]);
        let style = self.style();

        let (par_t, par_b, par_l, par_r) =
            match style.position.clone().unwrap_or(BinPosition::Window) {
                BinPosition::Window => (0.0, win_size[1], 0.0, win_size[0]),
                BinPosition::Parent => {
                    match self.parent() {
                        Some(ref parent) => {
                            let (top, left, width, height) = parent.pos_size_tlwh(win_size_);
                            (top, top + height, left, left + width)
                        },
                        None => (0.0, win_size[1], 0.0, win_size[0]),
                    }
                },
                BinPosition::Floating => unimplemented!(),
            };
        let pos_from_t = match style.pos_from_t {
            Some(some) => Some(some),
            None => {
                match style.pos_from_t_pct {
                    Some(some) => Some((some / 100.0) * (par_b - par_t)),
                    None => None,
                }
            },
        };
        let pos_from_b = match style.pos_from_b {
            Some(some) => Some(some),
            None => {
                match style.pos_from_b_pct {
                    Some(some) => Some((some / 100.0) * (par_b - par_t)),
                    None => None,
                }
            },
        };
        let pos_from_l = match style.pos_from_l {
            Some(some) => Some(some),
            None => {
                match style.pos_from_l_pct {
                    Some(some) => Some((some / 100.0) * (par_r - par_l)),
                    None => None,
                }
            },
        };
        let pos_from_r = match style.pos_from_r {
            Some(some) => Some(some),
            None => {
                match style.pos_from_r_pct {
                    Some(some) => Some((some / 100.0) * (par_r - par_l)),
                    None => None,
                }
            },
        };

        let from_t = match pos_from_t {
            Some(from_t) => par_t + from_t,
            None => {
                match pos_from_b {
                    Some(from_b) => {
                        match style.height {
                            Some(height) => par_b - from_b - height,
                            None => {
                                println!(
                                    "UI Bin Warning! ID: {}, Unable to get position from top, \
                                     position from bottom is specified but no height was \
                                     provied.",
                                    self.id
                                );
                                0.0
                            },
                        }
                    },
                    None => {
                        println!(
                            "UI Bin Warning! ID: {}, Unable to get position from top, \
                             position from bottom is non specified.",
                            self.id
                        );
                        0.0
                    },
                }
            },
        } + style.pos_from_t_offset.unwrap_or(0.0);

        let from_l = match pos_from_l {
            Some(from_l) => from_l + par_l,
            None => {
                match pos_from_r {
                    Some(from_r) => {
                        match style.width {
                            Some(width) => par_r - from_r - width,
                            None => {
                                println!(
                                    "UI Bin Warning! ID: {}, Unable to get position from \
                                     left, position from right is specified but no width was \
                                     provided.",
                                    self.id
                                );
                                0.0
                            },
                        }
                    },
                    None => {
                        println!(
                            "UI Bin Warning! ID: {}, Unable to get position fromleft, \
                             position from right is not specified.",
                            self.id
                        );
                        0.0
                    },
                }
            },
        } + style.pos_from_l_offset.unwrap_or(0.0);

        let width = {
            if pos_from_l.is_some() && pos_from_r.is_some() {
                par_r - pos_from_r.unwrap() - from_l
            } else {
                match style.width {
                    Some(some) => some,
                    None => {
                        match style.width_pct {
                            Some(some) => (some / 100.0) * (par_r - par_l),
                            None => {
                                println!(
                                    "UI Bin Warning! ID: {}, Unable to get width. Width must \
                                     be provided or both position from left and right must be \
                                     provided.",
                                    self.id
                                );
                                0.0
                            },
                        }
                    },
                }
            }
        };
        let height = {
            if pos_from_t.is_some() && pos_from_b.is_some() {
                par_b - pos_from_b.unwrap() - from_t
            } else {
                match style.height {
                    Some(some) => some,
                    None => {
                        match style.height_pct {
                            Some(some) => (some / 100.0) * (par_b - par_t),
                            None => {
                                println!(
                                    "UI Bin Warning! ID: {}, Unable to get height. Height \
                                     must be provied or both position from top and bottom \
                                     must be provied.",
                                    self.id
                                );
                                0.0
                            },
                        }
                    },
                }
            }
        };

        (from_t, from_l, width, height)
    }

    pub fn visible(&self) -> bool {
        !self.is_hidden(None)
    }

    pub fn toggle_hidden(&self) {
        let mut style = self.style_copy();
        style.hidden = Some(!style.hidden.unwrap_or(false));
        self.style_update(style);
    }

    fn is_hidden(&self, style_: Option<&BinStyle>) -> bool {
        match match style_ {
            Some(style) => {
                match style.hidden {
                    Some(hide) => hide,
                    None => false,
                }
            },
            None => {
                match self.style().hidden {
                    Some(hide) => hide,
                    None => false,
                }
            },
        } {
            true => true,
            false => {
                match self.parent() {
                    Some(parent) => parent.is_hidden(None),
                    None => false,
                }
            },
        }
    }

    pub(crate) fn verts_cp(
        &self,
    ) -> Vec<(
        Vec<ItfVertInfo>,
        Option<Arc<dyn vulkano::image::traits::ImageViewAccess + Send + Sync>>,
        u64,
    )> {
        self.verts.lock().clone()
    }

    pub(crate) fn wants_update(&self) -> bool {
        self.update.load(atomic::Ordering::SeqCst)
    }

    pub(crate) fn update_text(self: &Arc<Self>, scale: f32) -> Vec<Arc<Bin>> {
        let style = self.style();
        let mut last_text = self.current_text.lock();
        let hrchy = self.hrchy.load_full();

        if style.text.len() == 0 {
            *last_text = String::new();

            self.hrchy.store(Arc::new(BinHrchy {
                parent: hrchy.parent.clone(),
                children: hrchy.children.clone(),
                glyph_children: Vec::new(),
            }));

            return Vec::new();
        }

        let mut glyph_children = hrchy.glyph_children.clone();
        let post_update = self.post_update();

        // if style.text == *last_text {
        // return glyph_children;
        // }

        *last_text = style.text.clone();
        let pad_t = style.pad_t.clone().unwrap_or(0.0);
        let pad_b = style.pad_b.clone().unwrap_or(0.0);
        let pad_l = style.pad_l.clone().unwrap_or(0.0);
        let pad_r = style.pad_r.clone().unwrap_or(0.0);
        let body_width = (post_update.tri[0] - post_update.tli[0] - pad_l - pad_r) * scale;
        let body_height = (post_update.bli[1] - post_update.tli[1] - pad_t - pad_b) * scale;
        let color = style.text_color.clone().unwrap_or(Color::srgb_hex("000000"));
        let text_height = style.text_height.clone().unwrap_or(12.0);
        let text_wrap = style.text_wrap.clone().unwrap_or(ImtTextWrap::NewLine);
        let vert_align = style.text_vert_align.clone().unwrap_or(ImtVertAlign::Top);
        let hori_align = style.text_hori_align.clone().unwrap_or(ImtHoriAlign::Left);
        let line_spacing = style.line_spacing.clone().unwrap_or(0.0);

        let glyphs = match self.basalt.interface_ref().ilmenite.glyphs_for_text(
            "ABeeZee".into(),
            ImtWeight::Normal,
            text_height * scale,
            Some(ImtShapeOpts {
                body_width,
                body_height,
                text_height,
                line_spacing,
                text_wrap,
                vert_align,
                hori_align,
            }),
            style.text.clone(),
        ) {
            Ok(ok) => ok,
            Err(e) => {
                println!("Failed to generate text glyphs: {:?}", e);
                return Vec::new();
            },
        };

        if glyph_children.len() < glyphs.len() {
            let len = glyph_children.len();

            glyph_children.append(
                &mut self
                    .basalt
                    .interface_ref()
                    .new_bins(glyphs.len() - len)
                    .into_iter()
                    .map(|b| {
                        b.is_glyph.store(true, atomic::Ordering::SeqCst);
                        self.add_child(b.clone());
                        b
                    })
                    .collect(),
            );
        } else {
            glyph_children.truncate(glyphs.len());
        }

        for (i, glyph) in glyphs.into_iter().enumerate() {
            let cache_id = SubImageCacheID::Glyph(
                glyph.family,
                glyph.weight,
                glyph.index,
                OrderedFloat::from(text_height),
            );

            let coords = match self.basalt.atlas_ref().cache_coords(cache_id.clone()) {
                Some(mut coords) => {
                    coords.w -= glyph.crop_x.round() as u32;
                    coords.h -= glyph.crop_y.round() as u32;
                    Some(coords)
                },
                None => {
                    if glyph.w == 0 || glyph.h == 0 {
                        None
                    } else {
                        Some(
                            self.basalt
                                .atlas_ref()
                                .load_image(
                                    cache_id,
                                    Image::new(
                                        ImageType::LMono,
                                        ImageDims {
                                            w: glyph.w,
                                            h: glyph.h,
                                        },
                                        ImageData::D8(
                                            glyph
                                                .bitmap
                                                .into_iter()
                                                .map(|v| {
                                                    (v * u8::max_value() as f32).round() as u8
                                                })
                                                .collect(),
                                        ),
                                    )
                                    .unwrap(),
                                )
                                .unwrap(),
                        )
                    }
                },
            };

            glyph_children[i].style_update(BinStyle {
                position: Some(BinPosition::Parent),
                pos_from_t: Some((glyph.y / scale) + pad_t),
                pos_from_l: Some((glyph.x / scale) + pad_l),
                width: Some((glyph.w as f32 - glyph.crop_x) / scale),
                height: Some((glyph.h as f32 - glyph.crop_y) / scale),
                back_image_atlas: coords,
                back_color: Some(color.clone()),
                ..BinStyle::default()
            });
        }

        self.hrchy.store(Arc::new(BinHrchy {
            parent: hrchy.parent.clone(),
            children: hrchy.children.clone(),
            glyph_children: glyph_children.clone(),
        }));

        glyph_children
    }

    pub(crate) fn do_update(self: &Arc<Self>, win_size: [f32; 2], scale: f32) {
        let update_stats = self.basalt.show_bin_stats();
        let mut stats = BinUpdateStats::default();
        let mut inst = Instant::now();

        if *self.initial.lock() {
            return;
        }
        self.update.store(false, atomic::Ordering::SeqCst);

        if update_stats {
            stats.t_upcheck = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        let style = self.style();
        let scaled_win_size = [win_size[0] / scale, win_size[1] / scale];

        if update_stats {
            stats.t_style_obtain = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        if self.is_hidden(Some(&style)) {
            *self.verts.lock() = Vec::new();
            *self.last_update.lock() = Instant::now();
            return;
        }

        if update_stats {
            stats.t_hidden = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        let ancestor_data: Vec<(Arc<Bin>, Arc<BinStyle>, f32, f32, f32, f32)> = self
            .ancestors()
            .into_iter()
            .map(|bin| {
                let (top, left, width, height) = bin.pos_size_tlwh(Some(scaled_win_size));
                (bin.clone(), bin.style(), top, left, width, height)
            })
            .collect();

        if update_stats {
            stats.t_ancestors = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        let (top, left, width, height) = self.pos_size_tlwh(Some(scaled_win_size));
        let border_size_t = style.border_size_t.clone().unwrap_or(0.0);
        let border_size_b = style.border_size_b.clone().unwrap_or(0.0);
        let border_size_l = style.border_size_l.clone().unwrap_or(0.0);
        let border_size_r = style.border_size_r.clone().unwrap_or(0.0);
        let mut border_color_t = style.border_color_t.clone().unwrap_or(Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        });
        let mut border_color_b = style.border_color_b.clone().unwrap_or(Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        });
        let mut border_color_l = style.border_color_l.clone().unwrap_or(Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        });
        let mut border_color_r = style.border_color_r.clone().unwrap_or(Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        });
        let mut back_color = style.back_color.clone().unwrap_or(Color {
            r: 0.0,
            b: 0.0,
            g: 0.0,
            a: 0.0,
        });

        if update_stats {
            stats.t_position = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // -- z-index calc ------------------------------------------------------------- //

        let mut z_index = match style.z_index.as_ref() {
            Some(some) => *some,
            None => {
                let mut z_index_op = None;
                let mut checked = 0;

                for (_, check_style, ..) in &ancestor_data {
                    match check_style.z_index.as_ref() {
                        Some(some) => {
                            z_index_op = Some(*some + checked + 1);
                            break;
                        },
                        None => {
                            checked += 1;
                        },
                    }
                }

                z_index_op.unwrap_or(ancestor_data.len() as i16)
            },
        } + style.add_z_index.clone().unwrap_or(0);

        if self.used_by_basalt.load(atomic::Ordering::Relaxed) {
            z_index += ::std::i16::MAX - 100;
        } else if z_index >= ::std::i16::MAX - 100 {
            println!("Max z-index of {} reached!", ::std::i16::MAX - 101);
            z_index = ::std::i16::MAX - 101;
        }

        if update_stats {
            stats.t_zindex = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // -- create post update ------------------------------------------------------- //

        let mut bps = PostUpdate {
            tlo: [left - border_size_l, top - border_size_t],
            tli: [left, top],
            blo: [left - border_size_l, top + height + border_size_b],
            bli: [left, top + height],
            tro: [left + width + border_size_r, top - border_size_t],
            tri: [left + width, top],
            bro: [left + width + border_size_r, top + height + border_size_b],
            bri: [left + width, top + height],
            z_index,
            pre_bound_min_y: 0.0,
            pre_bound_max_y: 0.0,
        };

        if update_stats {
            stats.t_postset = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // -- Background Image --------------------------------------------------------- //

        let (back_img, back_coords) = match &*self.back_image.lock() {
            &Some(ref img_info) => {
                match &img_info.image {
                    &Some(ref img) => (Some(img.clone()), img_info.coords.clone()),
                    &None => (None, img_info.coords.clone()),
                }
            },
            &None => {
                match style.back_image.as_ref() {
                    Some(path) => {
                        match self.basalt.atlas_ref().load_image_from_path(path) {
                            Ok(coords) => (None, coords),
                            Err(e) => {
                                println!(
                                    "UI Bin Warning! ID: {}, failed to load image into atlas \
                                     {}: {}",
                                    self.id, path, e
                                );
                                (None, atlas::Coords::none())
                            },
                        }
                    },
                    None => {
                        match style.back_image_url.as_ref() {
                            Some(url) => {
                                match self.basalt.atlas_ref().load_image_from_url(url) {
                                    Ok(coords) => (None, coords),
                                    Err(e) => {
                                        println!(
                                            "UI Bin Warning! ID: {}, failed to load image \
                                             into atlas {}: {}",
                                            self.id, url, e
                                        );
                                        (None, atlas::Coords::none())
                                    },
                                }
                            },
                            None => {
                                match style.back_image_atlas.clone() {
                                    Some(coords) => (None, coords),
                                    None => (None, atlas::Coords::none()),
                                }
                            },
                        }
                    },
                }
            },
        };

        let back_img_vert_ty = match self.is_glyph() {
            true => 2,
            false => {
                match style.back_srgb_yuv.as_ref() {
                    Some(some) => {
                        match some {
                            &true => 101,
                            &false => {
                                match style.back_image_effect.as_ref() {
                                    Some(some) => some.vert_type(),
                                    None => 100,
                                }
                            },
                        }
                    },
                    None => {
                        match style.back_image_effect.as_ref() {
                            Some(some) => some.vert_type(),
                            None => 100,
                        }
                    },
                }
            },
        };

        if update_stats {
            stats.t_image = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // -- Opacity ------------------------------------------------------------------ //

        let mut opacity = style.opacity.clone().unwrap_or(1.0);

        for (_, check_style, ..) in &ancestor_data {
            opacity *= check_style.opacity.clone().unwrap_or(1.0);
        }

        if opacity != 1.0 {
            border_color_t.a *= opacity;
            border_color_b.a *= opacity;
            border_color_l.a *= opacity;
            border_color_r.a *= opacity;
            back_color.a *= opacity;
        }

        if update_stats {
            stats.t_opacity = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // ----------------------------------------------------------------------------- //

        let base_z =
            ((-1 * z_index) as i32 + i16::max_value() as i32) as f32 / i32::max_value() as f32;
        let content_z = ((-1 * (z_index + 1)) as i32 + i16::max_value() as i32) as f32
            / i32::max_value() as f32;
        let mut verts = Vec::with_capacity(54);

        let border_radius_tl = style.border_radius_tl.clone().unwrap_or(0.0);
        let border_radius_tr = style.border_radius_tr.clone().unwrap_or(0.0);
        let border_radius_bl = style.border_radius_bl.clone().unwrap_or(0.0);
        let border_radius_br = style.border_radius_br.clone().unwrap_or(0.0);

        if border_radius_tl != 0.0
            || border_radius_tr != 0.0
            || border_radius_bl != 0.0
            || border_radius_br != 0.0
        {
            let border_radius_tmax = if border_radius_tl > border_radius_tr {
                border_radius_tl
            } else {
                border_radius_tr
            };

            let border_radius_bmax = if border_radius_bl > border_radius_br {
                border_radius_bl
            } else {
                border_radius_br
            };

            if back_color.a > 0.0 || back_coords.img_id != 0 || back_img.is_some() {
                let mut back_verts = Vec::new();

                if border_radius_tl != 0.0 || border_radius_tr != 0.0 {
                    back_verts.push((bps.tri[0] - border_radius_tr, bps.tri[1]));
                    back_verts.push((bps.tli[0] + border_radius_tl, bps.tli[1]));
                    back_verts
                        .push((bps.tli[0] + border_radius_tl, bps.tli[1] + border_radius_tmax));
                    back_verts.push((bps.tri[0] - border_radius_tr, bps.tri[1]));
                    back_verts
                        .push((bps.tli[0] + border_radius_tl, bps.tli[1] + border_radius_tmax));
                    back_verts
                        .push((bps.tri[0] - border_radius_tr, bps.tri[1] + border_radius_tmax));

                    if border_radius_tl > border_radius_tr {
                        back_verts.push((bps.tri[0], bps.tri[1] + border_radius_tr));
                        back_verts.push((
                            bps.tri[0] - border_radius_tr,
                            bps.tri[1] + border_radius_tr,
                        ));
                        back_verts.push((
                            bps.tri[0] - border_radius_tr,
                            bps.tri[1] + border_radius_tmax,
                        ));
                        back_verts.push((bps.tri[0], bps.tri[1] + border_radius_tr));
                        back_verts.push((
                            bps.tri[0] - border_radius_tr,
                            bps.tri[1] + border_radius_tmax,
                        ));
                        back_verts.push((bps.tri[0], bps.tri[1] + border_radius_tmax));
                    } else if border_radius_tr > border_radius_tl {
                        back_verts.push((
                            bps.tli[0] + border_radius_tl,
                            bps.tli[1] + border_radius_tl,
                        ));
                        back_verts.push((bps.tli[0], bps.tli[1] + border_radius_tl));
                        back_verts.push((bps.tli[0], bps.tli[1] + border_radius_tmax));
                        back_verts.push((
                            bps.tli[0] + border_radius_tl,
                            bps.tli[1] + border_radius_tl,
                        ));
                        back_verts.push((bps.tli[0], bps.tli[1] + border_radius_tmax));
                        back_verts.push((
                            bps.tli[0] + border_radius_tl,
                            bps.tli[1] + border_radius_tmax,
                        ));
                    }
                }

                if border_radius_bl != 0.0 || border_radius_br != 0.0 {
                    back_verts
                        .push((bps.bri[0] - border_radius_br, bps.bri[1] - border_radius_bmax));
                    back_verts
                        .push((bps.bli[0] + border_radius_bl, bps.bli[1] - border_radius_bmax));
                    back_verts.push((bps.bli[0] + border_radius_bl, bps.bli[1]));
                    back_verts
                        .push((bps.bri[0] - border_radius_br, bps.bri[1] - border_radius_bmax));
                    back_verts.push((bps.bli[0] + border_radius_bl, bps.bli[1]));
                    back_verts.push((bps.bri[0] - border_radius_br, bps.bri[1]));

                    if border_radius_bl > border_radius_br {
                        back_verts.push((bps.bri[0], bps.bri[1] - border_radius_bmax));
                        back_verts.push((
                            bps.bri[0] - border_radius_br,
                            bps.bri[1] - border_radius_bmax,
                        ));
                        back_verts.push((
                            bps.bri[0] - border_radius_br,
                            bps.bri[1] - border_radius_br,
                        ));
                        back_verts.push((bps.bri[0], bps.bri[1] - border_radius_bmax));
                        back_verts.push((
                            bps.bri[0] - border_radius_br,
                            bps.bri[1] - border_radius_br,
                        ));
                        back_verts.push((bps.bri[0], bps.bri[1] - border_radius_br));
                    } else if border_radius_br > border_radius_bl {
                        back_verts.push((
                            bps.bli[0] + border_radius_bl,
                            bps.bli[1] - border_radius_bmax,
                        ));
                        back_verts.push((bps.bli[0], bps.bli[1] - border_radius_bmax));
                        back_verts.push((bps.bli[0], bps.bli[1] - border_radius_bl));
                        back_verts.push((
                            bps.bli[0] + border_radius_bl,
                            bps.bli[1] - border_radius_bmax,
                        ));
                        back_verts.push((bps.bli[0], bps.bli[1] - border_radius_bl));
                        back_verts.push((
                            bps.bli[0] + border_radius_bl,
                            bps.bli[1] - border_radius_bl,
                        ));
                    }
                }

                if border_radius_tl != 0.0 {
                    let triangles = border_radius_tl.ceil() as usize * 2;
                    let step_size = (0.5 * PI) / triangles as f32;
                    let base = PI / 2.0;
                    let mut points = Vec::new();

                    for i in 0..(triangles + 1) {
                        points.push((
                            (bps.tli[0] + border_radius_tl)
                                + (border_radius_tl * f32::cos(base + (step_size * i as f32))),
                            (bps.tli[1] + border_radius_tl)
                                - (border_radius_tl * f32::sin(base + (step_size * i as f32))),
                        ));
                    }

                    for i in 0..triangles {
                        back_verts.push(points[i].clone());
                        back_verts.push(points[i + 1].clone());
                        back_verts.push((
                            bps.tli[0] + border_radius_tl,
                            bps.tli[1] + border_radius_tl,
                        ));
                    }
                }

                if border_radius_tr != 0.0 {
                    let triangles = border_radius_tr.ceil() as usize * 2;
                    let step_size = (0.5 * PI) / triangles as f32;
                    let base = 0.0;
                    let mut points = Vec::new();

                    for i in 0..(triangles + 1) {
                        points.push((
                            (bps.tri[0] - border_radius_tr)
                                + (border_radius_tr * f32::cos(base + (step_size * i as f32))),
                            (bps.tri[1] + border_radius_tr)
                                - (border_radius_tr * f32::sin(base + (step_size * i as f32))),
                        ));
                    }

                    for i in 0..triangles {
                        back_verts.push(points[i].clone());
                        back_verts.push(points[i + 1].clone());
                        back_verts.push((
                            bps.tri[0] - border_radius_tl,
                            bps.tri[1] + border_radius_tr,
                        ));
                    }
                }

                if border_radius_bl != 0.0 {
                    let triangles = border_radius_bl.ceil() as usize * 2;
                    let step_size = (0.5 * PI) / triangles as f32;
                    let base = PI;
                    let mut points = Vec::new();

                    for i in 0..(triangles + 1) {
                        points.push((
                            (bps.bli[0] + border_radius_bl)
                                + (border_radius_bl * f32::cos(base + (step_size * i as f32))),
                            (bps.bli[1] - border_radius_bl)
                                - (border_radius_bl * f32::sin(base + (step_size * i as f32))),
                        ));
                    }

                    for i in 0..triangles {
                        back_verts.push(points[i].clone());
                        back_verts.push(points[i + 1].clone());
                        back_verts.push((
                            bps.bli[0] + border_radius_bl,
                            bps.bli[1] - border_radius_bl,
                        ));
                    }
                }

                if border_radius_br != 0.0 {
                    let triangles = border_radius_br.ceil() as usize * 2;
                    let step_size = (0.5 * PI) / triangles as f32;
                    let base = PI * 1.5;
                    let mut points = Vec::new();

                    for i in 0..(triangles + 1) {
                        points.push((
                            (bps.bri[0] - border_radius_br)
                                + (border_radius_br * f32::cos(base + (step_size * i as f32))),
                            (bps.bri[1] - border_radius_br)
                                - (border_radius_br * f32::sin(base + (step_size * i as f32))),
                        ));
                    }

                    for i in 0..triangles {
                        back_verts.push(points[i].clone());
                        back_verts.push(points[i + 1].clone());
                        back_verts.push((
                            bps.bri[0] - border_radius_br,
                            bps.bri[1] - border_radius_br,
                        ));
                    }
                }

                back_verts.push((bps.tri[0], bps.tri[1] + border_radius_tmax));
                back_verts.push((bps.tli[0], bps.tli[1] + border_radius_tmax));
                back_verts.push((bps.bli[0], bps.bli[1] - border_radius_bmax));
                back_verts.push((bps.tri[0], bps.tri[1] + border_radius_tmax));
                back_verts.push((bps.bli[0], bps.bli[1] - border_radius_bmax));
                back_verts.push((bps.bri[0], bps.bri[1] - border_radius_bmax));

                let ty = if back_coords.img_id != 0 || back_img.is_some() {
                    back_img_vert_ty
                } else {
                    0
                };

                for (x, y) in back_verts {
                    let coords_x = (((x - bps.tli[0]) / (bps.tri[0] - bps.tli[0]))
                        * back_coords.w as f32)
                        + back_coords.x as f32;
                    let coords_y = (((y - bps.tli[1]) / (bps.bli[1] - bps.tli[1]))
                        * back_coords.h as f32)
                        + back_coords.y as f32;
                    verts.push(ItfVertInfo {
                        position: (x, y, base_z),
                        coords: (coords_x, coords_y),
                        color: back_color.as_tuple(),
                        ty,
                    });
                }
            }
        } else {
            if border_color_t.a > 0.0 && border_size_t > 0.0 {
                // Top Border
                verts.push(ItfVertInfo {
                    position: (bps.tri[0], bps.tro[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_t.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tli[0], bps.tlo[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_t.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tli[0], bps.tli[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_t.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tri[0], bps.tro[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_t.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tli[0], bps.tli[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_t.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tri[0], bps.tri[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_t.as_tuple(),
                    ty: 0,
                });
            }
            if border_color_b.a > 0.0 && border_size_b > 0.0 {
                // Bottom Border
                verts.push(ItfVertInfo {
                    position: (bps.bri[0], bps.bri[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_b.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.bli[0], bps.bli[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_b.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.bli[0], bps.blo[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_b.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.bri[0], bps.bri[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_b.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.bli[0], bps.blo[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_b.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.bri[0], bps.bro[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_b.as_tuple(),
                    ty: 0,
                });
            }
            if border_color_l.a > 0.0 && border_size_l > 0.0 {
                // Left Border
                verts.push(ItfVertInfo {
                    position: (bps.tli[0], bps.tli[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_l.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tlo[0], bps.tli[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_l.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.blo[0], bps.bli[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_l.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tli[0], bps.tli[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_l.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.blo[0], bps.bli[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_l.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.bli[0], bps.bli[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_l.as_tuple(),
                    ty: 0,
                });
            }
            if border_color_r.a > 0.0 && border_size_r > 0.0 {
                // Right Border
                verts.push(ItfVertInfo {
                    position: (bps.tro[0], bps.tri[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_r.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tri[0], bps.tri[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_r.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.bri[0], bps.bri[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_r.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tro[0], bps.tri[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_r.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.bri[0], bps.bri[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_r.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.bro[0], bps.bri[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_r.as_tuple(),
                    ty: 0,
                });
            }
            if border_color_t.a > 0.0
                && border_size_t > 0.0
                && border_color_l.a > 0.0
                && border_size_l > 0.0
            {
                // Top Left Border Corner (Color of Left)
                verts.push(ItfVertInfo {
                    position: (bps.tlo[0], bps.tlo[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_l.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tlo[0], bps.tli[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_l.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tli[0], bps.tli[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_l.as_tuple(),
                    ty: 0,
                });
                // Top Left Border Corner (Color of Top)
                verts.push(ItfVertInfo {
                    position: (bps.tli[0], bps.tlo[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_t.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tlo[0], bps.tlo[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_t.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tli[0], bps.tli[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_t.as_tuple(),
                    ty: 0,
                });
            }
            if border_color_t.a > 0.0
                && border_size_t > 0.0
                && border_color_r.a > 0.0
                && border_size_r > 0.0
            {
                // Top Right Border Corner (Color of Right)
                verts.push(ItfVertInfo {
                    position: (bps.tro[0], bps.tro[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_r.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tri[0], bps.tri[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_r.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tro[0], bps.tri[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_r.as_tuple(),
                    ty: 0,
                });
                // Top Right Border Corner (Color of Top)
                verts.push(ItfVertInfo {
                    position: (bps.tro[0], bps.tro[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_t.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tri[0], bps.tro[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_t.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tri[0], bps.tri[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_t.as_tuple(),
                    ty: 0,
                });
            }
            if border_color_b.a > 0.0
                && border_size_b > 0.0
                && border_color_l.a > 0.0
                && border_size_l > 0.0
            {
                // Bottom Left Border Corner (Color of Left)
                verts.push(ItfVertInfo {
                    position: (bps.bli[0], bps.bli[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_l.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.blo[0], bps.bli[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_l.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.blo[0], bps.blo[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_l.as_tuple(),
                    ty: 0,
                });
                // Bottom Left Border Corner (Color of Bottom)
                verts.push(ItfVertInfo {
                    position: (bps.bli[0], bps.bli[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_b.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.blo[0], bps.blo[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_b.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.bli[0], bps.blo[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_b.as_tuple(),
                    ty: 0,
                });
            }
            if border_color_b.a > 0.0
                && border_size_b > 0.0
                && border_color_r.a > 0.0
                && border_size_r > 0.0
            {
                // Bottom Right Border Corner (Color of Right)
                verts.push(ItfVertInfo {
                    position: (bps.bro[0], bps.bri[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_r.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.bri[0], bps.bri[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_r.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.bro[0], bps.bro[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_r.as_tuple(),
                    ty: 0,
                });
                // Bottom Right Border Corner (Color of Bottom)
                verts.push(ItfVertInfo {
                    position: (bps.bri[0], bps.bri[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_b.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.bri[0], bps.bro[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_b.as_tuple(),
                    ty: 0,
                });
                verts.push(ItfVertInfo {
                    position: (bps.bro[0], bps.bro[1], 0.0),
                    coords: (0.0, 0.0),
                    color: border_color_b.as_tuple(),
                    ty: 0,
                });
            }
            if back_color.a > 0.0 || back_coords.img_id != 0 || back_img.is_some() {
                let ty = if back_coords.img_id != 0 || back_img.is_some() {
                    back_img_vert_ty
                } else {
                    0
                };

                verts.push(ItfVertInfo {
                    position: (bps.tri[0], bps.tri[1], base_z),
                    coords: back_coords.top_right(),
                    color: back_color.as_tuple(),
                    ty,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tli[0], bps.tli[1], base_z),
                    coords: back_coords.top_left(),
                    color: back_color.as_tuple(),
                    ty,
                });
                verts.push(ItfVertInfo {
                    position: (bps.bli[0], bps.bli[1], base_z),
                    coords: back_coords.bottom_left(),
                    color: back_color.as_tuple(),
                    ty,
                });
                verts.push(ItfVertInfo {
                    position: (bps.tri[0], bps.tri[1], base_z),
                    coords: back_coords.top_right(),
                    color: back_color.as_tuple(),
                    ty,
                });
                verts.push(ItfVertInfo {
                    position: (bps.bli[0], bps.bli[1], base_z),
                    coords: back_coords.bottom_left(),
                    color: back_color.as_tuple(),
                    ty,
                });
                verts.push(ItfVertInfo {
                    position: (bps.bri[0], bps.bri[1], base_z),
                    coords: back_coords.bottom_right(),
                    color: back_color.as_tuple(),
                    ty,
                });
            }
        }

        for &BinVert {
            ref position,
            ref color,
        } in &style.custom_verts
        {
            let z = if position.2 == 0 {
                content_z
            } else {
                ((-1 * (z_index + position.2)) as i32 + i16::max_value() as i32) as f32
                    / i32::max_value() as f32
            };

            verts.push(ItfVertInfo {
                position: (bps.tli[0] + position.0, bps.tli[1] + position.1, z),
                coords: (0.0, 0.0),
                color: color.as_tuple(),
                ty: 0,
            });
        }

        let mut vert_data = vec![(verts, back_img, back_coords.img_id)];

        for &mut (ref mut verts, ..) in &mut vert_data {
            for vert in verts {
                if vert.position.2 == 0.0 {
                    vert.position.2 = base_z;
                }
            }
        }

        if update_stats {
            stats.t_verts = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // -- Get current content height before overflow checks ------------------------ //

        for (verts, ..) in &mut vert_data {
            for vert in verts {
                if vert.position.1 < bps.pre_bound_min_y {
                    bps.pre_bound_min_y = vert.position.1;
                }

                if vert.position.1 > bps.pre_bound_max_y {
                    bps.pre_bound_max_y = vert.position.1;
                }
            }
        }

        // -- Make sure that the verts are within the boundries of all ancestors. ------ //
        // TODO: Implement horizonal checks

        let mut cut_amt;
        let mut cut_percent;
        let mut pos_min_y;
        let mut pos_max_y;
        let mut coords_min_y;
        let mut coords_max_y;
        let mut tri_h;
        let mut img_h;

        for (_check_bin, check_style, check_pft, _check_pfl, _check_w, check_h) in
            &ancestor_data
        {
            let scroll_y = check_style.scroll_y.clone().unwrap_or(0.0);
            let overflow_y = check_style.overflow_y.clone().unwrap_or(false);
            let check_b = *check_pft + *check_h;

            if !overflow_y {
                let bps_check_y: Vec<&mut f32> = vec![
                    &mut bps.tli[1],
                    &mut bps.tri[1],
                    &mut bps.bli[1],
                    &mut bps.bri[1],
                    &mut bps.tlo[1],
                    &mut bps.tro[1],
                    &mut bps.blo[1],
                    &mut bps.bro[1],
                ];

                for y in bps_check_y {
                    *y -= scroll_y;

                    if *y < *check_pft {
                        *y = *check_pft;
                    } else if *y > check_b {
                        *y = check_b;
                    }
                }
            }

            for (verts, ..) in &mut vert_data {
                let mut rm_tris: Vec<usize> = Vec::new();

                for (tri_i, tri) in verts.chunks_mut(3).enumerate() {
                    tri[0].position.1 -= scroll_y;
                    tri[1].position.1 -= scroll_y;
                    tri[2].position.1 -= scroll_y;

                    if !overflow_y {
                        if (tri[0].position.1 < *check_pft
                            && tri[1].position.1 < *check_pft
                            && tri[2].position.1 < *check_pft)
                            || (tri[0].position.1 > check_b
                                && tri[1].position.1 > check_b
                                && tri[2].position.1 > check_b)
                        {
                            rm_tris.push(tri_i);
                        } else {
                            pos_min_y = misc::partial_ord_min3(
                                tri[0].position.1,
                                tri[1].position.1,
                                tri[2].position.1,
                            );
                            pos_max_y = misc::partial_ord_max3(
                                tri[0].position.1,
                                tri[1].position.1,
                                tri[2].position.1,
                            );
                            coords_min_y = misc::partial_ord_min3(
                                tri[0].coords.1,
                                tri[1].coords.1,
                                tri[2].coords.1,
                            );
                            coords_max_y = misc::partial_ord_max3(
                                tri[0].coords.1,
                                tri[1].coords.1,
                                tri[2].coords.1,
                            );
                            tri_h = pos_max_y - pos_min_y;
                            img_h = coords_max_y - coords_min_y;

                            for vert in tri {
                                if vert.position.1 < *check_pft {
                                    cut_amt = check_pft - vert.position.1;
                                    cut_percent = cut_amt / tri_h;
                                    vert.coords.1 += cut_percent * img_h;
                                    vert.position.1 += cut_amt;
                                } else if vert.position.1 > check_b {
                                    cut_amt = vert.position.1 - check_b;
                                    cut_percent = cut_amt / tri_h;
                                    vert.coords.1 -= cut_percent * img_h;
                                    vert.position.1 -= cut_amt;
                                }
                            }
                        }
                    }
                }

                for tri_i in rm_tris.into_iter().rev() {
                    for i in (0..3).into_iter().rev() {
                        verts.swap_remove((tri_i * 3) + i);
                    }
                }
            }
        }

        if update_stats {
            stats.t_overflow = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // if bps.pre_bound_max_y - bps.pre_bound_min_y > bps.bli[1] - bps.tli[1] {
        // println!("{} {}", bps.pre_bound_min_y, bps.pre_bound_max_y);
        // }

        // ----------------------------------------------------------------------------- //

        for &mut (ref mut verts, ..) in &mut vert_data {
            scale_verts(&[win_size[0], win_size[1]], scale, verts);
        }

        if update_stats {
            stats.t_scale = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        *self.verts.lock() = vert_data;
        *self.post_update.write() = bps;
        *self.last_update.lock() = Instant::now();

        if update_stats {
            stats.t_locks = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        let mut funcs = self.on_update.lock().clone();
        funcs.append(&mut self.on_update_once.lock().split_off(0));

        for func in funcs {
            func();
        }

        if update_stats {
            stats.t_callbacks = inst.elapsed();
            stats.t_total += inst.elapsed();
        } else {
            stats.t_total = inst.elapsed();
        }

        *self.update_stats.lock() = stats;
    }

    pub fn force_update(&self) {
        self.update.store(true, atomic::Ordering::SeqCst);
        self.basalt.interface_ref().odb.unpark();
    }

    pub fn update_children(&self) {
        self.update_children_priv(false);
    }

    fn update_children_priv(&self, update_self: bool) {
        if update_self {
            self.update.store(true, atomic::Ordering::SeqCst);
        }

        for child in self.children().into_iter().chain(self.hrchy.load().glyph_children.clone())
        {
            child.update_children_priv(true);
        }
    }

    pub fn style(&self) -> Arc<BinStyle> {
        self.style.load().clone()
    }

    pub fn style_copy(&self) -> BinStyle {
        self.style.load().as_ref().clone()
    }

    pub fn style_update(&self, copy: BinStyle) {
        self.style.store(Arc::new(copy));
        *self.initial.lock() = false;
        self.update.store(true, atomic::Ordering::SeqCst);
        self.basalt.interface_ref().odb.unpark();
    }

    pub fn hidden(self: &Arc<Self>, to: Option<bool>) {
        let mut copy = self.style_copy();
        copy.hidden = to;
        self.style_update(copy);
        self.update_children();
    }

    pub fn set_raw_back_img(&self, img: Arc<dyn ImageViewAccess + Send + Sync>) {
        let mut coords = atlas::Coords::none();
        coords.w = 1;
        coords.h = 1;

        *self.back_image.lock() = Some(ImageInfo {
            image: Some(img),
            coords,
        });

        self.update.store(true, atomic::Ordering::SeqCst);
        self.basalt.interface_ref().odb.unpark();
    }

    pub fn set_raw_img_yuv_422(
        &self,
        width: u32,
        height: u32,
        data: Vec<u8>,
    ) -> Result<(), String> {
        use vulkano::sync::GpuFuture;

        let mut back_image = self.back_image.lock();

        let (img, future) = ImmutableImage::from_iter(
            data.into_iter(),
            vulkano::image::Dimensions::Dim2d {
                width,
                height: height + (height / 2),
            },
            vulkano::format::Format::R8Unorm,
            self.basalt.transfer_queue(),
        )
        .unwrap();

        let fence = future.then_signal_fence_and_flush().unwrap();
        fence.wait(None).unwrap();

        let mut coords = atlas::Coords::none();
        coords.w = 1;
        coords.h = 1;

        *back_image = Some(ImageInfo {
            image: Some(img),
            coords,
        });

        self.update.store(true, atomic::Ordering::SeqCst);
        self.basalt.interface_ref().odb.unpark();
        Ok(())
    }

    pub fn separate_raw_image(
        &self,
        width: u32,
        height: u32,
        data: Vec<u8>,
    ) -> Result<(), String> {
        let img = ImmutableImage::from_iter(
            data.into_iter(),
            vulkano::image::Dimensions::Dim2d {
                width,
                height,
            },
            vulkano::format::Format::R8G8B8A8Unorm,
            self.basalt.graphics_queue(),
        )
        .unwrap()
        .0;

        let mut coords = atlas::Coords::none();
        coords.w = 1;
        coords.h = 1;

        *self.back_image.lock() = Some(ImageInfo {
            image: Some(img),
            coords,
        });

        self.update.store(true, atomic::Ordering::SeqCst);
        self.basalt.interface_ref().odb.unpark();
        Ok(())
    }

    // pub fn set_raw_back_data(&self, width: u32, height: u32, data: Vec<u8>) -> Result<(),
    // String> { self.basalt.atlas_ref().remove_raw(self.id);
    //
    // let coords = match self.basalt.atlas_ref().load_raw(self.id, data, width, height) {
    // Ok(ok) => ok,
    // Err(e) => return Err(e)
    // };
    //
    // self.back_image.lock() = Some(ImageInfo {
    // image: None,
    // coords: coords
    // });
    //
    // self.update.store(true, atomic::Ordering::SeqCst);
    // self.basalt.interface_ref().odb.unpark();
    // Ok(())
    // }

    pub fn remove_raw_back_img(&self) {
        *self.back_image.lock() = None;
        self.update.store(true, atomic::Ordering::SeqCst);
        self.basalt.interface_ref().odb.unpark();
    }
}
