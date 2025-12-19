use std::cmp::Reverse;

use ordered_float::OrderedFloat;

#[cfg(feature = "winit_window")]
mod wnt_feature {
    pub mod wnt {
        pub use winit::monitor::{MonitorHandle, VideoModeHandle};
        pub use winit::window::Fullscreen;
    }

    pub use crate::window::{EnableFullScreenError, WindowError};
}

#[cfg(feature = "wayland_window")]
mod wl_feature {
    pub mod wl {
        pub use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
    }
}

#[cfg(feature = "wayland_window")]
use wl_feature::*;
#[cfg(feature = "winit_window")]
use wnt_feature::*;

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum MonitorHandle {
    #[cfg(feature = "winit_window")]
    Winit(wnt::MonitorHandle),
    #[cfg(feature = "wayland_window")]
    Wayland(wl::WlOutput),
    #[allow(dead_code)]
    NonExhaustive,
}

#[cfg(feature = "winit_window")]
impl TryInto<wnt::MonitorHandle> for MonitorHandle {
    type Error = ();

    fn try_into(self) -> Result<wnt::MonitorHandle, Self::Error> {
        match self {
            Self::Winit(handle) => Ok(handle),
            _ => Err(()),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum MonitorModeHandle {
    #[cfg(feature = "winit_window")]
    Winit(wnt::VideoModeHandle),
    #[cfg(feature = "wayland_window")]
    Wayland,
    #[allow(dead_code)]
    NonExhaustive,
}

#[cfg(feature = "winit_window")]
impl TryInto<wnt::VideoModeHandle> for MonitorModeHandle {
    type Error = ();

    fn try_into(self) -> Result<wnt::VideoModeHandle, Self::Error> {
        match self {
            Self::Winit(handle) => Ok(handle),
            _ => Err(()),
        }
    }
}

/// Object that represents a mode of a monitor.
///
/// ***Note:** this represents the current modes available at the time of querying and is not updated.*
#[derive(Clone, PartialEq, Eq)]
pub struct MonitorMode {
    pub(crate) resolution: [u32; 2],
    pub(crate) bit_depth: u16,
    pub(crate) refresh_rate: OrderedFloat<f32>,
    pub(crate) handle: MonitorModeHandle,
    pub(crate) monitor_handle: MonitorHandle,
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

/// Object that represents a monitor.
///
/// ***Note:** this represents the monitor at the time of querying and is not updated.*
#[derive(Clone, PartialEq, Eq)]
pub struct Monitor {
    pub(crate) name: String,
    pub(crate) resolution: [u32; 2],
    pub(crate) position: [i32; 2],
    pub(crate) refresh_rate: OrderedFloat<f32>,
    pub(crate) bit_depth: u16,
    pub(crate) is_current: bool,
    pub(crate) is_primary: bool,
    pub(crate) modes: Vec<MonitorMode>,
    pub(crate) handle: MonitorHandle,
}

impl std::fmt::Debug for Monitor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Monitor")
            .field("name", &self.name)
            .field("resolution", &self.resolution)
            .field("position", &self.position)
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

    #[cfg(feature = "winit_window")]
    pub(crate) fn from_winit(winit_monitor: wnt::MonitorHandle) -> Option<Self> {
        // Should always be some, "Returns None if the monitor doesn’t exist anymore."
        let name = winit_monitor.name()?;
        let physical_size = winit_monitor.size();
        let resolution = [physical_size.width, physical_size.height];
        let physical_position = winit_monitor.position();
        let position = [physical_position.x, physical_position.y];

        let refresh_rate_op = winit_monitor
            .refresh_rate_millihertz()
            .map(|mhz| OrderedFloat::from(mhz as f32 / 1000.0));

        let modes: Vec<MonitorMode> = winit_monitor
            .video_modes()
            .map(|winit_mode| {
                let physical_size = winit_mode.size();
                let resolution = [physical_size.width, physical_size.height];
                let bit_depth = winit_mode.bit_depth();

                let refresh_rate =
                    OrderedFloat::from(winit_mode.refresh_rate_millihertz() as f32 / 1000.0);

                MonitorMode {
                    resolution,
                    bit_depth,
                    refresh_rate,
                    handle: MonitorModeHandle::Winit(winit_mode),
                    monitor_handle: MonitorHandle::Winit(winit_monitor.clone()),
                }
            })
            .collect();

        if modes.is_empty() {
            return None;
        }

        let refresh_rate = refresh_rate_op.unwrap_or_else(|| {
            modes
                .iter()
                .max_by_key(|mode| mode.refresh_rate)
                .unwrap()
                .refresh_rate
        });

        let bit_depth = modes
            .iter()
            .max_by_key(|mode| mode.bit_depth)
            .unwrap()
            .bit_depth;

        Some(Monitor {
            name,
            resolution,
            position,
            refresh_rate,
            bit_depth,
            is_current: false,
            is_primary: false,
            modes,
            handle: MonitorHandle::Winit(winit_monitor),
        })
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

    #[cfg(feature = "winit_window")]
    pub(crate) fn determine_winit_fullscreen(
        &self,
        fallback_borderless: bool,
        exclusive_supported: bool,
        current_monitor: Option<Monitor>,
        primary_monitor: Option<Monitor>,
        monitors: Vec<Monitor>,
    ) -> Result<wnt::Fullscreen, WindowError> {
        if self.is_exclusive() && !exclusive_supported {
            if !fallback_borderless {
                return Err(EnableFullScreenError::ExclusiveNotSupported.into());
            }

            return match self {
                Self::AutoExclusive => Self::AutoBorderless,
                Self::AutoExclusivePrimary => Self::AutoBorderlessPrimary,
                Self::AutoExclusiveCurrent => Self::AutoBorderlessCurrent,
                Self::ExclusiveAutoMode(monitor) | Self::Exclusive(monitor, _) => {
                    Self::Borderless(monitor.clone())
                },
                _ => unreachable!(),
            }
            .determine_winit_fullscreen(
                true,
                false,
                current_monitor,
                primary_monitor,
                monitors,
            );
        }

        if *self == Self::Auto {
            return match exclusive_supported {
                true => Self::AutoExclusive,
                false => Self::AutoBorderless,
            }
            .determine_winit_fullscreen(
                fallback_borderless,
                exclusive_supported,
                current_monitor,
                primary_monitor,
                monitors,
            );
        }

        if self.is_exclusive() {
            let (monitor, mode) = match self.clone() {
                FullScreenBehavior::AutoExclusive => {
                    let monitor = match current_monitor {
                        Some(some) => some,
                        None => {
                            match primary_monitor {
                                Some(some) => some,
                                None => {
                                    match monitors.first() {
                                        Some(some) => some.clone(),
                                        None => {
                                            return Err(
                                                EnableFullScreenError::NoAvailableMonitors.into()
                                            );
                                        },
                                    }
                                },
                            }
                        },
                    };

                    let mode = monitor.optimal_mode();
                    (monitor, mode)
                },
                FullScreenBehavior::AutoExclusivePrimary => {
                    let monitor = match primary_monitor {
                        Some(some) => some,
                        None => return Err(EnableFullScreenError::UnableToDeterminePrimary.into()),
                    };

                    let mode = monitor.optimal_mode();
                    (monitor, mode)
                },
                FullScreenBehavior::AutoExclusiveCurrent => {
                    let monitor = match current_monitor {
                        Some(some) => some,
                        None => return Err(EnableFullScreenError::UnableToDetermineCurrent.into()),
                    };

                    let mode = monitor.optimal_mode();
                    (monitor, mode)
                },
                FullScreenBehavior::ExclusiveAutoMode(monitor) => {
                    let mode = monitor.optimal_mode();
                    (monitor, mode)
                },
                FullScreenBehavior::Exclusive(monitor, mode) => (monitor, mode),
                _ => unreachable!(),
            };

            if mode.monitor_handle != monitor.handle {
                return Err(EnableFullScreenError::IncompatibleMonitorMode.into());
            }

            Ok(wnt::Fullscreen::Exclusive(
                mode.handle.try_into().expect("unreachable"),
            ))
        } else {
            let monitor_op = match self.clone() {
                FullScreenBehavior::AutoBorderless => {
                    match current_monitor {
                        Some(some) => Some(some),
                        None => primary_monitor,
                    }
                },
                FullScreenBehavior::AutoBorderlessPrimary => {
                    match primary_monitor {
                        Some(some) => Some(some),
                        None => return Err(EnableFullScreenError::UnableToDeterminePrimary.into()),
                    }
                },
                FullScreenBehavior::AutoBorderlessCurrent => {
                    match current_monitor {
                        Some(some) => Some(some),
                        None => return Err(EnableFullScreenError::UnableToDetermineCurrent.into()),
                    }
                },
                FullScreenBehavior::Borderless(monitor) => Some(monitor),
                _ => unreachable!(),
            };

            Ok(wnt::Fullscreen::Borderless(monitor_op.map(|monitor| {
                monitor.handle.try_into().expect("unreachable")
            })))
        }
    }
}
