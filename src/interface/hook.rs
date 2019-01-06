#![allow(warnings)]

use keyboard::{self,Qwery};
use mouse;
use std::time::Instant;
use std::time::Duration;
use std::sync::Arc;
use std::collections::BTreeMap;
use std::collections::HashMap;
use parking_lot::Mutex;
use interface::bin::Bin;
use parking_lot::RwLock;
use Engine;
use std::sync::Weak;
use crossbeam::queue::MsQueue;

pub type BinHookFn = Arc<Fn(Arc<Bin>, &BinHookData) + Send + Sync>;

#[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord,Hash)]
pub struct BinHookID(u64);

pub enum BinHookTy {
	Press,
	Hold,
	Release,
	Character,
	MouseEnter,
	MouseLeave,
	MouseMove,
	MouseScroll,
	Focused,
	LostFocus,
}

pub enum BinHook {
	Press {
		keys: Vec<Qwery>,
		mouse_buttons: Vec<mouse::Button>,
	},
	
	Hold {
		keys: Vec<Qwery>,
		mouse_buttons: Vec<mouse::Button>,
		initial_delay: Duration,
		interval: Duration,
		accel: f32,
	},
	
	Release {
		keys: Vec<Qwery>,
		mouse_buttons: Vec<mouse::Button>,
	},
	
	Character,
	MouseEnter,
	MouseLeave,
	MouseMove,
	MouseScroll,
	Focused,
	LostFocus,
}

impl BinHook {
	fn into_data(self) -> BinHookData {
		match self {
			BinHook::Press {
				keys,
				mouse_buttons
			} => {
				let mut key_active = HashMap::new();
				let mut mouse_active = HashMap::new();
				
				for key in keys {
					key_active.insert(key, false);
				}
				
				for button in mouse_buttons {
					mouse_active.insert(button, false);
				}
				
				BinHookData::Press {
					key_active,
					mouse_active,
				}
			},
			
			BinHook::Hold {
				keys,
				mouse_buttons,
				initial_delay,
				interval,
				accel,
			} => {
				let mut key_active = HashMap::new();
				let mut mouse_active = HashMap::new();
				
				for key in keys {
					key_active.insert(key, false);
				}
				
				for button in mouse_buttons {
					mouse_active.insert(button, false);
				}
				
				BinHookData::Hold {
					key_active,
					mouse_active,
					first_call: Instant::now(),
					last_call: Instant::now(),
					is_first_call: true,
					initial_delay,
					initial_delay_wait: true,
					initial_delay_elapsed: false,
					interval,
					accel,
				}
			},
			
			BinHook::Release {
				keys,
				mouse_buttons
			} => {
				let mut key_active = HashMap::new();
				let mut mouse_active = HashMap::new();
				
				for key in keys {
					key_active.insert(key, false);
				}
				
				for button in mouse_buttons {
					mouse_active.insert(button, false);
				}
				
				BinHookData::Release {
					key_active,
					mouse_active,
					pressed: false,
				}
			},
			
			BinHook::Character => BinHookData::Character {
				char_ty: keyboard::CharType::Letter(' '),
			},
			
			BinHook::MouseEnter => BinHookData::MouseEnter {
				mouse_x: 0.0,
				mouse_y: 0.0,
			},
			
			BinHook::MouseLeave => BinHookData::MouseLeave {
				mouse_x: 0.0,
				mouse_y: 0.0,
			},
			
			BinHook::MouseMove => BinHookData::MouseMove {
				mouse_x: 0.0,
				mouse_y: 0.0,
				mouse_dx: 0.0,
				mouse_dy: 0.0,
			},
			
			BinHook::MouseScroll => BinHookData::MouseScroll {
				scroll_amt: 0.0,
			},
			
			BinHook::Focused => BinHookData::Focused,
			BinHook::LostFocus => BinHookData::LostFocus,
		}
	}
}

pub enum BinHookData {
	Press {
		key_active: HashMap<Qwery, bool>,
		mouse_active: HashMap<mouse::Button, bool>,
	},
	
