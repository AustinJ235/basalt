#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Qwery {
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

#[derive(Debug, Clone)]
pub enum Character {
	Backspace,
	Value(char),
}

impl Qwery {
	pub fn into_char(self, shift: bool) -> Option<Character> {
		match shift {
			false =>
				match self {
					Qwery::Esc => None,
					Qwery::F1 => None,
					Qwery::F2 => None,
					Qwery::F3 => None,
					Qwery::F4 => None,
					Qwery::F5 => None,
					Qwery::F6 => None,
					Qwery::F7 => None,
					Qwery::F8 => None,
					Qwery::F9 => None,
					Qwery::F10 => None,
					Qwery::F11 => None,
					Qwery::F12 => None,
					Qwery::Tilda => Some(Character::Value('`')),
					Qwery::One => Some(Character::Value('1')),
					Qwery::Two => Some(Character::Value('2')),
					Qwery::Three => Some(Character::Value('3')),
					Qwery::Four => Some(Character::Value('4')),
					Qwery::Five => Some(Character::Value('5')),
					Qwery::Six => Some(Character::Value('6')),
					Qwery::Seven => Some(Character::Value('7')),
					Qwery::Eight => Some(Character::Value('8')),
					Qwery::Nine => Some(Character::Value('9')),
					Qwery::Zero => Some(Character::Value('0')),
					Qwery::Dash => Some(Character::Value('-')),
					Qwery::Equal => Some(Character::Value('=')),
					Qwery::Backspace => Some(Character::Backspace),
					Qwery::Tab => None,
					Qwery::Q => Some(Character::Value('q')),
					Qwery::W => Some(Character::Value('w')),
					Qwery::E => Some(Character::Value('e')),
					Qwery::R => Some(Character::Value('r')),
					Qwery::T => Some(Character::Value('t')),
					Qwery::Y => Some(Character::Value('y')),
					Qwery::U => Some(Character::Value('u')),
					Qwery::I => Some(Character::Value('i')),
					Qwery::O => Some(Character::Value('o')),
					Qwery::P => Some(Character::Value('p')),
					Qwery::LSqBracket => Some(Character::Value('[')),
					Qwery::RSqBracket => Some(Character::Value(']')),
					Qwery::Backslash => Some(Character::Value('\\')),
					Qwery::Caps => None,
					Qwery::A => Some(Character::Value('a')),
					Qwery::S => Some(Character::Value('s')),
					Qwery::D => Some(Character::Value('d')),
					Qwery::F => Some(Character::Value('f')),
					Qwery::G => Some(Character::Value('g')),
					Qwery::H => Some(Character::Value('h')),
					Qwery::J => Some(Character::Value('j')),
					Qwery::K => Some(Character::Value('k')),
					Qwery::L => Some(Character::Value('l')),
					Qwery::SemiColon => Some(Character::Value(';')),
					Qwery::Parenthesis => Some(Character::Value('\'')),
					Qwery::Enter => Some(Character::Value('\n')),
					Qwery::LShift => None,
					Qwery::Z => Some(Character::Value('z')),
					Qwery::X => Some(Character::Value('x')),
					Qwery::C => Some(Character::Value('c')),
					Qwery::V => Some(Character::Value('v')),
					Qwery::B => Some(Character::Value('b')),
					Qwery::N => Some(Character::Value('n')),
					Qwery::M => Some(Character::Value('m')),
					Qwery::Comma => Some(Character::Value(',')),
					Qwery::Period => Some(Character::Value('.')),
					Qwery::Slash => Some(Character::Value('/')),
					Qwery::RShift => None,
					Qwery::LCtrl => None,
					Qwery::LSuper => None,
					Qwery::LAlt => None,
					Qwery::Space => Some(Character::Value(' ')),
					Qwery::RAlt => None,
					Qwery::RSuper => None,
					Qwery::RCtrl => None,
					Qwery::PrintScreen => None,
					Qwery::ScrollLock => None,
					Qwery::Pause => None,
					Qwery::Insert => None,
					Qwery::Home => None,
					Qwery::PageUp => None,
					Qwery::Delete => None,
					Qwery::End => None,
					Qwery::PageDown => None,
					Qwery::ArrowUp => None,
					Qwery::ArrowLeft => None,
					Qwery::ArrowDown => None,
					Qwery::ArrowRight => None,
					Qwery::TrackMute => None,
					Qwery::TrackVolDown => None,
					Qwery::TrackVolUp => None,
					Qwery::TrackPlayPause => None,
					Qwery::TrackBack => None,
					Qwery::TrackNext => None,
				},
			true =>
				match self {
					Qwery::Esc => None,
					Qwery::F1 => None,
					Qwery::F2 => None,
					Qwery::F3 => None,
					Qwery::F4 => None,
					Qwery::F5 => None,
					Qwery::F6 => None,
					Qwery::F7 => None,
					Qwery::F8 => None,
					Qwery::F9 => None,
					Qwery::F10 => None,
					Qwery::F11 => None,
					Qwery::F12 => None,
					Qwery::Tilda => Some(Character::Value('~')),
					Qwery::One => Some(Character::Value('!')),
					Qwery::Two => Some(Character::Value('@')),
					Qwery::Three => Some(Character::Value('#')),
					Qwery::Four => Some(Character::Value('$')),
					Qwery::Five => Some(Character::Value('%')),
					Qwery::Six => Some(Character::Value('^')),
					Qwery::Seven => Some(Character::Value('&')),
					Qwery::Eight => Some(Character::Value('*')),
					Qwery::Nine => Some(Character::Value('(')),
					Qwery::Zero => Some(Character::Value(')')),
					Qwery::Dash => Some(Character::Value('_')),
					Qwery::Equal => Some(Character::Value('+')),
					Qwery::Backspace => Some(Character::Backspace),
					Qwery::Tab => None,
					Qwery::Q => Some(Character::Value('Q')),
					Qwery::W => Some(Character::Value('W')),
					Qwery::E => Some(Character::Value('E')),
					Qwery::R => Some(Character::Value('R')),
					Qwery::T => Some(Character::Value('T')),
					Qwery::Y => Some(Character::Value('Y')),
					Qwery::U => Some(Character::Value('U')),
					Qwery::I => Some(Character::Value('I')),
					Qwery::O => Some(Character::Value('O')),
					Qwery::P => Some(Character::Value('P')),
					Qwery::LSqBracket => Some(Character::Value('{')),
					Qwery::RSqBracket => Some(Character::Value('}')),
					Qwery::Backslash => Some(Character::Value('|')),
					Qwery::Caps => None,
					Qwery::A => Some(Character::Value('A')),
					Qwery::S => Some(Character::Value('S')),
					Qwery::D => Some(Character::Value('D')),
					Qwery::F => Some(Character::Value('F')),
					Qwery::G => Some(Character::Value('G')),
					Qwery::H => Some(Character::Value('H')),
					Qwery::J => Some(Character::Value('J')),
					Qwery::K => Some(Character::Value('K')),
					Qwery::L => Some(Character::Value('L')),
					Qwery::SemiColon => Some(Character::Value(':')),
					Qwery::Parenthesis => Some(Character::Value('"')),
					Qwery::Enter => Some(Character::Value('\n')),
					Qwery::LShift => None,
					Qwery::Z => Some(Character::Value('Z')),
					Qwery::X => Some(Character::Value('X')),
					Qwery::C => Some(Character::Value('C')),
					Qwery::V => Some(Character::Value('V')),
					Qwery::B => Some(Character::Value('B')),
					Qwery::N => Some(Character::Value('N')),
					Qwery::M => Some(Character::Value('M')),
					Qwery::Comma => Some(Character::Value('<')),
					Qwery::Period => Some(Character::Value('>')),
					Qwery::Slash => Some(Character::Value('?')),
					Qwery::RShift => None,
					Qwery::LCtrl => None,
					Qwery::LSuper => None,
					Qwery::LAlt => None,
					Qwery::Space => Some(Character::Value(' ')),
					Qwery::RAlt => None,
					Qwery::RSuper => None,
					Qwery::RCtrl => None,
					Qwery::PrintScreen => None,
					Qwery::ScrollLock => None,
					Qwery::Pause => None,
					Qwery::Insert => None,
					Qwery::Home => None,
					Qwery::PageUp => None,
					Qwery::Delete => None,
					Qwery::End => None,
					Qwery::PageDown => None,
					Qwery::ArrowUp => None,
					Qwery::ArrowLeft => None,
					Qwery::ArrowDown => None,
					Qwery::ArrowRight => None,
					Qwery::TrackMute => None,
					Qwery::TrackVolDown => None,
					Qwery::TrackVolUp => None,
					Qwery::TrackPlayPause => None,
					Qwery::TrackBack => None,
					Qwery::TrackNext => None,
				},
		}
	}
}

