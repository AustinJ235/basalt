//! Various Key related definitions

use std::ops::Deref;

/// A keyboard/mouse agnostic type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    Keyboard(Qwerty),
    Mouse(MouseButton),
}

impl From<Qwerty> for Key {
    fn from(key: Qwerty) -> Self {
        Key::Keyboard(key)
    }
}

impl From<MouseButton> for Key {
    fn from(key: MouseButton) -> Self {
        Key::Mouse(key)
    }
}

/// A wrapper around `char` that provides some convenience methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Char(pub char);

impl Char {
    /// Modifies the provided string.
    /// - Backspace: pops character
    /// - Carriage Return: adds new line
    /// - Regular: pushes character
    pub fn modify_string(self, string: &mut String) {
        match self.0 {
            '\x08' => {
                string.pop();
            },
            '\r' => {
                string.push('\n');
            },
            c => {
                string.push(c);
            },
        }
    }

    /// Returns true if character is either carriage return or line feed.
    pub fn is_new_line(&self) -> bool {
        self.0 == '\r' || self.0 == '\n'
    }

    /// Return true if character is backspace
    pub fn is_backspace(&self) -> bool {
        self.0 == '\x08'
    }
}

impl From<Char> for char {
    fn from(c: Char) -> Self {
        c.0
    }
}

impl From<char> for Char {
    fn from(c: char) -> Self {
        Self(c)
    }
}

impl Deref for Char {
    type Target = char;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Trait used for various methods that can take multiple `Key`s.
///
/// A `Key` can be either `Qwerty` or `MouseButton`.
///
/// Supports being a lone key, a `Vec`, an `array` or a `tuple`.
pub trait KeyCombo {
    fn into_vec(self) -> Vec<Key>;
}

impl<T: Into<Key>> KeyCombo for T {
    #[inline]
    fn into_vec(self) -> Vec<Key> {
        vec![self.into()]
    }
}

impl<T: Into<Key>> KeyCombo for Vec<T> {
    #[inline]
    fn into_vec(self) -> Vec<Key> {
        self.into_iter().map(|key| key.into()).collect()
    }
}

impl<T: Into<Key>, const N: usize> KeyCombo for [T; N] {
    #[inline]
    fn into_vec(self) -> Vec<Key> {
        self.into_iter().map(|key| key.into()).collect()
    }
}

macro_rules! impl_tuple_combo {
    ($first:ident $(, $others:ident)+) => (
        impl<$first$(, $others)+> KeyCombo for ($first, $($others),+)
            where $first: Into<Key>
                  $(, $others: Into<Key>)*
        {
            #[inline]
            #[allow(non_snake_case)]
            fn into_vec(self) -> Vec<Key> {
                let ($first, $($others,)*) = self;
                vec![$first.into() $(, $others.into())+]
            }
        }

        impl_tuple_combo!($($others),+);
    );

    ($i:ident) => ();
}

impl_tuple_combo!(A, B, C, D, E, F, G, H);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Hash)]
/// Enum of mouse buttons.
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other(u8),
}

/// For use when key location matters. May not always correlate to the actual key.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Qwerty {
    Esc,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Tilda,
    One,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Zero,
    Dash,
    Equal,
    Backspace,
    Tab,
    Q,
    W,
    E,
    R,
    T,
    Y,
    U,
    I,
    O,
    P,
    LSqBracket,
    RSqBracket,
    Backslash,
    Caps,
    A,
    S,
    D,
    F,
    G,
    H,
    J,
    K,
    L,
    SemiColon,
    Parenthesis,
    Enter,
    LShift,
    Z,
    X,
    C,
    V,
    B,
    N,
    M,
    Comma,
    Period,
    Slash,
    RShift,
    LCtrl,
    LSuper,
    LAlt,
    Space,
    RAlt,
    RSuper,
    RCtrl,
    PrintScreen,
    ScrollLock,
    Pause,
    Insert,
    Home,
    PageUp,
    Delete,
    End,
    PageDown,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    TrackMute,
    TrackVolDown,
    TrackVolUp,
    TrackPlayPause,
    TrackBack,
    TrackNext,
    Unknown(u32),
}

