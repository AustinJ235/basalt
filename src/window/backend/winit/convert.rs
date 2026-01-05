use ordered_float::OrderedFloat;

use crate::input::Qwerty;
use crate::window::monitor::{MonitorHandle, MonitorModeHandle};
use crate::window::{
    CursorIcon, EnableFullScreenError, FullScreenBehavior, Monitor, MonitorMode, WindowError,
};

mod wnt {
    pub use winit::event::KeyEvent;
    pub use winit::keyboard::{Key, KeyCode, NamedKey, NativeKeyCode, PhysicalKey};
    pub use winit::monitor::MonitorHandle;
    pub use winit::window::{CursorIcon, Fullscreen};
}

pub fn fsb_to_wnt(
    mut fsb: FullScreenBehavior,
    fallback_borderless: bool,
    exclusive_supported: bool,
    current_monitor: Option<Monitor>,
    primary_monitor: Option<Monitor>,
    monitors: Vec<Monitor>,
) -> Result<wnt::Fullscreen, WindowError> {
    if fsb.is_exclusive() && !exclusive_supported {
        if !fallback_borderless {
            return Err(EnableFullScreenError::ExclusiveNotSupported.into());
        }

        fsb = match fsb {
            FullScreenBehavior::AutoExclusive => FullScreenBehavior::AutoBorderless,
            FullScreenBehavior::AutoExclusivePrimary => FullScreenBehavior::AutoBorderlessPrimary,
            FullScreenBehavior::AutoExclusiveCurrent => FullScreenBehavior::AutoBorderlessCurrent,
            FullScreenBehavior::ExclusiveAutoMode(monitor)
            | FullScreenBehavior::Exclusive(monitor, _) => FullScreenBehavior::Borderless(monitor),
            _ => unreachable!(),
        };
    }

    if fsb == FullScreenBehavior::Auto {
        fsb = match exclusive_supported {
            true => FullScreenBehavior::AutoExclusive,
            false => FullScreenBehavior::AutoBorderless,
        };
    }

    if fsb.is_exclusive() {
        let (monitor, mode) = match fsb.clone() {
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

        let wnt_monitor_mode_handle = match mode.handle {
            MonitorModeHandle::Winit(handle) => handle,
            _ => unreachable!(),
        };

        Ok(wnt::Fullscreen::Exclusive(wnt_monitor_mode_handle))
    } else {
        let monitor_op = match fsb.clone() {
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

pub fn monitor_from_wnt_handle(wnt_monitor_handle: wnt::MonitorHandle) -> Option<Monitor> {
    // Should always be some, "Returns None if the monitor doesn’t exist anymore."
    let name = wnt_monitor_handle.name()?;
    let physical_size = wnt_monitor_handle.size();
    let resolution = [physical_size.width, physical_size.height];
    let physical_position = wnt_monitor_handle.position();
    let position = [physical_position.x, physical_position.y];

    let refresh_rate_op = wnt_monitor_handle
        .refresh_rate_millihertz()
        .map(|mhz| OrderedFloat::from(mhz as f32 / 1000.0));

    let modes: Vec<MonitorMode> = wnt_monitor_handle
        .video_modes()
        .map(|wnt_monitor_mode| {
            let physical_size = wnt_monitor_mode.size();
            let resolution = [physical_size.width, physical_size.height];
            let bit_depth = wnt_monitor_mode.bit_depth();

            let refresh_rate =
                OrderedFloat::from(wnt_monitor_mode.refresh_rate_millihertz() as f32 / 1000.0);

            MonitorMode {
                resolution,
                bit_depth,
                refresh_rate,
                handle: MonitorModeHandle::Winit(wnt_monitor_mode),
                monitor_handle: MonitorHandle::Winit(wnt_monitor_handle.clone()),
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
        handle: MonitorHandle::Winit(wnt_monitor_handle),
    })
}

pub fn key_ev_to_qwerty(event: &wnt::KeyEvent) -> Option<Qwerty> {
    let by_logical = match event.logical_key {
        wnt::Key::Named(named_key) => {
            match named_key {
                wnt::NamedKey::AudioVolumeMute => Some(Qwerty::TrackMute),
                wnt::NamedKey::AudioVolumeDown => Some(Qwerty::TrackVolDown),
                wnt::NamedKey::AudioVolumeUp => Some(Qwerty::TrackVolUp),
                wnt::NamedKey::MediaPlayPause => Some(Qwerty::TrackPlayPause),
                wnt::NamedKey::MediaLast => Some(Qwerty::TrackBack),
                wnt::NamedKey::MediaSkipForward => Some(Qwerty::TrackNext),
                _ => None,
            }
        },
        _ => None,
    };

    if let Some(qwerty) = by_logical {
        return Some(qwerty);
    }

    match event.physical_key {
        wnt::PhysicalKey::Code(code) => {
            match code {
                wnt::KeyCode::Escape => Some(Qwerty::Esc),
                wnt::KeyCode::F1 => Some(Qwerty::F1),
                wnt::KeyCode::F2 => Some(Qwerty::F2),
                wnt::KeyCode::F3 => Some(Qwerty::F3),
                wnt::KeyCode::F4 => Some(Qwerty::F4),
                wnt::KeyCode::F5 => Some(Qwerty::F5),
                wnt::KeyCode::F6 => Some(Qwerty::F6),
                wnt::KeyCode::F7 => Some(Qwerty::F7),
                wnt::KeyCode::F8 => Some(Qwerty::F8),
                wnt::KeyCode::F9 => Some(Qwerty::F9),
                wnt::KeyCode::F10 => Some(Qwerty::F10),
                wnt::KeyCode::F11 => Some(Qwerty::F11),
                wnt::KeyCode::F12 => Some(Qwerty::F12),
                wnt::KeyCode::Backquote => Some(Qwerty::Tilda),
                wnt::KeyCode::Digit1 => Some(Qwerty::One),
                wnt::KeyCode::Digit2 => Some(Qwerty::Two),
                wnt::KeyCode::Digit3 => Some(Qwerty::Three),
                wnt::KeyCode::Digit4 => Some(Qwerty::Four),
                wnt::KeyCode::Digit5 => Some(Qwerty::Five),
                wnt::KeyCode::Digit6 => Some(Qwerty::Six),
                wnt::KeyCode::Digit7 => Some(Qwerty::Seven),
                wnt::KeyCode::Digit8 => Some(Qwerty::Eight),
                wnt::KeyCode::Digit9 => Some(Qwerty::Nine),
                wnt::KeyCode::Digit0 => Some(Qwerty::Zero),
                wnt::KeyCode::Minus => Some(Qwerty::Dash),
                wnt::KeyCode::Equal => Some(Qwerty::Equal),
                wnt::KeyCode::Backspace => Some(Qwerty::Backspace),
                wnt::KeyCode::Tab => Some(Qwerty::Tab),
                wnt::KeyCode::KeyQ => Some(Qwerty::Q),
                wnt::KeyCode::KeyW => Some(Qwerty::W),
                wnt::KeyCode::KeyE => Some(Qwerty::E),
                wnt::KeyCode::KeyR => Some(Qwerty::R),
                wnt::KeyCode::KeyT => Some(Qwerty::T),
                wnt::KeyCode::KeyY => Some(Qwerty::Y),
                wnt::KeyCode::KeyU => Some(Qwerty::U),
                wnt::KeyCode::KeyI => Some(Qwerty::I),
                wnt::KeyCode::KeyO => Some(Qwerty::O),
                wnt::KeyCode::KeyP => Some(Qwerty::P),
                wnt::KeyCode::BracketLeft => Some(Qwerty::LSqBracket),
                wnt::KeyCode::BracketRight => Some(Qwerty::RSqBracket),
                wnt::KeyCode::Backslash => Some(Qwerty::Backslash),
                wnt::KeyCode::CapsLock => Some(Qwerty::Caps),
                wnt::KeyCode::KeyA => Some(Qwerty::A),
                wnt::KeyCode::KeyS => Some(Qwerty::S),
                wnt::KeyCode::KeyD => Some(Qwerty::D),
                wnt::KeyCode::KeyF => Some(Qwerty::F),
                wnt::KeyCode::KeyG => Some(Qwerty::G),
                wnt::KeyCode::KeyH => Some(Qwerty::H),
                wnt::KeyCode::KeyJ => Some(Qwerty::J),
                wnt::KeyCode::KeyK => Some(Qwerty::K),
                wnt::KeyCode::KeyL => Some(Qwerty::L),
                wnt::KeyCode::Semicolon => Some(Qwerty::SemiColon),
                wnt::KeyCode::Quote => Some(Qwerty::Parenthesis),
                wnt::KeyCode::Enter => Some(Qwerty::Enter),
                wnt::KeyCode::ShiftLeft => Some(Qwerty::LShift),
                wnt::KeyCode::KeyZ => Some(Qwerty::Z),
                wnt::KeyCode::KeyX => Some(Qwerty::X),
                wnt::KeyCode::KeyC => Some(Qwerty::C),
                wnt::KeyCode::KeyV => Some(Qwerty::V),
                wnt::KeyCode::KeyB => Some(Qwerty::B),
                wnt::KeyCode::KeyN => Some(Qwerty::N),
                wnt::KeyCode::KeyM => Some(Qwerty::M),
                wnt::KeyCode::Comma => Some(Qwerty::Comma),
                wnt::KeyCode::Period => Some(Qwerty::Period),
                wnt::KeyCode::Slash => Some(Qwerty::Slash),
                wnt::KeyCode::ShiftRight => Some(Qwerty::RShift),
                wnt::KeyCode::ControlLeft => Some(Qwerty::LCtrl),
                wnt::KeyCode::SuperLeft => Some(Qwerty::LSuper),
                wnt::KeyCode::AltLeft => Some(Qwerty::LAlt),
                wnt::KeyCode::Space => Some(Qwerty::Space),
                wnt::KeyCode::AltRight => Some(Qwerty::RAlt),
                wnt::KeyCode::SuperRight => Some(Qwerty::RSuper),
                wnt::KeyCode::ControlRight => Some(Qwerty::RCtrl),
                wnt::KeyCode::PrintScreen => Some(Qwerty::PrintScreen),
                wnt::KeyCode::ScrollLock => Some(Qwerty::ScrollLock),
                wnt::KeyCode::Pause => Some(Qwerty::Pause),
                wnt::KeyCode::Insert => Some(Qwerty::Insert),
                wnt::KeyCode::Home => Some(Qwerty::Home),
                wnt::KeyCode::PageUp => Some(Qwerty::PageUp),
                wnt::KeyCode::Delete => Some(Qwerty::Delete),
                wnt::KeyCode::End => Some(Qwerty::End),
                wnt::KeyCode::PageDown => Some(Qwerty::PageDown),
                wnt::KeyCode::ArrowUp => Some(Qwerty::ArrowUp),
                wnt::KeyCode::ArrowDown => Some(Qwerty::ArrowDown),
                wnt::KeyCode::ArrowLeft => Some(Qwerty::ArrowLeft),
                wnt::KeyCode::ArrowRight => Some(Qwerty::ArrowRight),
                _ => None,
            }
        },
        wnt::PhysicalKey::Unidentified(wnt::NativeKeyCode::Windows(0xE11D)) => Some(Qwerty::Pause),
        _ => None,
    }
}

pub fn cursor_icon_to_wnt(cursor_icon: CursorIcon) -> Result<wnt::CursorIcon, WindowError> {
    Ok(match cursor_icon {
        CursorIcon::Default => wnt::CursorIcon::Default,
        CursorIcon::ContextMenu => wnt::CursorIcon::ContextMenu,
        CursorIcon::Help => wnt::CursorIcon::Help,
        CursorIcon::Pointer => wnt::CursorIcon::Pointer,
        CursorIcon::Progress => wnt::CursorIcon::Progress,
        CursorIcon::Wait => wnt::CursorIcon::Wait,
        CursorIcon::Cell => wnt::CursorIcon::Cell,
        CursorIcon::Crosshair => wnt::CursorIcon::Crosshair,
        CursorIcon::Text => wnt::CursorIcon::Text,
        CursorIcon::VerticalText => wnt::CursorIcon::VerticalText,
        CursorIcon::Alias => wnt::CursorIcon::Alias,
        CursorIcon::Copy => wnt::CursorIcon::Copy,
        CursorIcon::Move => wnt::CursorIcon::Move,
        CursorIcon::NoDrop => wnt::CursorIcon::NoDrop,
        CursorIcon::NotAllowed => wnt::CursorIcon::NotAllowed,
        CursorIcon::Grab => wnt::CursorIcon::Grab,
        CursorIcon::Grabbing => wnt::CursorIcon::Grabbing,
        CursorIcon::EResize => wnt::CursorIcon::EResize,
        CursorIcon::NResize => wnt::CursorIcon::NResize,
        CursorIcon::NeResize => wnt::CursorIcon::NeResize,
        CursorIcon::NwResize => wnt::CursorIcon::NwResize,
        CursorIcon::SResize => wnt::CursorIcon::SResize,
        CursorIcon::SeResize => wnt::CursorIcon::SeResize,
        CursorIcon::SwResize => wnt::CursorIcon::SwResize,
        CursorIcon::WResize => wnt::CursorIcon::WResize,
        CursorIcon::EwResize => wnt::CursorIcon::EwResize,
        CursorIcon::NsResize => wnt::CursorIcon::NsResize,
        CursorIcon::NeswResize => wnt::CursorIcon::NeswResize,
        CursorIcon::NwseResize => wnt::CursorIcon::NwseResize,
        CursorIcon::ColResize => wnt::CursorIcon::ColResize,
        CursorIcon::RowResize => wnt::CursorIcon::RowResize,
        CursorIcon::AllScroll => wnt::CursorIcon::AllScroll,
        CursorIcon::ZoomIn => wnt::CursorIcon::ZoomIn,
        CursorIcon::ZoomOut => wnt::CursorIcon::ZoomOut,
        CursorIcon::DndAsk | CursorIcon::AllResize => {
            return Err(WindowError::NotSupported);
        },
    })
}