impl Into<u32> for Qwery {
	fn into(self) -> u32 {
		// Linux X11
		match self {
			Qwery::Esc => 1,
			Qwery::F1 => 59,
			Qwery::F2 => 60,
			Qwery::F3 => 61,
			Qwery::F4 => 62,
			Qwery::F5 => 63,
			Qwery::F6 => 64,
			Qwery::F7 => 65,
			Qwery::F8 => 66,
			Qwery::F9 => 67,
			Qwery::F10 => 68,
			Qwery::F11 => 87,
			Qwery::F12 => 88,
			Qwery::Tilda => 41,
			Qwery::One => 2,
			Qwery::Two => 3,
			Qwery::Three => 4,
			Qwery::Four => 5,
			Qwery::Five => 6,
			Qwery::Six => 7,
			Qwery::Seven => 8,
			Qwery::Eight => 9,
			Qwery::Nine => 10,
			Qwery::Zero => 11,
			Qwery::Dash => 12,
			Qwery::Equal => 13,
			Qwery::Backspace => 14,
			Qwery::Tab => 15,
			Qwery::Q => 16,
			Qwery::W => 17,
			Qwery::E => 18,
			Qwery::R => 19,
			Qwery::T => 20,
			Qwery::Y => 21,
			Qwery::U => 22,
			Qwery::I => 23,
			Qwery::O => 24,
			Qwery::P => 25,
			Qwery::LSqBracket => 26,
			Qwery::RSqBracket => 27,
			Qwery::Backslash => 43,
			Qwery::Caps => 58,
			Qwery::A => 30,
			Qwery::S => 31,
			Qwery::D => 32,
			Qwery::F => 33,
			Qwery::G => 34,
			Qwery::H => 35,
			Qwery::J => 36,
			Qwery::K => 37,
			Qwery::L => 38,
			Qwery::SemiColon => 39,
			Qwery::Parenthesis => 40,
			Qwery::Enter => 28,
			Qwery::LShift => 42,
			Qwery::Z => 44,
			Qwery::X => 45,
			Qwery::C => 46,
			Qwery::V => 47,
			Qwery::B => 48,
			Qwery::N => 49,
			Qwery::M => 50,
			Qwery::Comma => 51,
			Qwery::Period => 52,
			Qwery::Slash => 53,
			Qwery::RShift => 54,
			Qwery::LCtrl => 29,
			Qwery::LAlt => 56,
			Qwery::Space => 57,
			Qwery::RAlt => 100,
			Qwery::RSuper => 126,
			Qwery::RCtrl => 97,
			Qwery::PrintScreen => 99,
			Qwery::ScrollLock => 70,
			Qwery::Insert => 110,
			Qwery::TrackMute => 113,
			Qwery::TrackVolDown => 114,
			Qwery::TrackVolUp => 115,
			Qwery::TrackPlayPause => 164,
			Qwery::TrackBack => 165,
			Qwery::TrackNext => 163,
			_ => {
				#[cfg(target_os = "windows")]
				{
					match self {
						Qwery::LSuper => 71,
						Qwery::RSuper => 92,
						Qwery::RCtrl => 29,
						Qwery::Pause => 69,
						Qwery::Home => 71,
						Qwery::PageUp => 73,
						Qwery::Delete => 83,
						Qwery::End => 79,
						Qwery::PageDown => 81,
						Qwery::ArrowUp => 72,
						Qwery::ArrowLeft => 75,
						Qwery::ArrowDown => 80,
						Qwery::ArrowRight => 77,
						_ => unreachable!(),
					}
				}
				#[cfg(not(target_os = "windows"))]
				{
					match self {
						Qwery::LSuper => 125,
						Qwery::RSuper => 126,
						Qwery::RCtrl => 97,
						Qwery::Pause => 119,
						Qwery::Home => 102,
						Qwery::PageUp => 104,
						Qwery::Delete => 111,
						Qwery::End => 107,
						Qwery::PageDown => 109,
						Qwery::ArrowUp => 103,
						Qwery::ArrowLeft => 105,
						Qwery::ArrowDown => 108,
						Qwery::ArrowRight => 106,
						_ => unreachable!(),
					}
				}
			},
		}
	}
}

