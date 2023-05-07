pub mod bin;
pub mod checkbox;
pub mod on_off_button;
pub mod render;
pub mod scroll_bar;
pub mod slider;

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

use parking_lot::{Mutex, RwLock};
use vulkano::buffer::BufferContents;
use vulkano::command_buffer::{AutoCommandBufferBuilder, PrimaryAutoCommandBuffer};
use vulkano::device::{Device, Queue};
use vulkano::format::Format as VkFormat;
use vulkano::pipeline::graphics::vertex_input::Vertex;

use self::bin::{Bin, BinID, FontStretch, FontStyle, FontWeight};
use self::render::composer::{Composer, ComposerEv, ComposerInit};
pub use self::render::ItfDrawTarget;
use self::render::{ItfRenderer, ItfRendererInit};
use crate::image_view::BstImageView;
use crate::window::BstWindowID;
use crate::{Atlas, Basalt, BasaltWindow, BstOptions};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefaultFont {
    pub family: Option<String>,
    pub weight: Option<FontWeight>,
    pub strench: Option<FontStretch>,
    pub style: Option<FontStyle>,
}

#[derive(BufferContents, Vertex, Clone, Debug)]
#[repr(C)]
pub(crate) struct ItfVertInfo {
    #[format(R32G32B32_SFLOAT)]
    pub position: [f32; 3],
    #[format(R32G32_SFLOAT)]
    pub coords: [f32; 2],
    #[format(R32G32B32A32_SFLOAT)]
    pub color: [f32; 4],
    #[format(R32_SINT)]
    pub ty: i32,
    #[format(R32_UINT)]
    pub tex_i: u32,
}

impl Default for ItfVertInfo {
    fn default() -> Self {
        ItfVertInfo {
            position: [0.0; 3],
            coords: [0.0; 2],
            color: [0.0; 4],
            ty: 0,
            tex_i: 0,
        }
    }
}

pub(crate) fn scale_verts(win_size: &[f32; 2], scale: f32, verts: &mut Vec<ItfVertInfo>) {
    for vert in verts {
        vert.position[0] *= scale;
        vert.position[1] *= scale;
        vert.position[0] += win_size[0] / -2.0;
        vert.position[0] /= win_size[0] / 2.0;
        vert.position[1] += win_size[1] / -2.0;
        vert.position[1] /= win_size[1] / 2.0;
    }
}

#[derive(Clone, Copy)]
struct Scale {
    pub win: f32,
    pub itf: f32,
}

impl Scale {
    fn effective(&self, ignore_win: bool) -> f32 {
        if ignore_win {
            self.itf
        } else {
            self.itf * self.win
        }
    }
}

pub struct Interface {
    options: BstOptions,
    renderer: Mutex<ItfRenderer>,
    composer: Arc<Composer>,
    scale: Mutex<Scale>,
    bins_state: RwLock<BinsState>,
    default_font: Mutex<DefaultFont>,
}

#[derive(Default)]
struct BinsState {
    bst: Option<Arc<Basalt>>,
    id: u64,
    map: BTreeMap<BinID, Weak<Bin>>,
}

pub(crate) struct InterfaceInit {
    pub options: BstOptions,
    pub device: Arc<Device>,
    pub transfer_queue: Arc<Queue>,
    pub compute_queue: Arc<Queue>,
    pub itf_format: VkFormat,
    pub atlas: Arc<Atlas>,
    pub window: Arc<dyn BasaltWindow>,
}

impl Interface {
    pub(crate) fn new(init: InterfaceInit) -> Arc<Self> {
        let InterfaceInit {
            options,
            device,
            transfer_queue,
            compute_queue: _compute_queue,
            itf_format,
            atlas,
            window,
        } = init;

        let scale = Scale {
            win: window.scale_factor(),
            itf: options.scale,
        };

        let composer = Composer::new(ComposerInit {
            options: options.clone(),
            device: device.clone(),
            transfer_queue,
            atlas: atlas.clone(),
            initial_scale: scale.effective(options.ignore_dpi),
        });

        Arc::new(Interface {
            bins_state: RwLock::new(BinsState::default()),
            scale: Mutex::new(scale),
            renderer: Mutex::new(ItfRenderer::new(ItfRendererInit {
                options: options.clone(),
                device,
                itf_format,
                atlas,
                composer: composer.clone(),
            })),
            composer,
            options,
            default_font: Mutex::new(DefaultFont::default()),
        })
    }

