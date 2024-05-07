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
}
