//! System for storing interface related objects.

mod bin;
pub mod checkbox;
mod color;
pub mod on_off_button;
pub mod scroll_bar;
pub mod slider;
mod style;

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

use parking_lot::{Mutex, RwLock};
use vulkano::buffer::BufferContents;
use vulkano::pipeline::graphics::vertex_input::Vertex;

pub(crate) use self::bin::UpdateContext;
pub use self::bin::style::{
    BackImageRegion, BinStyle, BinStyleError, BinStyleErrorType, BinStyleValidation, BinStyleWarn,
    BinStyleWarnType, BinVertex, FloatWeight, ImageEffect, LineLimit, LineSpacing, Opacity,
    TextHoriAlign, TextVertAlign, TextWrap, Visibility, ZIndex,
};
pub use self::bin::text_body::{
    ExtendTextSelection, PosTextCursor, TextAttrs, TextBody, TextCursor, TextCursorAffinity,
    TextSelection, TextSpan,
};
pub use self::bin::text_modify::TextBodyGuard;
pub use self::bin::{Bin, BinID, BinPostUpdate, OVDPerfMetrics};
pub use self::color::Color;
pub use self::style::{Flow, FontFamily, FontStretch, FontStyle, FontWeight, Position, UnitValue};
use crate::Basalt;
use crate::window::WindowID;

/// Default font style used.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DefaultFont {
    pub height: UnitValue,
    pub family: FontFamily,
    pub weight: FontWeight,
    pub stretch: FontStretch,
    pub style: FontStyle,
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

/// System for storing interface related objects.
pub struct Interface {
    bins_state: RwLock<BinsState>,
    default_font: Mutex<DefaultFont>,
    binary_fonts: Mutex<Vec<Arc<dyn AsRef<[u8]> + Sync + Send>>>,
}

#[derive(Default)]
struct BinsState {
    bst: Option<Arc<Basalt>>,
    id: u64,
    map: BTreeMap<BinID, Weak<Bin>>,
}

impl Interface {
    pub(crate) fn new(binary_fonts: Vec<Arc<dyn AsRef<[u8]> + Sync + Send>>) -> Arc<Self> {
        Arc::new(Interface {
            bins_state: RwLock::new(BinsState::default()),
            default_font: Mutex::new(DefaultFont {
                height: UnitValue::Pixels(12.0),
                family: FontFamily::Serif,
                weight: FontWeight::Normal,
                stretch: FontStretch::Normal,
                style: FontStyle::Normal,
            }),
            binary_fonts: Mutex::new(binary_fonts),
        })
    }

    pub(crate) fn associate_basalt(&self, basalt: Arc<Basalt>) {
        let mut bins_state = self.bins_state.write();
        bins_state.bst = Some(basalt);
    }

    pub(crate) fn binary_fonts(&self) -> Vec<Arc<dyn AsRef<[u8]> + Sync + Send>> {
        self.binary_fonts.lock().clone()
    }

    /// Retrieve the current default font.
    pub fn default_font(&self) -> DefaultFont {
        self.default_font.lock().clone()
    }

    /// Set the default font.
    ///
    /// ***Note**: An invalid font will not cause a panic, but text may not render.*
    pub fn set_default_font(&self, mut default_font: DefaultFont) {
        if default_font.height == UnitValue::Undefined {
            default_font.height = UnitValue::Pixels(12.0);
        }

        if default_font.family == FontFamily::Inheirt {
            default_font.family = FontFamily::Serif;
        }

        if default_font.weight == FontWeight::Inheirt {
            default_font.weight = FontWeight::Normal;
        }

        if default_font.stretch == FontStretch::Inheirt {
            default_font.stretch = FontStretch::Normal;
        }

        if default_font.style == FontStyle::Inheirt {
            default_font.style = FontStyle::Normal;
        }

        *self.default_font.lock() = default_font.clone();

        self.bins_state
            .read()
            .bst
            .as_ref()
            .unwrap()
            .window_manager_ref()
            .set_default_font(default_font);
    }

    /// Load a font from a binary source.
    ///
    /// **Note**: Invalid fonts will not cause an error, but text may not render.*
    pub fn add_binary_font<T: AsRef<[u8]> + Sync + Send + 'static>(&self, data: T) {
        let binary_font = Arc::new(data);
        self.binary_fonts.lock().push(binary_font.clone());