// TODO: Replace with try_from when that is stable
impl From<u32> for Qwery {
	fn from(code: u32) -> Qwery {
		// Linux X11
		match code {
			1 => Qwery::Esc,
			59 => Qwery::F1,
			60 => Qwery::F2,
			61 => Qwery::F3,
			62 => Qwery::F4,
			63 => Qwery::F5,
			64 => Qwery::F6,
			65 => Qwery::F7,
			66 => Qwery::F8,
			67 => Qwery::F9,
			68 => Qwery::F10,
			87 => Qwery::F11,
			88 => Qwery::F12,
			41 => Qwery::Tilda,
			2 => Qwery::One,
			3 => Qwery::Two,
			4 => Qwery::Three,
			5 => Qwery::Four,
			6 => Qwery::Five,
			7 => Qwery::Six,
			8 => Qwery::Seven,
			9 => Qwery::Eight,
			10 => Qwery::Nine,
			11 => Qwery::Zero,
			12 => Qwery::Dash,
			13 => Qwery::Equal,
			14 => Qwery::Backspace,
			15 => Qwery::Tab,
			16 => Qwery::Q,
			17 => Qwery::W,
			18 => Qwery::E,
			19 => Qwery::R,
			20 => Qwery::T,
			21 => Qwery::Y,
			22 => Qwery::U,
			23 => Qwery::I,
			24 => Qwery::O,
			25 => Qwery::P,
			26 => Qwery::LSqBracket,
			27 => Qwery::RSqBracket,
			43 => Qwery::Backslash,
			58 => Qwery::Caps,
			30 => Qwery::A,
			31 => Qwery::S,
			32 => Qwery::D,
			33 => Qwery::F,
			34 => Qwery::G,
			35 => Qwery::H,
			36 => Qwery::J,
			37 => Qwery::K,
			38 => Qwery::L,
			39 => Qwery::SemiColon,
			40 => Qwery::Parenthesis,
			28 => Qwery::Enter,
			42 => Qwery::LShift,
			44 => Qwery::Z,
			45 => Qwery::X,
			46 => Qwery::C,
			47 => Qwery::V,
			48 => Qwery::B,
			49 => Qwery::N,
			50 => Qwery::M,
			51 => Qwery::Comma,
			52 => Qwery::Period,
			53 => Qwery::Slash,
			54 => Qwery::RShift,
			29 => Qwery::LCtrl,
			56 => Qwery::LAlt,
			57 => Qwery::Space,
			100 => Qwery::RAlt,
			99 => Qwery::PrintScreen,
			70 => Qwery::ScrollLock,
			110 => Qwery::Insert,
			113 => Qwery::TrackMute,
			114 => Qwery::TrackVolDown,
			115 => Qwery::TrackVolUp,
			164 => Qwery::TrackPlayPause,
			165 => Qwery::TrackBack,
			163 => Qwery::TrackNext,
			_ => {
				#[cfg(target_os = "windows")]
				{
					match code {
						91 => Qwery::LSuper,
						92 => Qwery::RSuper,
						29 => Qwery::RCtrl,
						69 => Qwery::Pause,
						71 => Qwery::Home,
						73 => Qwery::PageUp,
						83 => Qwery::Delete,
						79 => Qwery::End,
						81 => Qwery::PageDown,
						72 => Qwery::ArrowUp,
						75 => Qwery::ArrowLeft,
						80 => Qwery::ArrowDown,
						77 => Qwery::ArrowRight,
						_ => {
							println!("Qwery from ScanCode: Unsupported keycode: {}", code);
							Qwery::Esc
						},
					}
				}
				#[cfg(not(target_os = "windows"))]
				{
					match code {
						125 => Qwery::LSuper,
						126 => Qwery::RSuper,
						97 => Qwery::RCtrl,
						119 => Qwery::Pause,
						102 => Qwery::Home,
						104 => Qwery::PageUp,
						111 => Qwery::Delete,
						107 => Qwery::End,
						109 => Qwery::PageDown,
						103 => Qwery::ArrowUp,
						105 => Qwery::ArrowLeft,
						108 => Qwery::ArrowDown,
						106 => Qwery::ArrowRight,
						_ => {
							println!("Qwery from ScanCode: Unsupported keycode: {}", code);
							Qwery::Esc
						},
					}
				}
			},
		}
	}
}