    pub(crate) fn attach_basalt(&self, basalt: Arc<Basalt>) {
        let mut bins_state = self.bins_state.write();
        bins_state.bst = Some(basalt);
    }

    /// The current scale without taking into account dpi based window scaling.
    pub fn current_scale(&self) -> f32 {
        self.scale.lock().itf
    }

    /// The current scale taking into account dpi based window scaling.
    pub fn current_effective_scale(&self) -> f32 {
        let ignore_dpi = self.options.ignore_dpi;
        self.scale.lock().effective(ignore_dpi)
    }

    /// Set the current scale. Doesn't account for dpi based window scaling.
    pub fn set_scale(&self, set_scale: f32) {
        let ignore_dpi = self.options.ignore_dpi;
        let mut scale = self.scale.lock();
        scale.itf = set_scale;
        self.composer
            .send_event(ComposerEv::Scale(scale.effective(ignore_dpi)));
    }

    pub(crate) fn set_window_scale(&self, set_scale: f32) {
        let ignore_dpi = self.options.ignore_dpi;
        let mut scale = self.scale.lock();
        scale.win = set_scale;
        self.composer
            .send_event(ComposerEv::Scale(scale.effective(ignore_dpi)));
    }

    /// Set the current scale taking into account dpi based window scaling.
    pub fn set_effective_scale(&self, set_scale: f32) {
        let ignore_dpi = self.options.ignore_dpi;
        let mut scale = self.scale.lock();

        if ignore_dpi {
            scale.itf = set_scale;
        } else {
            scale.itf = set_scale / scale.win;
        };

        self.composer
            .send_event(ComposerEv::Scale(scale.effective(ignore_dpi)));
    }

    /// Get the current MSAA level.
    pub fn current_msaa(&self) -> BstMSAALevel {
        let mut renderer = self.renderer.lock();
        *renderer.msaa_mut_ref()
    }

    /// Set the MSAA Level.
    pub fn set_msaa(&self, set_msaa: BstMSAALevel) {
        let mut renderer = self.renderer.lock();
        *renderer.msaa_mut_ref() = set_msaa;
    }

    /// Increase MSAA to the next step.
    pub fn increase_msaa(&self) -> BstMSAALevel {
        let mut renderer = self.renderer.lock();
        renderer.msaa_mut_ref().increase();
        *renderer.msaa_mut_ref()
    }

    /// Decrease MSAA to the next step.
    pub fn decrease_msaa(&self) -> BstMSAALevel {
        let mut renderer = self.renderer.lock();
        renderer.msaa_mut_ref().decrease();
        *renderer.msaa_mut_ref()
    }

    /// Retrieve the current default font.
    pub fn default_font(&self) -> DefaultFont {
        self.default_font.lock().clone()
    }

    /// Set the default font.
    ///
    /// **Note**: An invalid font will not cause a panic, but text may not render.
    pub fn set_default_font(&self, font: DefaultFont) {
        *self.default_font.lock() = font.clone();
        self.composer.send_event(ComposerEv::DefaultFont(font));
    }

    pub(crate) fn composer_ref(&self) -> &Arc<Composer> {
        &self.composer
    }

    #[inline]
    pub fn get_bin_id_atop(&self, window: BstWindowID, x: f32, y: f32) -> Option<BinID> {
        self.get_bins_atop(window, x, y)
            .into_iter()
            .next()
            .map(|bin| bin.id())
    }

    #[inline]
    pub fn get_bin_atop(&self, window: BstWindowID, x: f32, y: f32) -> Option<Arc<Bin>> {
        self.get_bins_atop(window, x, y).into_iter().next()
    }