        self.bins_state
            .read()
            .bst
            .as_ref()
            .unwrap()
            .window_manager_ref()
            .add_binary_font(binary_font);
    }

    /// Get the top-most `Bin` given a window & position.
    #[inline]
    pub fn get_bin_atop(&self, window: WindowID, x: f32, y: f32) -> Option<Arc<Bin>> {
        self.get_bins_atop(window, x, y).into_iter().next()
    }

    /// Get `Bin`'s atop the provided window & position.
    ///
    /// ***Note:** This is sorted where the top-most is first and the bottom-most is last.*
    pub fn get_bins_atop(&self, window_id: WindowID, mut x: f32, mut y: f32) -> Vec<Arc<Bin>> {
        let state = self.bins_state.read();

        let window = match state
            .bst
            .as_ref()
            .unwrap()
            .window_manager_ref()
            .window(window_id)
        {
            Some(some) => some,
            None => return Vec::new(),
        };

        let effective_scale = window.effective_interface_scale();
        x /= effective_scale;
        y /= effective_scale;

        let mut bins = window
            .associated_bins()
            .into_iter()
            .filter(|bin| bin.mouse_inside(x, y))
            .collect::<Vec<_>>();

        bins.sort_by_cached_key(|bin| Reverse(bin.post_update().z_index));
        bins
    }

    /// Get the top-most `BinID` given a window & position.
    #[inline]
    pub fn get_bin_id_atop(&self, window: WindowID, x: f32, y: f32) -> Option<BinID> {
        self.get_bins_atop(window, x, y)
            .into_iter()
            .next()
            .map(|bin| bin.id())
    }

    /// Get `BinID`'s atop the provided window & position.
    ///
    /// ***Note:** This is sorted where the top-most is first and the bottom-most is last.*
    #[inline]
    pub fn get_bin_ids_atop(&self, window: WindowID, x: f32, y: f32) -> Vec<BinID> {
        self.get_bins_atop(window, x, y)
            .into_iter()
            .map(|bin| bin.id())
            .collect()
    }

    /// Returns a list of all bins that have a strong reference.
    ///
    /// ***Note:** Keeping this list will keep all bins returned alive and prevent them from being
    /// dropped. This list should be dropped asap to prevent issues with bins being dropped.*
    pub fn bins(&self) -> Vec<Arc<Bin>> {
        self.bins_state
            .read()
            .map
            .iter()
            .filter_map(|(_, b)| b.upgrade())
            .collect()
    }

    /// Create a new `Bin`
    ///
    /// ***Note:** This `Bin` will not have a window association. Using this method on a `Window`
    /// should be the preferred way of creating s `Bin`.*
    pub fn new_bin(&self) -> Arc<Bin> {
        self.new_bins(1).pop().unwrap()
    }

    /// Create new `Bin`'s
    ///
    /// ***Note:** These `Bin`'s will not have a window association. Using this method on a `Window`
    /// should be the preferred way of creating `Bin`'s.*
    pub fn new_bins(&self, amt: usize) -> Vec<Arc<Bin>> {
        let mut out = Vec::with_capacity(amt);
        let mut bins_state = self.bins_state.write();

        for _ in 0..amt {
            let id = BinID(bins_state.id);
            bins_state.id += 1;
            let bin = Bin::new(id, bins_state.bst.clone().unwrap());
            bins_state.map.insert(id, Arc::downgrade(&bin));
            out.push(bin);
        }

        out
    }

    /// Retreive a `Bin` given its `BinID`.
    pub fn get_bin(&self, id: BinID) -> Option<Arc<Bin>> {
        match self.bins_state.read().map.get(&id) {
            Some(some) => some.upgrade(),
            None => None,
        }
    }

    /// Checks if the mouse position is on top of any `Bin`'s in the interface.
    pub fn mouse_inside(&self, window_id: WindowID, mut x: f32, mut y: f32) -> bool {
        let state = self.bins_state.read();

        let window = match state
            .bst
            .as_ref()
            .unwrap()
            .window_manager_ref()
            .window(window_id)
        {
            Some(some) => some,
            None => return false,
        };

        let effective_scale = window.effective_interface_scale();
        x /= effective_scale;
        y /= effective_scale;

        for bin in window.associated_bins() {
            if bin.mouse_inside(x, y) {
                return true;
            }
        }

        false
    }
}
