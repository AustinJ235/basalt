use ordered_float::OrderedFloat;

use crate::input::{MouseButton, Qwerty};
use crate::window::monitor::{MonitorHandle, MonitorModeHandle};
use crate::window::{CursorIcon, Monitor, MonitorMode};

mod wl {
    pub use smithay_client_toolkit::output::OutputState;
    pub use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput as Output;
    pub use smithay_client_toolkit::seat::pointer::CursorIcon;
}

pub fn wl_output_to_monitor(
    wl_output_state: &wl::OutputState,
    wl_output: &wl::Output,
    is_current: bool,
) -> Option<Monitor> {
    let info = wl_output_state.info(wl_output)?;

    let mut monitor = Monitor {
        name: info.name.unwrap_or_else(String::new),
        resolution: [0; 2],
        position: [info.location.0, info.location.1],
        refresh_rate: 0.0.into(),
        bit_depth: 32, // Note: Not Supported
        is_current,
        is_primary: false, // Note: Not Supported
        modes: Vec::with_capacity(info.modes.len()),
        handle: MonitorHandle::Wayland(wl_output.clone()),
    };

    for mode in info.modes.iter() {
        if mode.current {
            monitor.resolution = [
                mode.dimensions.0.try_into().unwrap_or(0),
                mode.dimensions.1.try_into().unwrap_or(0),
            ];

            monitor.refresh_rate = OrderedFloat(mode.refresh_rate as f32 / 1000.0);
        }

        monitor.modes.push(MonitorMode {
            resolution: [
                mode.dimensions.0.try_into().unwrap_or(0),
                mode.dimensions.1.try_into().unwrap_or(0),
            ],
            bit_depth: 32, // Note: Not Supported
            refresh_rate: OrderedFloat(mode.refresh_rate as f32 / 1000.0),
            handle: MonitorModeHandle::Wayland,
            monitor_handle: monitor.handle.clone(),
        });
    }

    Some(monitor)
}

pub fn wl_button_to_mouse_button(wl_button: u32) -> Option<MouseButton> {
    match wl_button {
        272 => Some(MouseButton::Left),
        273 => Some(MouseButton::Right),
        274 => Some(MouseButton::Middle),
        _ => None,
    }
}

pub fn raw_code_to_qwerty(raw_code: u32) -> Option<Qwerty> {
    Some(match raw_code {
        1 => Qwerty::Esc,
        59 => Qwerty::F1,
        60 => Qwerty::F2,
        61 => Qwerty::F3,
        62 => Qwerty::F4,
        63 => Qwerty::F5,
        64 => Qwerty::F6,
        65 => Qwerty::F7,
        66 => Qwerty::F8,
        67 => Qwerty::F9,
        68 => Qwerty::F10,
        87 => Qwerty::F11,
        88 => Qwerty::F12,
        41 => Qwerty::Tilda,
        2 => Qwerty::One,
        3 => Qwerty::Two,
        4 => Qwerty::Three,
        5 => Qwerty::Four,
        6 => Qwerty::Five,
        7 => Qwerty::Six,
        8 => Qwerty::Seven,
        9 => Qwerty::Eight,
        10 => Qwerty::Nine,
        11 => Qwerty::Zero,
        12 => Qwerty::Dash,
        13 => Qwerty::Equal,
        14 => Qwerty::Backspace,
        15 => Qwerty::Tab,
        16 => Qwerty::Q,
        17 => Qwerty::W,
        18 => Qwerty::E,
        19 => Qwerty::R,
        20 => Qwerty::T,
        21 => Qwerty::Y,
        22 => Qwerty::U,
        23 => Qwerty::I,
        24 => Qwerty::O,
        25 => Qwerty::P,
        26 => Qwerty::LSqBracket,
        27 => Qwerty::RSqBracket,
        43 => Qwerty::Backslash,
        58 => Qwerty::Caps,
        30 => Qwerty::A,
        31 => Qwerty::S,
        32 => Qwerty::D,
        33 => Qwerty::F,
        34 => Qwerty::G,
        35 => Qwerty::H,
        36 => Qwerty::J,
        37 => Qwerty::K,
        38 => Qwerty::L,
        39 => Qwerty::SemiColon,
        40 => Qwerty::Parenthesis,
        28 => Qwerty::Enter,
        42 => Qwerty::LShift,
        44 => Qwerty::Z,
        45 => Qwerty::X,
        46 => Qwerty::C,
        47 => Qwerty::V,
        48 => Qwerty::B,
        49 => Qwerty::N,
        50 => Qwerty::M,
        51 => Qwerty::Comma,
        52 => Qwerty::Period,
        53 => Qwerty::Slash,
        // ??? => Qwerty::RShift,
        29 => Qwerty::LCtrl,
        // ??? => Qwerty::LSuper,
        56 => Qwerty::LAlt,
        57 => Qwerty::Space,
        100 => Qwerty::RAlt,
        // ??? => Qwerty::RSuper,
        // ??? => Qwerty::RCtrl,
        99 => Qwerty::PrintScreen,
        70 => Qwerty::ScrollLock,
        // ??? => Qwerty::Pause,
        110 => Qwerty::Insert,
        102 => Qwerty::Home,
        // ??? => Qwerty::PageUp,
        111 => Qwerty::Delete,
        107 => Qwerty::End,
        // ??? => Qwerty::PageDown,
        103 => Qwerty::ArrowUp,
        108 => Qwerty::ArrowDown,
        105 => Qwerty::ArrowLeft,
        106 => Qwerty::ArrowRight,
        113 => Qwerty::TrackMute,
        114 => Qwerty::TrackVolDown,
        115 => Qwerty::TrackVolUp,
        // ??? => Qwerty::TrackPlayPause,
        // ??? => Qwerty::TrackBack,
        // ??? => Qwerty::TrackNext,
        _ => return None,
    })
}

