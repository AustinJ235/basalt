use std::cmp::Reverse;

use ordered_float::OrderedFloat;
use winit::monitor::{MonitorHandle as WinitMonitorHandle, VideoMode as WinitVideoMode};

#[derive(Clone, PartialEq, Eq)]
pub struct MonitorMode {
    resolution: [u32; 2],
    bit_depth: u16,
    refresh_rate: OrderedFloat<f32>,
    handle: WinitVideoMode,
    monitor_handle: WinitMonitorHandle,
}

impl MonitorMode {
    /// Returns the resolution of this mode.
    pub fn resolution(&self) -> [u32; 2] {
        self.resolution
    }

    /// Returns the bit depth of this mode.
    pub fn bit_depth(&self) -> u16 {
        self.bit_depth
    }

    /// Returns the refresh rate in Hz of this mode.
    pub fn refresh_rate(&self) -> f32 {
        self.refresh_rate.into_inner()
    }
}

impl std::fmt::Debug for MonitorMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MonitorMode")
            .field("resolution", &self.resolution)
            .field("bit_depth", &self.bit_depth)
            .field("refresh_rate", &self.refresh_rate.into_inner())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Monitor {
    name: String,
    resolution: [u32; 2],
    position: [i32; 2],
    refresh_rate: OrderedFloat<f32>,
    bit_depth: u16,
    is_current: bool,
    is_primary: bool,
    modes: Vec<MonitorMode>,
    handle: WinitMonitorHandle,
}

impl std::fmt::Debug for Monitor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Monitor")
            .field("name", &self.name)
            .field("resolution", &self.resolution)
            .field("bit_depth", &self.bit_depth)
            .field("refresh_rate", &self.refresh_rate.into_inner())
            .field("is_current", &self.is_current)
            .field("is_primary", &self.is_primary)
            .field("modes", &self.modes)
            .finish()
    }
}

impl Monitor {
    /// Returns a human-readable name of the monitor.
    pub fn name(&self) -> String {
        self.name.clone()
    }

    /// Returns the monitor’s resolution.
    pub fn resolution(&self) -> [u32; 2] {
        self.resolution
    }

    /// Returns the top-left corner position of the monitor relative to the larger full screen area.
    pub fn position(&self) -> [i32; 2] {
        self.position
    }

    /// Returns the bit depth of this monitor.
    pub fn bit_depth(&self) -> u16 {
        self.bit_depth
    }

    /// The monitor refresh rate used by the system.
    pub fn refresh_rate(&self) -> f32 {
        self.refresh_rate.into_inner()
    }

    /// Returns a list of `MonitorMode`'s supported by this monitor.
    pub fn modes(&self) -> Vec<MonitorMode> {
        self.modes.clone()
    }

    /// Returns `true` if it is the systems primary monitor.
    pub fn is_primary(&self) -> bool {
        self.is_primary
    }

    /// Returns `true` if it is the current monitor in use.
    pub fn is_current(&self) -> bool {
        self.is_current
    }

