use std::sync::{Arc,Barrier};
use std::time::{Duration,Instant};
use Engine;
use std::thread::{self,JoinHandle};
use parking_lot::Mutex;
use std::collections::HashMap;
use crossbeam::queue::MsQueue;
use misc::HashMapExtras;
use winit;

type HookFunc = Arc<Fn(CallInfo) + Send + Sync>;

struct Hook {
	id: u64,
	on_char: bool,
	on_press: bool,
	on_hold: bool,
	on_release: bool,
	keys: Vec<Vec<u32>>,
	frequency: Option<u64>,
	function: HookFunc,
	press_start: Option<Instant>,
	first_call_this_press: bool,
	last_call: Option<Instant>,
	active: bool,
}

pub struct CallInfo {
	pub hook_id: u64,
	pub engine: Arc<Engine>,
	pub ty: CallType,
	pub elapsed: Option<f64>,
	pub combos: Vec<Vec<Qwery>>,
	pub char_ty: Option<CharType>,
	pub first_call_this_press: bool,
	pub hold_time: f64,
}

pub enum CallType {
	Press,
	Hold,
	Release,
	Unknown,
}

enum Event {
	Press(u32),
	Release(u32),
	NewHook(Hook),
	DeleteHook(u64),
	DelayTest(Arc<Barrier>),
}

pub struct Keyboard {
	engine: Arc<Engine>,
	hook_i: Mutex<u64>,
	event_queue: Arc<MsQueue<Event>>,
	exec_thread: Mutex<Option<JoinHandle<()>>>,
}

impl Keyboard {
	fn next_hook_id(&self) -> u64 {
		let mut hook_i = self.hook_i.lock();
		let out = *hook_i;
		*hook_i += 1;
		out
	}
	
	pub fn delete_hook(&self, id: u64) {
		self.event_queue.push(Event::DeleteHook(id));
	}
	
	pub fn on_char_press(&self, func: HookFunc) -> u64 {
		let id = self.next_hook_id();
		self.event_queue.push(Event::NewHook(Hook {
			id: id,
			on_char: true,
			on_press: false,
			on_hold: false,
			on_release: false,
			keys: Vec::new(),
			frequency: None,
			function: func,
			first_call_this_press: false,
			last_call: None,
			active: false,
			press_start: None,
		})); id
	}
	
	pub fn on_hold<K: Into<winit::ScanCode>>(&self, combos: Vec<Vec<K>>, freq: u64, func: HookFunc) -> u64 {
		let id = self.next_hook_id();
		let keys: Vec<Vec<u32>> = combos.into_iter().map(|v| v.into_iter().map(|k| k.into()).collect()).collect();
		
		self.event_queue.push(Event::NewHook(Hook {
			id: id,
			on_press: false,
			on_hold: true,
			on_release: false,
			on_char: false,
			keys: keys,
			frequency: Some(freq),
			function: func,
			first_call_this_press: false,
			last_call: None,
			active: false,
			press_start: None,
		})); id
	}
	
	pub fn on_press_and_hold<K: Into<winit::ScanCode>>(&self, combos: Vec<Vec<K>>, freq: u64, func: HookFunc) -> u64 {
		let id = self.next_hook_id();
		let keys: Vec<Vec<u32>> = combos.into_iter().map(|v| v.into_iter().map(|k| k.into()).collect()).collect();
		
		self.event_queue.push(Event::NewHook(Hook {
			id: id,
			on_press: true,
			on_hold: true,
			on_release: false,
			on_char: false,
			keys: keys,
			frequency: Some(freq),
			function: func,
			first_call_this_press: false,
			last_call: None,
			active: false,
			press_start: None,
		})); id
	}
	