pub fn cursor_icon_to_wl(cursor_icon: CursorIcon) -> wl::CursorIcon {
    match cursor_icon {
        CursorIcon::Default => wl::CursorIcon::Default,
        CursorIcon::ContextMenu => wl::CursorIcon::ContextMenu,
        CursorIcon::Help => wl::CursorIcon::Help,
        CursorIcon::Pointer => wl::CursorIcon::Pointer,
        CursorIcon::Progress => wl::CursorIcon::Progress,
        CursorIcon::Wait => wl::CursorIcon::Wait,
        CursorIcon::Cell => wl::CursorIcon::Cell,
        CursorIcon::Crosshair => wl::CursorIcon::Crosshair,
        CursorIcon::Text => wl::CursorIcon::Text,
        CursorIcon::VerticalText => wl::CursorIcon::VerticalText,
        CursorIcon::Alias => wl::CursorIcon::Alias,
        CursorIcon::Copy => wl::CursorIcon::Copy,
        CursorIcon::Move => wl::CursorIcon::Move,
        CursorIcon::NoDrop => wl::CursorIcon::NoDrop,
        CursorIcon::NotAllowed => wl::CursorIcon::NotAllowed,
        CursorIcon::Grab => wl::CursorIcon::Grab,
        CursorIcon::Grabbing => wl::CursorIcon::Grabbing,
        CursorIcon::EResize => wl::CursorIcon::EResize,
        CursorIcon::NResize => wl::CursorIcon::NResize,
        CursorIcon::NeResize => wl::CursorIcon::NeResize,
        CursorIcon::NwResize => wl::CursorIcon::NwResize,
        CursorIcon::SResize => wl::CursorIcon::SResize,
        CursorIcon::SeResize => wl::CursorIcon::SeResize,
        CursorIcon::SwResize => wl::CursorIcon::SwResize,
        CursorIcon::WResize => wl::CursorIcon::WResize,
        CursorIcon::EwResize => wl::CursorIcon::EwResize,
        CursorIcon::NsResize => wl::CursorIcon::NsResize,
        CursorIcon::NeswResize => wl::CursorIcon::NeswResize,
        CursorIcon::NwseResize => wl::CursorIcon::NwseResize,
        CursorIcon::ColResize => wl::CursorIcon::ColResize,
        CursorIcon::RowResize => wl::CursorIcon::RowResize,
        CursorIcon::AllScroll => wl::CursorIcon::AllScroll,
        CursorIcon::ZoomIn => wl::CursorIcon::ZoomIn,
        CursorIcon::ZoomOut => wl::CursorIcon::ZoomOut,
        CursorIcon::DndAsk => wl::CursorIcon::DndAsk,
        CursorIcon::AllResize => wl::CursorIcon::AllResize,
    }
}