impl From<Qwerty> for u32 {
    fn from(key: Qwerty) -> u32 {
        // Linux X11
        match key {
            Qwerty::Esc => 1,
            Qwerty::F1 => 59,
            Qwerty::F2 => 60,
            Qwerty::F3 => 61,
            Qwerty::F4 => 62,
            Qwerty::F5 => 63,
            Qwerty::F6 => 64,
            Qwerty::F7 => 65,
            Qwerty::F8 => 66,
            Qwerty::F9 => 67,
            Qwerty::F10 => 68,
            Qwerty::F11 => 87,
            Qwerty::F12 => 88,
            Qwerty::Tilda => 41,
            Qwerty::One => 2,
            Qwerty::Two => 3,
            Qwerty::Three => 4,
            Qwerty::Four => 5,
            Qwerty::Five => 6,
            Qwerty::Six => 7,
            Qwerty::Seven => 8,
            Qwerty::Eight => 9,
            Qwerty::Nine => 10,
            Qwerty::Zero => 11,
            Qwerty::Dash => 12,
            Qwerty::Equal => 13,
            Qwerty::Backspace => 14,
            Qwerty::Tab => 15,
            Qwerty::Q => 16,
            Qwerty::W => 17,
            Qwerty::E => 18,
            Qwerty::R => 19,
            Qwerty::T => 20,
            Qwerty::Y => 21,
            Qwerty::U => 22,
            Qwerty::I => 23,
            Qwerty::O => 24,
            Qwerty::P => 25,
            Qwerty::LSqBracket => 26,
            Qwerty::RSqBracket => 27,
            Qwerty::Backslash => 43,
            Qwerty::Caps => 58,
            Qwerty::A => 30,
            Qwerty::S => 31,
            Qwerty::D => 32,
            Qwerty::F => 33,
            Qwerty::G => 34,
            Qwerty::H => 35,
            Qwerty::J => 36,
            Qwerty::K => 37,
            Qwerty::L => 38,
            Qwerty::SemiColon => 39,
            Qwerty::Parenthesis => 40,
            Qwerty::Enter => 28,
            Qwerty::LShift => 42,
            Qwerty::Z => 44,
            Qwerty::X => 45,
            Qwerty::C => 46,
            Qwerty::V => 47,
            Qwerty::B => 48,
            Qwerty::N => 49,
            Qwerty::M => 50,
            Qwerty::Comma => 51,
            Qwerty::Period => 52,
            Qwerty::Slash => 53,
            Qwerty::RShift => 54,
            Qwerty::LCtrl => 29,
            Qwerty::LAlt => 56,
            Qwerty::Space => 57,
            Qwerty::RAlt => 100,
            Qwerty::RSuper => 126,
            Qwerty::RCtrl => 97,
            Qwerty::PrintScreen => 99,
            Qwerty::ScrollLock => 70,
            Qwerty::Insert => 110,
            Qwerty::TrackMute => 113,
            Qwerty::TrackVolDown => 114,
            Qwerty::TrackVolUp => 115,
            Qwerty::TrackPlayPause => 164,
            Qwerty::TrackBack => 165,
            Qwerty::TrackNext => 163,
            _ => {
                #[cfg(target_os = "windows")]
                {
                    match key {
                        Qwerty::LSuper => 71,
                        Qwerty::RSuper => 92,
                        Qwerty::RCtrl => 29,
                        Qwerty::Pause => 69,
                        Qwerty::Home => 71,
                        Qwerty::PageUp => 73,
                        Qwerty::Delete => 83,
                        Qwerty::End => 79,
                        Qwerty::PageDown => 81,
                        Qwerty::ArrowUp => 72,
                        Qwerty::ArrowLeft => 75,
                        Qwerty::ArrowDown => 80,
                        Qwerty::ArrowRight => 77,
                        _ => unreachable!(),
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    match key {
                        Qwerty::LSuper => 125,
                        Qwerty::RSuper => 126,
                        Qwerty::RCtrl => 97,
                        Qwerty::Pause => 119,
                        Qwerty::Home => 102,
                        Qwerty::PageUp => 104,
                        Qwerty::Delete => 111,
                        Qwerty::End => 107,
                        Qwerty::PageDown => 109,
                        Qwerty::ArrowUp => 103,
                        Qwerty::ArrowLeft => 105,
                        Qwerty::ArrowDown => 108,
                        Qwerty::ArrowRight => 106,
                        _ => unreachable!(),
                    }
                }
            },
        }
    }
}

impl From<u32> for Qwerty {
    fn from(code: u32) -> Qwerty {
        // Linux X11
        match code {
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
            54 => Qwerty::RShift,
            29 => Qwerty::LCtrl,
            56 => Qwerty::LAlt,
            57 => Qwerty::Space,
            100 => Qwerty::RAlt,
            99 => Qwerty::PrintScreen,
            70 => Qwerty::ScrollLock,
            110 => Qwerty::Insert,
            113 => Qwerty::TrackMute,
            114 => Qwerty::TrackVolDown,
            115 => Qwerty::TrackVolUp,
            164 => Qwerty::TrackPlayPause,
            165 => Qwerty::TrackBack,
            163 => Qwerty::TrackNext,
            _ => {
                #[cfg(target_os = "windows")]
                {
                    match code {
                        91 => Qwerty::LSuper,
                        92 => Qwerty::RSuper,
                        29 => Qwerty::RCtrl,
                        69 => Qwerty::Pause,
                        71 => Qwerty::Home,
                        73 => Qwerty::PageUp,
                        83 => Qwerty::Delete,
                        79 => Qwerty::End,
                        81 => Qwerty::PageDown,
                        72 => Qwerty::ArrowUp,
                        75 => Qwerty::ArrowLeft,
                        80 => Qwerty::ArrowDown,
                        77 => Qwerty::ArrowRight,
                        _ => {
                            println!("[Basalt]: Unknown Scan Code: {}", code);
                            Qwerty::Unknown(code)
                        },
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    match code {
                        125 => Qwerty::LSuper,
                        126 => Qwerty::RSuper,
                        97 => Qwerty::RCtrl,
                        119 => Qwerty::Pause,
                        102 => Qwerty::Home,
                        104 => Qwerty::PageUp,
                        111 => Qwerty::Delete,
                        107 => Qwerty::End,
                        109 => Qwerty::PageDown,
                        103 => Qwerty::ArrowUp,
                        105 => Qwerty::ArrowLeft,
                        108 => Qwerty::ArrowDown,
                        106 => Qwerty::ArrowRight,
                        _ => {
                            println!("[Basalt]: Unknown Scan Code: {}", code);
                            Qwerty::Unknown(code)
                        },
                    }
                }
            },
        }
    }
}