	pub fn on_press<K: Into<winit::ScanCode>>(&self, combos: Vec<Vec<K>>, func: HookFunc) -> u64 {
		let id = self.next_hook_id();
		let keys: Vec<Vec<u32>> = combos.into_iter().map(|v| v.into_iter().map(|k| k.into()).collect()).collect();
		
		self.event_queue.push(Event::NewHook(Hook {
			id: id,
			on_press: true,
			on_hold: false,
			on_release: false,
			on_char: false,
			keys: keys,
			frequency: None,
			function: func,
			first_call_this_press: false,
			last_call: None,
			active: false,
			press_start: None,
		})); id
	}
	
	pub fn on_release<K: Into<winit::ScanCode>>(&self, combos: Vec<Vec<K>>, func: HookFunc) -> u64 {
		let id = self.next_hook_id();
		let keys: Vec<Vec<u32>> = combos.into_iter().map(|v| v.into_iter().map(|k| k.into()).collect()).collect();
		
		self.event_queue.push(Event::NewHook(Hook {
			id: id,
			on_press: false,
			on_hold: false,
			on_release: true,
			on_char: false,
			keys: keys,
			frequency: None,
			function: func,
			first_call_this_press: false,
			last_call: None,
			active: false,
			press_start: None,
		})); id
	}
	
	pub fn delay_test(&self) -> f64 {
		let barrier = Arc::new(Barrier::new(2));
		let now = Instant::now();
		self.event_queue.push(Event::DelayTest(barrier.clone()));
		barrier.wait();
		let elapsed = now.elapsed();
		((elapsed.as_secs() * 1000000000) + elapsed.subsec_nanos() as u64) as f64 / 1000000.0
	}

	pub(crate) fn press(&self, code: u32) {
		self.event_queue.push(Event::Press(code));
	}
	
	pub(crate) fn release(&self, code: u32) {
		self.event_queue.push(Event::Release(code));
	}