	Hold {
		first_call: Instant,
		last_call: Instant,
		is_first_call: bool,
		initial_delay: Duration,
		initial_delay_wait: bool,
		initial_delay_elapsed: bool,
		interval: Duration,
		accel: f32,
		key_active: HashMap<Qwery, bool>,
		mouse_active: HashMap<mouse::Button, bool>,
	},
	
	Release {
		pressed: bool,
		key_active: HashMap<Qwery, bool>,
		mouse_active: HashMap<mouse::Button, bool>,
	},
	
	Character {
		char_ty: keyboard::CharType,
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

pub(crate) enum InputEvent {	
	MousePress(mouse::Button),
	MouseRelease(mouse::Button),
	KeyPress(Qwery),
	KeyRelease(Qwery),
	MousePosition(f32, f32),
	MouseDelta(f32, f32),
	Scroll(f32),
}

impl BinHookData {
	pub fn ty(&self) -> BinHookTy {
		match self {
			BinHookData::Press { .. } => BinHookTy::Press,
			BinHookData::Hold { .. } => BinHookTy::Hold,
			BinHookData::Release { .. } => BinHookTy::Release,
			BinHookData::Character { .. } => BinHookTy::Character,
			BinHookData::MouseEnter { .. } => BinHookTy::MouseEnter,
			BinHookData::MouseLeave { .. } => BinHookTy::MouseLeave,
			BinHookData::MouseMove { .. } => BinHookTy::MouseMove,
			BinHookData::MouseScroll { .. } => BinHookTy::MouseScroll,
			BinHookData::Focused => BinHookTy::Focused,
			BinHookData::LostFocus => BinHookTy::LostFocus,
		}
	}

	fn is_active(&self) -> bool {
		match match self {
			BinHookData::Press { key_active, mouse_active, .. } => Some((key_active, mouse_active)),
			BinHookData::Release { key_active, mouse_active, .. } => Some((key_active, mouse_active)),
			BinHookData::Hold { key_active, mouse_active, .. } => Some((key_active, mouse_active)),
			_ => None
		} {
			Some((key_active, mouse_active)) => {
				for (_, v) in key_active {
					if *v == false {
						return false;
					}
				}
				
				for (_, v) in mouse_active {
					if *v == false {
						return false;
					}
				}
				
				true
			}, None => true
		}
	}
}

pub(crate) struct HookManager {
	focused: Mutex<Option<u64>>,
	hooks: Mutex<BTreeMap<BinHookID, (Arc<Bin>, BinHookData, BinHookFn)>>,
	current_id: Mutex<u64>,
	engine: Arc<Engine>,
	bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>,
	events: MsQueue<InputEvent>,
}

impl HookManager {
	pub fn send_event(&self, event: InputEvent) {
		self.events.push(event);
	}
	
	pub fn add_hook(&self, bin: Arc<Bin>, hook: BinHook, func: BinHookFn) -> BinHookID {
		let mut current_id = self.current_id.lock();
		let id = BinHookID(*current_id);
		*current_id += 1;
		drop(current_id);
		self.hooks.lock().insert(id, (bin, hook.into_data(), func));
		id
	}

	pub fn new(engine: Arc<Engine>, bin_map: Arc<RwLock<BTreeMap<u64, Weak<Bin>>>>) -> Arc<Self> {
		let hman_ret = Arc::new(HookManager {
			focused: Mutex::new(None),
			hooks: Mutex::new(BTreeMap::new()),
			current_id: Mutex::new(0),
			engine,
			bin_map,
			events: MsQueue::new(),
		});
	
		/*
			Press: Mouse(X), Key(X)
			Hold: Mouse(X), Key(X)
			Release: Mouse(X), Key(X)
			Character(-)
			MouseEnter(-)
			MouseLeave(-)
			MouseMove(-)
			MouseScroll(-)
			Focused(X)
			LostFocus(X)
		*/
		
		let hman = hman_ret.clone();
		
		::std::thread::spawn(move || {
			let mut last_tick = Instant::now();
			let tick_interval = Duration::from_millis(5);
			let mut m_window_x = 0.0;
			let mut m_window_y = 0.0;
			let mut m_delta_x = 0.0;
			let mut m_delta_y = 0.0;
			let mut m_moved = false;
			let mut key_state = HashMap::new();
			let mut mouse_state = HashMap::new();
			
			loop {
				let mut focused = hman.focused.lock();
				let mut hooks = hman.hooks.lock();
				let mut scroll_amt = 0.0;
				let mut events = Vec::new();
			
				while let Some(event) = hman.events.try_pop() {
					match event {
						InputEvent::MousePosition(x, y) => {
							m_window_x = x;
							m_window_y = y;
						}, InputEvent::MouseDelta(x, y) => {
							m_delta_x += x;
							m_delta_y += y;
							m_moved = true;
						}, InputEvent::Scroll(y) => {
							scroll_amt += y;
						}, InputEvent::MousePress(button) => {
							let mut modified = false;
						
							mouse_state.entry(button.clone()).and_modify(|v: &mut bool| if !*v {
								*v = true;
								modified = true;
							}).or_insert_with(|| {
								modified = true;
								true
							});
							
							if modified {
								events.push(InputEvent::MousePress(button));
							}
						}, InputEvent::MouseRelease(button) => {
							let mut modified = false;
						
							mouse_state.entry(button.clone()).and_modify(|v: &mut bool| if *v {
								*v = false;
								modified = true;
							}).or_insert_with(|| {
								modified = true;
								false
							});
							
							if modified {
								events.push(InputEvent::MouseRelease(button));
							}
						}, InputEvent::KeyPress(key) => {
							let mut modified = false;
						
							key_state.entry(key.clone()).and_modify(|v: &mut bool| if !*v {
								*v = true;
								modified = true;
							}).or_insert_with(|| {
								modified = true;
								true
							});
							
							if modified {
								events.push(InputEvent::KeyPress(key));
							}
						}, InputEvent::KeyRelease(key) => {
							let mut modified = false;
						
							key_state.entry(key.clone()).and_modify(|v: &mut bool| if *v {
								*v = false;
								modified = true;
							}).or_insert_with(|| {
								modified = true;
								false
							});
							
							if modified {
								events.push(InputEvent::KeyRelease(key));
							}
						},
						
						e => events.push(e)
					}
				}
				
				for event in events {
					match event {
						InputEvent::MousePress(button) => {
							let mut top_bin_op = hman.engine.interface_ref().get_bin_atop(m_window_x, m_window_y);
							
							if top_bin_op.as_ref().map(|v| v.id()) != *focused {
								if let Some(bin_id) = &*focused {
									for (_, (hb, hook, func)) in &mut *hooks {
										if hb.id() == *bin_id {
											match hook.ty() {
												BinHookTy::LostFocus => {
													func(hb.clone(), hook);
												},
												
												BinHookTy::Press => {
													if let BinHookData::Press {
														key_active,
														mouse_active,
														..
													} = hook {
														for (_, v) in key_active {
															*v = false;
														}
														
														for (_, v) in mouse_active {
															*v = false;
														}
													}
												},
												
												BinHookTy::Hold => {
													if let BinHookData::Hold {
														key_active,
														mouse_active,
														is_first_call,
														initial_delay_wait,
														initial_delay_elapsed,
														..
													} = hook {
														for (_, v) in key_active {
															*v = false;
														}
														
														for (_, v) in mouse_active {
															*v = false;
														}
														
														*is_first_call = true;
														*initial_delay_wait = true;
														*initial_delay_elapsed = false;
													}
												},
												
												BinHookTy::Release => {
													let mut call = false;
												
													if let BinHookData::Release {
														key_active,
														mouse_active,
														pressed,
														..
													} = hook {
														call = *pressed;
														
														for (_, v) in key_active {
															*v = false;
														}
														
														for (_, v) in mouse_active {
															*v = false;
														}
													}
													
													if call {
														func(hb.clone(), hook);
													}
												},
												
												_ => ()
											}
										}
									}
								}
								
								*focused = top_bin_op.map(|v| v.id());
								
								if let Some(bin_id) = &*focused {
									for (_, (hb, hook, func)) in &mut *hooks {
										if hb.id() == *bin_id {
											match hook {
												BinHookData::Focused => func(hb.clone(), hook), // Call Focused
												_ => ()
											}
										}
									}
								}
							}
							
							if let Some(bin_id) = &*focused {
								for (hook_id, (hb, hook, func)) in &mut *hooks {
									if hb.id() == *bin_id {
										match hook.ty() {
											BinHookTy::Press => {
												let mut check = false;
												
												if let BinHookData::Press { mouse_active, .. } = hook {
													if let Some(v) = mouse_active.get_mut(&button) {
														if !*v {
															*v = true;
															check = true;
														}
													}
												}
												
												if check && hook.is_active() {
													func(hb.clone(), hook); // Call Press
												}
											},
											
											BinHookTy::Hold => {
												let mut check = false;
												
												if let BinHookData::Hold { mouse_active, .. } = hook {
													if let Some(v) = mouse_active.get_mut(&button) {
														if !*v {
															*v = true;
															check = true;
														}
													}
												}
												
												if check && hook.is_active() {
													if let BinHookData::Hold { first_call, .. } = hook {
														*first_call = Instant::now();
													}
												}
											},
											
											BinHookTy::Release => {
												let mut check = false;
												
												if let BinHookData::Release { mouse_active, .. } = hook {
													if let Some(v) = mouse_active.get_mut(&button) {
														if !*v {
															*v = true;
															check = true;
														}
													}
												}
												
												if check && hook.is_active() {
													if let BinHookData::Release { pressed, .. } = hook {
														*pressed = true;
													}
												}
											},
											
											_ => ()
										}
									}
								}
							}
						},
						
						InputEvent::MouseRelease(button) => {
							if let Some(bin_id) = &*focused {
								for (_, (hb, hook, func)) in &mut *hooks {
									if hb.id() == *bin_id {
										match hook.ty() {
											BinHookTy::Press => {
												if let BinHookData::Press { mouse_active, .. } = hook {
													if let Some(v) = mouse_active.get_mut(&button) {
														*v = false;
													}
												}
											},
											
											BinHookTy::Hold => {
												if let BinHookData::Hold {
													mouse_active,
													is_first_call,
													initial_delay_wait,
													initial_delay_elapsed,
													.. 
												} = hook {
													if let Some(v) = mouse_active.get_mut(&button) {
														if *v {
															*v = false;
															*is_first_call = true;
															*initial_delay_wait = true;
															*initial_delay_elapsed = false;
														}
													}
												}
											},
											
											BinHookTy::Release => {
												let mut check = false;
												
												if let BinHookData::Release { mouse_active, .. } = hook {
													if let Some(v) = mouse_active.get_mut(&button) {
														if *v {
															*v = false;
															check = true;
														}
													}
												}
												
												if check && !hook.is_active() {
													let mut call = false;
													
													if let BinHookData::Release { pressed, .. } = hook {
														if *pressed {
															*pressed = false;
															call = true;
														}
													}
													
													if call {
														func(hb.clone(), hook); // Call Release
													}
												}
											},
											
											_ => ()
										}
									}
								}
							}
						},
						
						InputEvent::KeyPress(key) => {
							if let Some(bin_id) = &*focused {
								for (hook_id, (hb, hook, func)) in &mut *hooks {
									if hb.id() == *bin_id {
										match hook.ty() {
											BinHookTy::Press => {
												let mut check = false;
												
												if let BinHookData::Press { key_active, .. } = hook {
													if let Some(v) = key_active.get_mut(&key) {
														if !*v {
															*v = true;
															check = true;
														}
													}
												}
												
												if check && hook.is_active() {
													func(hb.clone(), hook); // Call Press
												}
											},
											
											BinHookTy::Hold => {
												let mut check = false;
												
												if let BinHookData::Hold { key_active, .. } = hook {
													if let Some(v) = key_active.get_mut(&key) {
														if !*v {
															*v = true;
															check = true;
														}
													}
												}
												
												if check && hook.is_active() {
													if let BinHookData::Hold { first_call, .. } = hook {
														*first_call = Instant::now();
													}
												}
											},
											
											BinHookTy::Release => {
												let mut check = false;
												
												if let BinHookData::Release { key_active, .. } = hook {
													if let Some(v) = key_active.get_mut(&key) {
														if !*v {
															*v = true;
															check = true;
														}
													}
												}
												
												if check && hook.is_active() {
													if let BinHookData::Release { pressed, .. } = hook {
														*pressed = true;
													}
												}
											},
											
											_ => ()
										}
									}
								}
							}
						},
						
						InputEvent::KeyRelease(key) => {
							if let Some(bin_id) = &*focused {
								for (_, (hb, hook, func)) in &mut *hooks {
									if hb.id() == *bin_id {
										match hook.ty() {
											BinHookTy::Press => {
												if let BinHookData::Press { key_active, .. } = hook {
													if let Some(v) = key_active.get_mut(&key) {
														*v = false;
													}
												}
											},
											
											BinHookTy::Hold => {
												if let BinHookData::Hold {
													key_active,
													is_first_call,
													initial_delay_wait,
													initial_delay_elapsed,
													.. 
												} = hook {
													if let Some(v) = key_active.get_mut(&key) {
														if *v {
															*v = false;
															*is_first_call = true;
															*initial_delay_wait = true;
															*initial_delay_elapsed = false;
														}
													}
												}
											},
											
											BinHookTy::Release => {
												let mut check = false;
												
												if let BinHookData::Release { key_active, .. } = hook {
													if let Some(v) = key_active.get_mut(&key) {
														if *v {
															*v = false;
															check = true;
														}
													}
												}
												
												if check && !hook.is_active() {
													let mut call = false;
													
													if let BinHookData::Release { pressed, .. } = hook {
														if *pressed {
															*pressed = false;
															call = true;
														}
													}
													
													if call {
														func(hb.clone(), hook); // Call Release
													}
												}
											},
											
											_ => ()
										}
									}
								}
							}
						},
						
						_ => ()
					}
				}
				
				if let Some(bin_id) = &*focused {
					for (_, (hb, hook, func)) in &mut *hooks {
						if hb.id() == *bin_id {
							if let BinHookData::Hold { .. } = hook {
								if !hook.is_active() {
									continue;
								}
							}
						
							if match hook {
								BinHookData::Hold {
									first_call,
									last_call,
									is_first_call,
									interval,
									initial_delay,
									initial_delay_wait,
									initial_delay_elapsed,
									..
								} => {
									if *is_first_call {
										if *initial_delay_wait {
											if first_call.elapsed() < *initial_delay {
												continue;
											} else {
												*initial_delay_wait = false;
												*initial_delay_elapsed = true;
												*first_call = Instant::now();
												*is_first_call = false;
											}
										} else if !*initial_delay_elapsed {
											*initial_delay_wait = true;
											*first_call = Instant::now();
											continue;
										}
									} else if last_call.elapsed() < *interval {
										continue;
									}
									
									true
								},
								
								_ => false
							} {
								func(hb.clone(), hook); // Call Hold
								
								if let BinHookData::Hold { last_call, .. } = &mut *hook {
									*last_call = Instant::now();
								}
							}
						}
					}
				}
				
				let elapsed = last_tick.elapsed();
				
				if elapsed < tick_interval {
					::std::thread::sleep(tick_interval - elapsed);
				}
			}
		});	
			
		hman_ret
	}
}

