use keyboard::{self,Qwery};
use mouse;
use std::time::Instant;
use std::time::Duration;
use std::sync::Arc;

pub type BinHookFn = Arc<Fn(&BinHook) + Send + Sync>;

pub enum BinHook {
	Press {
		keys: Vec<Qwery>,
		mouse: Vec<mouse::Button>,
	},
	
	Hold {
		keys: Vec<Qwery>,
		mouse: Vec<mouse::Button>,
		first_call: Instant,
		last_call: Instant,
		is_first_call: bool,
		inital_delay: Duration,
		interval: Duration,
		accel: bool,
		accel_rate: f32,
	},
	
	Release {
		keys: Vec<Qwery>,
		mouse: Vec<mouse::Button>,
	},
	
	Character {
		char_ty: Option<keyboard::CharType>,
	},
	
	MouseEnter {
		mouse_x: f32,
		mouse_y: f32,
	},
	
	MouseLeave {
		mouse_x: f32,
		mouse_y: f32,
	},
	
	MouseMove {
		mouse_x: f32,
		mouse_y: f32,
		mouse_dx: f32,
		mouse_dy: f32,
	},
	
	MouseScroll {
		scroll_amt: f32,
	},
	
	Focused,
	LostFocus,
}