	pub fn new(engine: Arc<Engine>) -> Arc<Self> {
		let keyboard = Arc::new(Keyboard {
			engine: engine,
			hook_i: Mutex::new(0),
			exec_thread: Mutex::new(None),
			event_queue: Arc::new(MsQueue::new()),
		});
		
		let keyboard_copy = keyboard.clone();
		let handle = thread::spawn(move || {
			let keyboard = keyboard_copy;
			let mut key_state: HashMap<u32, u8> = HashMap::new();
			let mut hooks = Vec::new();
			
			let active = |key_state: &mut HashMap<u32, u8>, check: &Vec<Vec<u32>>| -> (bool, Vec<_>) {
				let mut active = false;
				let mut active_combos = Vec::new();
				
				for combo in check {
					let mut pressed = true;
					
					for key in combo {
						if *key_state.get_mut_or_create(&key, 2) == 2 {
							pressed = false;
							break;
						}
					}
					
					if pressed {
						active = true;
						active_combos.push(combo.iter().cloned().map(|v| Qwery::from(v)).collect());
					}
				} (active, active_combos)
			};
			
			loop {
				let iter_start = Instant::now();
				let mut new_pressed = Vec::new();
				let mut delay_test_barriers = Vec::new();
				
				while let Some(event) = keyboard.event_queue.try_pop() {
					let _ = match event {
						Event::Press(k) => {
							new_pressed.push(k);
							key_state.insert(k, 1);
						}, Event::Release(k) => {
							key_state.insert(k, 2);
						}, Event::NewHook(h) => {
							hooks.push(h);
						}, Event::DelayTest(b) => {
							delay_test_barriers.push(b);
						}, Event::DeleteHook(id) => {
							let mut delete_i = None;
							for (i, hook) in hooks.iter().enumerate() {
								if hook.id == id {
									delete_i = Some(i);
									break;
								}
							} if let Some(i) = delete_i {
								hooks.swap_remove(i);
							} else {
								println!("[ENGINE]: Keyboard failed to remove hook id: {}", id);
							}
						},
					};
				}
				
				if !new_pressed.is_empty() {
					let lshift_code: winit::ScanCode = Qwery::LShift.into();
					let rshift_code: winit::ScanCode = Qwery::RShift.into();
					
					let shift = *key_state.get_mut_or_create(&(lshift_code as u32), 2) == 1 ||
						*key_state.get_mut_or_create(&(rshift_code as u32), 2) == 1;
					
					for code in new_pressed {
						if let Some(char_ty) = Qwery::from(code as winit::ScanCode).into_char(shift) {
							for mut hook in &mut hooks.iter_mut().filter(|hook| hook.on_char) {
								(hook.function)(CallInfo {
									hook_id: hook.id,
									engine: keyboard.engine.clone(),
									combos: vec![vec![Qwery::from(code)]],
									ty: CallType::Press,
									elapsed: None,
									char_ty: Some(char_ty),
									first_call_this_press: true,
									hold_time: 0.0,
								});
							}
						}
					}
				}
			
				for mut hook in &mut hooks.iter_mut().filter(|hook| !hook.on_char) {
					let (active, active_combos) = active(&mut key_state, &hook.keys);
					let elapsed_ms = match &hook.last_call {
						&Some(ref last) => {
							let elapsed = last.elapsed();
							Some((elapsed.as_secs() * 1000) as f64 + (elapsed.subsec_nanos() as f64 / 1000000.0))
						}, &None => None
					};				
					
					let hold_time_ms = match &hook.press_start {
						&Some(ref start) => {
							let elapsed = start.elapsed();
							(elapsed.as_secs() * 1000) as f64 + (elapsed.subsec_nanos() as f64 / 1000000.0)
						}, &None => 0.0
					};
					
					let (call_func, call_ty) = if active && hook.active { // held
						(if hook.on_hold {
							match hook.frequency {
								Some(freq) => match &elapsed_ms {
									&Some(ref ms) => {
										if *ms >= freq as f64 {
											true
										} else {
											false
										}
									}, &None => true
								}, None => true
							}
						} else {
							false
						}, CallType::Hold)
					} else if active && !hook.active { // press
						hook.active = true;
						hook.first_call_this_press = true;
						hook.press_start = Some(Instant::now());
						(hook.on_press, CallType::Press)
					} else if !active && hook.active { // release
						hook.active = false;
						hook.press_start = None;
						(hook.on_release, CallType::Release)
					} else {
						(false, CallType::Unknown)
					};
					
					if call_func {
						hook.last_call = Some(Instant::now());
						let first_call_this_press = hook.first_call_this_press;
						hook.first_call_this_press = false;
						
						(hook.function)(CallInfo {
							hook_id: hook.id,
							engine: keyboard.engine.clone(),
							combos: active_combos,
							ty: call_ty,
							elapsed: elapsed_ms,
							char_ty: None,
							first_call_this_press: first_call_this_press,
							hold_time: hold_time_ms,
						});
					}
				}
				
				for barrier in delay_test_barriers {
					barrier.wait();
				}
				
				let iter_elapsed = iter_start.elapsed();
				let nanos = (iter_elapsed.as_secs() * 1000000000) + iter_elapsed.subsec_nanos() as u64;
				
				if nanos < 5000000 {
					thread::sleep(Duration::new(0, (5000000-nanos) as u32));
				} else {
					println!("[ENGINE]: Keyboard loop iteration taking more than 5ms!");
				}
			}
		});
		
		*keyboard.exec_thread.lock() = Some(handle);
		keyboard
	}
}

#[derive(Debug,Copy,Clone)]
pub enum Qwery {
	Esc, F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
	Tilda, One, Two, Three, Four, Five, Six, Seven, Eight, Nine, Zero, Dash, Equal, Backspace,
	Tab, Q, W, E, R, T, Y, U, I, O, P, LSqBracket, RSqBracket, Backslash,
	Caps, A, S, D, F, G, H, J, K, L, SemiColon, Parenthesis, Enter,
	LShift, Z, X, C, V, B, N, M, Comma, Period, Slash, RShift,
	LCtrl, LSuper, LAlt, Space, RAlt, RSuper, RCtrl,
	PrintScreen, ScrollLock, Pause,
	Insert, Home, PageUp,
	Delete, End, PageDown,
	ArrowUp, ArrowDown, ArrowLeft, ArrowRight
}

