use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign};
use std::sync::Arc;

use crate::Basalt;
use crate::window::backend::wayland::WlLayerAttributes;
use crate::window::monitor::MonitorHandle;
use crate::window::{Monitor, Window, WindowAttributes, WindowError};

mod wl {
    pub use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer};
}

/// Mask used to specific which display edges to anchor to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WlLayerAnchor(u32);

impl WlLayerAnchor {
    pub const BOTTOM: Self = Self(2);
    pub const LEFT: Self = Self(4);
    pub const RIGHT: Self = Self(8);
    pub const TOP: Self = Self(1);
    pub const NONE: Self = Self(0);
    pub const ALL_EDGES: Self = Self(15);

    pub(crate) fn as_wl(&self) -> wl::Anchor {
        wl::Anchor::from_bits(self.0).unwrap()
    }
}

impl BitAnd for WlLayerAnchor {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl BitAndAssign for WlLayerAnchor {
    fn bitand_assign(&mut self, rhs: Self) {
        *self = Self(self.0 & rhs.0);
    }
}

impl BitOr for WlLayerAnchor {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for WlLayerAnchor {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = Self(self.0 | rhs.0);
    }
}

impl BitXor for WlLayerAnchor {
    type Output = Self;

    fn bitxor(self, rhs: Self) -> Self::Output {
        Self(self.0 ^ rhs.0)
    }
}

impl BitXorAssign for WlLayerAnchor {
    fn bitxor_assign(&mut self, rhs: Self) {
        *self = Self(self.0 ^ rhs.0);
    }
}

/// The depth of the layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WlLayerDepth {
    Background,
    Bottom,
    Top,
    Overlay,
}

impl WlLayerDepth {
    pub(crate) fn as_wl(&self) -> wl::Layer {
        match self {
            Self::Background => wl::Layer::Background,
            Self::Bottom => wl::Layer::Bottom,
            Self::Top => wl::Layer::Top,
            Self::Overlay => wl::Layer::Overlay,
        }
    }
}

/// How keyboard focus is handled for the layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WlLayerKeyboardFocus {
    /// The layer will not receive any keyboard input.
    None,
    /// The layer will receive keyboard input exclusively.
    Exclusive,
    /// The layer will receive keyboard input like normal.
    OnDemand,
}

impl WlLayerKeyboardFocus {
    pub(crate) fn as_wl(&self) -> wl::KeyboardInteractivity {
        match self {
            Self::None => wl::KeyboardInteractivity::None,
            Self::Exclusive => wl::KeyboardInteractivity::Exclusive,
            Self::OnDemand => wl::KeyboardInteractivity::OnDemand,
        }
    }
}

/// Builder for creating a wayland layer.
///
/// This uses the `wlr_layer_shell` extension and not all compositors support it.
///
/// See compositor support see: [wlr-layer-shell-unstable-v1#compositor-support](https://wayland.app/protocols/wlr-layer-shell-unstable-v1#compositor-support).
pub struct WlLayerBuilder {
    basalt: Arc<Basalt>,
    namespace_op: Option<String>,
    size_op: Option<[u32; 2]>,
    anchor: WlLayerAnchor,
    exclusive_zone: i32,
    margin_t: i32,
    margin_b: i32,
    margin_l: i32,
    margin_r: i32,
    depth: WlLayerDepth,
    keyboard_focus: WlLayerKeyboardFocus,
    monitor_op: Option<Monitor>,
}

impl WlLayerBuilder {
    pub(crate) fn new(basalt: Arc<Basalt>) -> Self {
        Self {
            basalt,
            namespace_op: None,
            size_op: None,
            anchor: WlLayerAnchor(0),
            exclusive_zone: 0,
            margin_t: 0,
            margin_b: 0,
            margin_l: 0,
            margin_r: 0,
            depth: WlLayerDepth::Top,
            keyboard_focus: WlLayerKeyboardFocus::OnDemand,
            monitor_op: None,
        }
    }


    pub fn namespace<N>(mut self, namespace: N) -> Self
    where
        N: Into<String>,
    {
        self.namespace_op = Some(namespace.into());
        self
    }

    /// Sets the size of the layer.
    ///
    /// Defaults to `[0, 0]`
    ///
    /// If *zero* is used for either value, the compositor will decide, but their respective edges
    /// must be anchored to. For example if a width of *zero* is used, the layer must be anchored to
    /// both `WlLayerAnchor::LEFT` and `WlLayerAnchor::RIGHT`.
    pub fn size(mut self, size: [u32; 2]) -> Self {
        self.size_op = Some(size);
        self
    }