    /// Get the `Bin`'s that are at the given mouse position accounting for current effective
    /// scale. Returned `Vec` is sorted where the top-most `Bin`'s are first.
    pub fn get_bins_atop(&self, _window: BstWindowID, mut x: f32, mut y: f32) -> Vec<Arc<Bin>> {
        // TODO: Check window

        let scale = self.current_effective_scale();
        x /= scale;
        y /= scale;

        let mut bins: Vec<_> = self
            .bins_state
            .read()
            .map
            .iter()
            .filter_map(|(_, bin_wk)| {
                match bin_wk.upgrade() {
                    Some(bin) if bin.mouse_inside(x, y) => Some(bin),
                    _ => None,
                }
            })
            .collect();

        bins.sort_by_cached_key(|bin| Reverse(bin.post_update().z_index));
        bins
    }

    /// Get the `BinID`'s that are at the given mouse position accounting for current effective
    /// scale. Returned `Vec` is sorted where the top-most `Bin`'s are first.
    #[inline]
    pub fn get_bin_ids_atop(&self, window: BstWindowID, x: f32, y: f32) -> Vec<BinID> {
        self.get_bins_atop(window, x, y)
            .into_iter()
            .map(|bin| bin.id())
            .collect()
    }

    /// Returns a list of all bins that have a strong reference. Note keeping this
    /// list will keep all bins returned alive and prevent them from being dropped.
    /// This list should be dropped asap to prevent issues with bins being dropped.
    pub fn bins(&self) -> Vec<Arc<Bin>> {
        self.bins_state
            .read()
            .map
            .iter()
            .filter_map(|(_, b)| b.upgrade())
            .collect()
    }

    pub fn new_bins(&self, amt: usize) -> Vec<Arc<Bin>> {
        let mut out = Vec::with_capacity(amt);
        let mut bins_state = self.bins_state.write();

        for _ in 0..amt {
            let id = BinID(bins_state.id);
            bins_state.id += 1;
            let bin = Bin::new(id, bins_state.bst.clone().unwrap());
            bins_state.map.insert(id, Arc::downgrade(&bin));
            self.composer
                .send_event(ComposerEv::AddBin(Arc::downgrade(&bin)));
            out.push(bin);
        }

        out
    }

    pub fn new_bin(&self) -> Arc<Bin> {
        self.new_bins(1).pop().unwrap()
    }

    pub fn get_bin(&self, id: BinID) -> Option<Arc<Bin>> {
        match self.bins_state.read().map.get(&id) {
            Some(some) => some.upgrade(),
            None => None,
        }
    }

    /// Checks if the mouse position is on top of any `Bin`'s in the interface.
    pub fn mouse_inside(&self, _window: BstWindowID, mut mouse_x: f32, mut mouse_y: f32) -> bool {
        let scale = self.current_effective_scale();
        mouse_x /= scale;
        mouse_y /= scale;

        for bin in self.bins() {
            if bin.mouse_inside(mouse_x, mouse_y) {
                return true;
            }
        }

        false
    }

    pub fn draw(
        &self,
        cmd: AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
        target: ItfDrawTarget,
    ) -> (
        AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
        Option<Arc<BstImageView>>,
    ) {
        self.renderer.lock().draw(cmd, target)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BstMSAALevel {
    One,
    Two,
    Four,
    Eight,
}

impl PartialOrd for BstMSAALevel {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BstMSAALevel {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_u32().cmp(&other.as_u32())
    }
}

impl BstMSAALevel {
    pub(crate) fn as_u32(&self) -> u32 {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Four => 4,
            Self::Eight => 8,
        }
    }

    pub(crate) fn as_vulkano(&self) -> vulkano::image::SampleCount {
        match self {
            Self::One => vulkano::image::SampleCount::Sample1,
            Self::Two => vulkano::image::SampleCount::Sample2,
            Self::Four => vulkano::image::SampleCount::Sample4,
            Self::Eight => vulkano::image::SampleCount::Sample8,
        }
    }

    pub fn increase(&mut self) {
        *self = match self {
            Self::One => Self::Two,
            Self::Two => Self::Four,
            Self::Four => Self::Eight,
            Self::Eight => Self::Eight,
        };
    }

    pub fn decrease(&mut self) {
        *self = match self {
            Self::One => Self::One,
            Self::Two => Self::One,
            Self::Four => Self::Two,
            Self::Eight => Self::Four,
        };
    }
}