#[derive(Clone,Copy)]
pub enum CharType {
	Backspace,
	Letter(char),
}

impl Qwery {
	pub fn into_char(self, shift: bool) -> Option<CharType> {
		match shift {
			false => match self {
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
				Qwery::Tilda => Some(CharType::Letter('`')),
				Qwery::One => Some(CharType::Letter('1')),
				Qwery::Two => Some(CharType::Letter('2')),
				Qwery::Three => Some(CharType::Letter('3')),
				Qwery::Four => Some(CharType::Letter('4')),
				Qwery::Five => Some(CharType::Letter('5')),
				Qwery::Six => Some(CharType::Letter('6')),
				Qwery::Seven => Some(CharType::Letter('7')),
				Qwery::Eight => Some(CharType::Letter('8')),
				Qwery::Nine => Some(CharType::Letter('9')),
				Qwery::Zero => Some(CharType::Letter('0')),
				Qwery::Dash => Some(CharType::Letter('-')),
				Qwery::Equal => Some(CharType::Letter('=')),
				Qwery::Backspace => Some(CharType::Backspace),
				Qwery::Tab => None,
				Qwery::Q => Some(CharType::Letter('q')),
				Qwery::W => Some(CharType::Letter('w')),
				Qwery::E => Some(CharType::Letter('e')),
				Qwery::R => Some(CharType::Letter('r')),
				Qwery::T => Some(CharType::Letter('t')),
				Qwery::Y => Some(CharType::Letter('y')),
				Qwery::U => Some(CharType::Letter('u')),
				Qwery::I => Some(CharType::Letter('i')),
				Qwery::O => Some(CharType::Letter('o')),
				Qwery::P => Some(CharType::Letter('p')),
				Qwery::LSqBracket => Some(CharType::Letter('[')),
				Qwery::RSqBracket => Some(CharType::Letter(']')),
				Qwery::Backslash => Some(CharType::Letter('\\')),
				Qwery::Caps => None,
				Qwery::A => Some(CharType::Letter('a')),
				Qwery::S => Some(CharType::Letter('s')),
				Qwery::D => Some(CharType::Letter('d')),
				Qwery::F => Some(CharType::Letter('f')),
				Qwery::G => Some(CharType::Letter('g')),
				Qwery::H => Some(CharType::Letter('h')),
				Qwery::J => Some(CharType::Letter('j')),
				Qwery::K => Some(CharType::Letter('k')),
				Qwery::L => Some(CharType::Letter('l')),
				Qwery::SemiColon => Some(CharType::Letter(';')),
				Qwery::Parenthesis => Some(CharType::Letter('\'')),
				Qwery::Enter => Some(CharType::Letter('\n')),
				Qwery::LShift => None,
				Qwery::Z => Some(CharType::Letter('z')),
				Qwery::X => Some(CharType::Letter('x')),
				Qwery::C => Some(CharType::Letter('c')),
				Qwery::V => Some(CharType::Letter('v')),
				Qwery::B => Some(CharType::Letter('b')),
				Qwery::N => Some(CharType::Letter('n')),
				Qwery::M => Some(CharType::Letter('m')),
				Qwery::Comma => Some(CharType::Letter(',')),
				Qwery::Period => Some(CharType::Letter('.')),
				Qwery::Slash => Some(CharType::Letter('/')),
				Qwery::RShift => None,
				Qwery::LCtrl => None,
				Qwery::LSuper => None,
				Qwery::LAlt => None,
				Qwery::Space => Some(CharType::Letter(' ')),
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
				Qwery::ArrowRight => None
			}, true => match self {
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
				Qwery::Tilda => Some(CharType::Letter('~')),
				Qwery::One => Some(CharType::Letter('!')),
				Qwery::Two => Some(CharType::Letter('@')),
				Qwery::Three => Some(CharType::Letter('#')),
				Qwery::Four => Some(CharType::Letter('$')),
				Qwery::Five => Some(CharType::Letter('%')),
				Qwery::Six => Some(CharType::Letter('^')),
				Qwery::Seven => Some(CharType::Letter('&')),
				Qwery::Eight => Some(CharType::Letter('*')),
				Qwery::Nine => Some(CharType::Letter('(')),
				Qwery::Zero => Some(CharType::Letter(')')),
				Qwery::Dash => Some(CharType::Letter('_')),
				Qwery::Equal => Some(CharType::Letter('+')),
				Qwery::Backspace => Some(CharType::Backspace),
				Qwery::Tab => None,
				Qwery::Q => Some(CharType::Letter('Q')),
				Qwery::W => Some(CharType::Letter('W')),
				Qwery::E => Some(CharType::Letter('E')),
				Qwery::R => Some(CharType::Letter('R')),
				Qwery::T => Some(CharType::Letter('T')),
				Qwery::Y => Some(CharType::Letter('Y')),
				Qwery::U => Some(CharType::Letter('U')),
				Qwery::I => Some(CharType::Letter('I')),
				Qwery::O => Some(CharType::Letter('O')),
				Qwery::P => Some(CharType::Letter('P')),
				Qwery::LSqBracket => Some(CharType::Letter('{')),
				Qwery::RSqBracket => Some(CharType::Letter('}')),
				Qwery::Backslash => Some(CharType::Letter('|')),
				Qwery::Caps => None,
				Qwery::A => Some(CharType::Letter('A')),
				Qwery::S => Some(CharType::Letter('S')),
				Qwery::D => Some(CharType::Letter('D')),
				Qwery::F => Some(CharType::Letter('F')),
				Qwery::G => Some(CharType::Letter('G')),
				Qwery::H => Some(CharType::Letter('H')),
				Qwery::J => Some(CharType::Letter('J')),
				Qwery::K => Some(CharType::Letter('K')),
				Qwery::L => Some(CharType::Letter('L')),
				Qwery::SemiColon => Some(CharType::Letter(':')),
				Qwery::Parenthesis => Some(CharType::Letter('"')),
				Qwery::Enter => Some(CharType::Letter('\n')),
				Qwery::LShift => None,
				Qwery::Z => Some(CharType::Letter('Z')),
				Qwery::X => Some(CharType::Letter('X')),
				Qwery::C => Some(CharType::Letter('C')),
				Qwery::V => Some(CharType::Letter('V')),
				Qwery::B => Some(CharType::Letter('B')),
				Qwery::N => Some(CharType::Letter('N')),
				Qwery::M => Some(CharType::Letter('M')),
				Qwery::Comma => Some(CharType::Letter('<')),
				Qwery::Period => Some(CharType::Letter('>')),
				Qwery::Slash => Some(CharType::Letter('?')),
				Qwery::RShift => None,
				Qwery::LCtrl => None,
				Qwery::LSuper => None,
				Qwery::LAlt => None,
				Qwery::Space => Some(CharType::Letter(' ')),
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
				Qwery::ArrowRight => None
			}
		}
	}
}

impl Into<winit::ScanCode> for Qwery {
	fn into(self) -> winit::ScanCode {
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
						_ => unreachable!()
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
						_ => unreachable!()
					}	
				}
			}
		}
	}
}

// TODO: Replace with try_from when that is stable
impl From<winit::ScanCode> for Qwery {
	fn from(code: winit::ScanCode) -> Qwery {
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
						}
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
						}
					}
				}
			}
		}
	}
}