    /// Anchor the layer to edges of the display.
    ///
    /// Defaults to `WlLayerAnchor::NONE`
    ///
    /// If two parallel edges are set, the layer will be centered upon that axis. For example, if
    /// `WlLayerAnchor::LEFT` and `WlLayerAnchor::RIGHT` the layer will be centered horizontally.
    /// This is the same as not setting either! However, this is can be used for letting the
    /// compositor decide the size. See [`size`](Self::size) for further explanation.
    pub fn anchor(mut self, anchor: WlLayerAnchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// Set the exclusive zone value of the layer.
    ///
    /// Defaults to `0`
    ///
    /// The default value of *zero* will avoid other layers, but not regular windows.
    ///
    /// A positive value is used as the exclusive zone (avoids other layers and regular windows).
    /// It is only meaningful if the layer is anchored to one edge or an edge and both perpendicular
    /// edges. If the layer is not anchored, anchored to only two perpendicular edges (a corner),
    /// anchored to only two parallel edges or anchored to all edges, it will be treated as *zero*.
    ///
    /// A negative value is used to inform the compositor to not interfer other layers or windows.
    ///
    /// **Example**: Horizontal Bar:
    /// ```no_run
    /// let layer = window_manager
    ///    .create_layer()
    ///    .size([0, 30])
    ///    .anchor(WlLayerAnchor::LEFT | WlLayerAnchor::RIGHT| WlLayerAnchor::BOTTOM)
    ///    .exclusive_zone(30)
    ///    .build()
    ///    .unwrap();
    /// ```
    ///
    /// In the above example a layer is created to be used a horizonal bar. It'll span from the
    /// left edge to the right edge of the display and be anchored to the bottom. The height is
    /// specified as `30`, so an exclusive zone value of `30` is used to hint to the compositor
    /// that the entire area should be exclusive.
    ///
    /// **Example**: Wallpaper:
    /// ```no_run
    /// let layer = window_manager
    ///    .create_layer()
    ///    .depth(WlLayerDepth::Background)
    ///    .anchor(WlLayerAnchor::ALL_EDGES)
    ///    .exclusive_zone(-1)
    ///    .build()
    ///    .unwrap();
    /// ```
    ///
    /// In the above example a layer is created to be used as a wallpaper. A depth of
    /// `WlLayerDepth::Background` is used to display below all other layers. Size is not set along
    /// with anchoring it to all edges. This will result in the layer covering the entire display.
    /// An exclusive zone of `-1` is then used to not interfer with any other layers or windows.
    pub fn exclusive_zone(mut self, exclusive_zone: i32) -> Self {
        self.exclusive_zone = exclusive_zone;
        self
    }

    pub fn margin_top(mut self, margin_t: i32) -> Self {
        self.margin_t = margin_t;
        self
    }

    pub fn margin_bottom(mut self, margin_b: i32) -> Self {
        self.margin_b = margin_b;
        self
    }

    pub fn margin_left(mut self, margin_l: i32) -> Self {
        self.margin_l = margin_l;
        self
    }

    pub fn margin_right(mut self, margin_r: i32) -> Self {
        self.margin_r = margin_r;
        self
    }

    pub fn depth(mut self, depth: WlLayerDepth) -> Self {
        self.depth = depth;
        self
    }

    pub fn keyboard_focus(mut self, keyboard_focus: WlLayerKeyboardFocus) -> Self {
        self.keyboard_focus = keyboard_focus;
        self
    }

    pub fn monitor(mut self, monitor: Monitor) -> Self {
        self.monitor_op = Some(monitor);
        self
    }

    pub fn build(self) -> Result<Arc<Window>, WindowError> {
        let Self {
            basalt,
            namespace_op,
            size_op,
            anchor,
            exclusive_zone,
            margin_t,
            margin_b,
            margin_l,
            margin_r,
            depth,
            keyboard_focus,
            monitor_op,
        } = self;

        basalt
            .window_manager_ref()
            .create_window(WindowAttributes::WlLayer(WlLayerAttributes {
                namespace_op,
                size_op,
                anchor,
                exclusive_zone,
                margin_t,
                margin_b,
                margin_l,
                margin_r,
                depth,
                keyboard_focus,
                output_op: match monitor_op {
                    Some(monitor) => {
                        match monitor.handle {
                            MonitorHandle::Wayland(output) => Some(output),
                            _ => unreachable!(),
                        }
                    },
                    None => None,
                },
            }))
    }
}
