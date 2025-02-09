use crate::input::Qwerty;

mod winit {
    pub use winit::event::KeyEvent;
    pub use winit::keyboard::{Key, KeyCode, NamedKey, NativeKeyCode, PhysicalKey};
}

pub fn event_to_qwerty(event: &winit::KeyEvent) -> Option<Qwerty> {
    let by_logical = match event.logical_key {
        winit::Key::Named(named_key) => {
            match named_key {
                winit::NamedKey::AudioVolumeMute => Some(Qwerty::TrackMute),
                winit::NamedKey::AudioVolumeDown => Some(Qwerty::TrackVolDown),
                winit::NamedKey::AudioVolumeUp => Some(Qwerty::TrackVolUp),
                winit::NamedKey::MediaPlayPause => Some(Qwerty::TrackPlayPause),
                winit::NamedKey::MediaLast => Some(Qwerty::TrackBack),
                winit::NamedKey::MediaSkipForward => Some(Qwerty::TrackNext),
                _ => None,
            }
        },
        _ => None,
    };

    if let Some(qwerty) = by_logical {
        return Some(qwerty);
    }

    match event.physical_key {
        winit::PhysicalKey::Code(code) => {
            match code {
                winit::KeyCode::Escape => Some(Qwerty::Esc),
                winit::KeyCode::F1 => Some(Qwerty::F1),
                winit::KeyCode::F2 => Some(Qwerty::F2),
                winit::KeyCode::F3 => Some(Qwerty::F3),
                winit::KeyCode::F4 => Some(Qwerty::F4),
                winit::KeyCode::F5 => Some(Qwerty::F5),
                winit::KeyCode::F6 => Some(Qwerty::F6),
                winit::KeyCode::F7 => Some(Qwerty::F7),
                winit::KeyCode::F8 => Some(Qwerty::F8),
                winit::KeyCode::F9 => Some(Qwerty::F9),
                winit::KeyCode::F10 => Some(Qwerty::F10),
                winit::KeyCode::F11 => Some(Qwerty::F11),
                winit::KeyCode::F12 => Some(Qwerty::F12),
                winit::KeyCode::Backquote => Some(Qwerty::Tilda),
                winit::KeyCode::Digit1 => Some(Qwerty::One),
                winit::KeyCode::Digit2 => Some(Qwerty::Two),
                winit::KeyCode::Digit3 => Some(Qwerty::Three),
                winit::KeyCode::Digit4 => Some(Qwerty::Four),
                winit::KeyCode::Digit5 => Some(Qwerty::Five),
                winit::KeyCode::Digit6 => Some(Qwerty::Six),
                winit::KeyCode::Digit7 => Some(Qwerty::Seven),
                winit::KeyCode::Digit8 => Some(Qwerty::Eight),
                winit::KeyCode::Digit9 => Some(Qwerty::Nine),
                winit::KeyCode::Digit0 => Some(Qwerty::Zero),
                winit::KeyCode::Minus => Some(Qwerty::Dash),
                winit::KeyCode::Equal => Some(Qwerty::Equal),
                winit::KeyCode::Backspace => Some(Qwerty::Backspace),
                winit::KeyCode::Tab => Some(Qwerty::Tab),
                winit::KeyCode::KeyQ => Some(Qwerty::Q),
                winit::KeyCode::KeyW => Some(Qwerty::W),
                winit::KeyCode::KeyE => Some(Qwerty::E),
                winit::KeyCode::KeyR => Some(Qwerty::R),
                winit::KeyCode::KeyT => Some(Qwerty::T),
                winit::KeyCode::KeyY => Some(Qwerty::Y),
                winit::KeyCode::KeyU => Some(Qwerty::U),
                winit::KeyCode::KeyI => Some(Qwerty::I),
                winit::KeyCode::KeyO => Some(Qwerty::O),
                winit::KeyCode::KeyP => Some(Qwerty::P),
                winit::KeyCode::BracketLeft => Some(Qwerty::LSqBracket),
                winit::KeyCode::BracketRight => Some(Qwerty::RSqBracket),
                winit::KeyCode::Backslash => Some(Qwerty::Backslash),
                winit::KeyCode::CapsLock => Some(Qwerty::Caps),
                winit::KeyCode::KeyA => Some(Qwerty::A),
                winit::KeyCode::KeyS => Some(Qwerty::S),
                winit::KeyCode::KeyD => Some(Qwerty::D),
                winit::KeyCode::KeyF => Some(Qwerty::F),
                winit::KeyCode::KeyG => Some(Qwerty::G),
                winit::KeyCode::KeyH => Some(Qwerty::H),
                winit::KeyCode::KeyJ => Some(Qwerty::J),
                winit::KeyCode::KeyK => Some(Qwerty::K),
                winit::KeyCode::KeyL => Some(Qwerty::L),
                winit::KeyCode::Semicolon => Some(Qwerty::SemiColon),
                winit::KeyCode::Quote => Some(Qwerty::Parenthesis),
                winit::KeyCode::Enter => Some(Qwerty::Enter),
                winit::KeyCode::ShiftLeft => Some(Qwerty::LShift),
                winit::KeyCode::KeyZ => Some(Qwerty::Z),
                winit::KeyCode::KeyX => Some(Qwerty::X),
                winit::KeyCode::KeyC => Some(Qwerty::C),
                winit::KeyCode::KeyV => Some(Qwerty::V),
                winit::KeyCode::KeyB => Some(Qwerty::B),
                winit::KeyCode::KeyN => Some(Qwerty::N),
                winit::KeyCode::KeyM => Some(Qwerty::M),
                winit::KeyCode::Comma => Some(Qwerty::Comma),
                winit::KeyCode::Period => Some(Qwerty::Period),
                winit::KeyCode::Slash => Some(Qwerty::Slash),
                winit::KeyCode::ShiftRight => Some(Qwerty::RShift),
                winit::KeyCode::ControlLeft => Some(Qwerty::LCtrl),
                winit::KeyCode::SuperLeft => Some(Qwerty::LSuper),
                winit::KeyCode::AltLeft => Some(Qwerty::LAlt),
                winit::KeyCode::Space => Some(Qwerty::Space),
                winit::KeyCode::AltRight => Some(Qwerty::RAlt),
                winit::KeyCode::SuperRight => Some(Qwerty::RSuper),
                winit::KeyCode::ControlRight => Some(Qwerty::RCtrl),
                winit::KeyCode::PrintScreen => Some(Qwerty::PrintScreen),
                winit::KeyCode::ScrollLock => Some(Qwerty::ScrollLock),
                winit::KeyCode::Pause => Some(Qwerty::Pause),
                winit::KeyCode::Insert => Some(Qwerty::Insert),
                winit::KeyCode::Home => Some(Qwerty::Home),
                winit::KeyCode::PageUp => Some(Qwerty::PageUp),
                winit::KeyCode::Delete => Some(Qwerty::Delete),
                winit::KeyCode::End => Some(Qwerty::End),
                winit::KeyCode::PageDown => Some(Qwerty::PageDown),
                winit::KeyCode::ArrowUp => Some(Qwerty::ArrowUp),
                winit::KeyCode::ArrowDown => Some(Qwerty::ArrowDown),
                winit::KeyCode::ArrowLeft => Some(Qwerty::ArrowLeft),
                winit::KeyCode::ArrowRight => Some(Qwerty::ArrowRight),
                _ => None,
            }
        },
        winit::PhysicalKey::Unidentified(winit::NativeKeyCode::Windows(0xE11D)) => {
            Some(Qwerty::Pause)
        },
        _ => None,
    }
}