    /// Get the most optimal mode for this monitor.
    ///
    /// # Priority
    /// 1. Resolution (Higher than the monitor are less favorable)
    /// 2. Aspect Ratio
    /// 2. Refresh Rate
    /// 3. Bit Depth
    pub fn optimal_mode(&self) -> MonitorMode {
        assert!(!self.modes.is_empty());

        let mut modes: Vec<&MonitorMode> = {
            let same_resolution: Vec<_> = self
                .modes
                .iter()
                .filter(|mode| mode.resolution == self.resolution)
                .collect();

            if !same_resolution.is_empty() {
                same_resolution
            } else {
                // Same resolution isn't available, try lesser ones first.
                let mut modes_a: Vec<_> = self
                    .modes
                    .iter()
                    .filter(|mode| {
                        mode.resolution[0] <= self.resolution[0]
                            && mode.resolution[1] <= self.resolution[1]
                    })
                    .collect();

                // No lesser resolution modes, use them all.
                if modes_a.is_empty() {
                    modes_a = self.modes.iter().collect();
                }

                // Try to find one with the same aspect ratio
                let ideal_aspect = self.resolution[0] as f32 / self.resolution[1] as f32;
                let mut modes_b: Vec<_> = modes_a
                    .iter()
                    .filter(|mode| {
                        mode.resolution[0] as f32 / mode.resolution[1] as f32 == ideal_aspect
                    })
                    .collect();

                // No modes with same aspect ratio use modes_a.
                if modes_b.is_empty() {
                    // TODO: sort by closest aspect ratio?
                    modes_b = modes_a.iter().collect();
                }

                modes_b.sort_by_key(|mode| Reverse(mode.resolution[0] * mode.resolution[1]));
                modes_b.into_iter().copied().collect()
            }
        };

        let best_resolution = modes[0].resolution;
        modes.retain(|mode| mode.resolution == best_resolution);
        modes.sort_by_key(|mode| Reverse(mode.refresh_rate));
        let best_refresh_rate = modes[0].refresh_rate;
        modes.retain(|mode| mode.refresh_rate == best_refresh_rate);
        modes.sort_by_key(|mode| Reverse(mode.bit_depth));
        let best_bit_depth = modes[0].bit_depth;
        modes.retain(|mode| mode.bit_depth == best_bit_depth);
        modes[0].clone()
    }
}

/// Determines how the application should go into full screen.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum FullScreenBehavior {
    #[default]
    /// **Default**
    ///
    /// If fullscreen exclusive support is enabled this uses, `AutoExclusive` otherwise `AutoBorderless`.
    Auto,
    /// Enable borderless full screen on a monitor determined by this order:
    /// 1. Current Monitor
    /// 2. Primary Monitor
    /// 3. Implementation decides
    AutoBorderless,
    /// Enable borderless full screen on the primary monitor.
    AutoBorderlessPrimary,
    /// Enable borderless full screen on the current monitor.
    AutoBorderlessCurrent,
    /// Enable borderless full screen on the provided monitor.
    Borderless(Monitor),
    /// Enable exclusive full screen on a monitor determined by this order:
    /// 1. Current Monitor
    /// 2. Primary Monitor
    /// 3. First enumerated
    AutoExclusive,
    /// Enable exclusive full screen on the primary monitor
    ///
    /// See `Monitor::optimal_mode` for how the mode is determined.
    AutoExclusivePrimary,
    /// Enable exclusive full screen on the current monitor
    ///
    /// See `Monitor::optimal_mode` for how the mode is determined.
    AutoExclusiveCurrent,
    /// Enable exclusive full screen on the provided monitor and automatically select the mode.
    ///
    /// See `Monitor::optimal_mode` for how the mode is determined.
    ExclusiveAutoMode(Monitor),
    /// Enable exclusive full screen on the provided monitor and mode.
    Exclusive(Monitor, MonitorMode),
}

impl FullScreenBehavior {
    /// Returns false for `Auto`
    pub fn is_exclusive(&self) -> bool {
        match self {
            Self::Auto => false,
            Self::AutoBorderless => false,
            Self::AutoBorderlessPrimary => false,
            Self::AutoBorderlessCurrent => false,
            Self::Borderless(_) => false,
            Self::AutoExclusive => true,
            Self::AutoExclusivePrimary => true,
            Self::AutoExclusiveCurrent => true,
            Self::ExclusiveAutoMode(_) => true,
            Self::Exclusive(..) => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullScreenError {
    /// The window implmentation is unable to determine the primary monitor.
    UnableToDeterminePrimary,
    /// The window implmentation is unable to determine the current monitor.
    UnableToDetermineCurrent,
    /// Attempted to use exclusive fullscreen, when it wasn't enabled.
    ///
    /// See: `BstOptions::use_exclusive_fullscreen`
    ExclusiveNotSupported,
    /// The monitor no longer exists.
    MonitorDoesNotExist,
    /// No available monitors
    NoAvailableMonitors,
    /// The provided mode doesn't belong to the monitor.
    IncompatibleMonitorMode,
}
