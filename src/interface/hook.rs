use crate::input::*;
use crate::interface::bin::{Bin, BinID};
use crate::interface::Interface;
use crossbeam::channel::{self, Sender};
use parking_lot::{Condvar, Mutex};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{self, AtomicU64};
use std::sync::{Arc, Weak};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BinHookID(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

#[derive(Clone, Debug, PartialEq)]
pub enum BinHook {
	Press {
		keys: Vec<Qwerty>,
		mouse_buttons: Vec<MouseButton>,
	},

	Hold {
		keys: Vec<Qwerty>,
		mouse_buttons: Vec<MouseButton>,
		initial_delay: Duration,
		interval: Duration,
		accel: f32,
	},

	Release {
		keys: Vec<Qwerty>,
		mouse_buttons: Vec<MouseButton>,
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
				mouse_buttons,
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
					mouse_x: 0.0,
					mouse_y: 0.0,
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
					mouse_x: 0.0,
					mouse_y: 0.0,
				}
			},

			BinHook::Release {
				keys,
				mouse_buttons,
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

			BinHook::Character => BinHookData::Character(' '),

			BinHook::MouseEnter =>
				BinHookData::MouseEnter {
					mouse_x: 0.0,
					mouse_y: 0.0,
				},

			BinHook::MouseLeave =>
				BinHookData::MouseLeave {
					mouse_x: 0.0,
					mouse_y: 0.0,
				},

			BinHook::MouseMove =>
				BinHookData::MouseMove {
					mouse_x: 0.0,
					mouse_y: 0.0,
					mouse_dx: 0.0,
					mouse_dy: 0.0,
				},

			BinHook::MouseScroll =>
				BinHookData::MouseScroll {
					scroll_amt: 0.0,
				},

			BinHook::Focused => BinHookData::Focused,
			BinHook::LostFocus => BinHookData::LostFocus,
		}
	}
}

#[derive(Clone, Debug)]
pub enum BinHookData {
	Press {
		mouse_x: f32,
		mouse_y: f32,
		key_active: HashMap<Qwerty, bool>,
		mouse_active: HashMap<MouseButton, bool>,
	},

	Hold {
		mouse_x: f32,
		mouse_y: f32,
		first_call: Instant,
		last_call: Instant,
		is_first_call: bool,
		initial_delay: Duration,
		initial_delay_wait: bool,
		initial_delay_elapsed: bool,
		interval: Duration,
		accel: f32,
		key_active: HashMap<Qwerty, bool>,
		mouse_active: HashMap<MouseButton, bool>,
	},

	Release {
		pressed: bool,
		key_active: HashMap<Qwerty, bool>,
		mouse_active: HashMap<MouseButton, bool>,
	},

	Character(char),

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

pub(crate) enum BinHookEvent {
	MousePress(MouseButton),
	MouseRelease(MouseButton),
	KeyPress(Qwerty),
	KeyRelease(Qwerty),
	MousePosition(f32, f32),
	MouseDelta(f32, f32),
	Scroll(f32),
	SetScrollProps(ScrollProps),
	Character(char),
}

impl BinHookData {
	pub fn ty(&self) -> BinHookTy {
		match self {
			BinHookData::Press {
				..
			} => BinHookTy::Press,
			BinHookData::Hold {
				..
			} => BinHookTy::Hold,
			BinHookData::Release {
				..
			} => BinHookTy::Release,
			BinHookData::Character {
				..
			} => BinHookTy::Character,
			BinHookData::MouseEnter {
				..
			} => BinHookTy::MouseEnter,
			BinHookData::MouseLeave {
				..
			} => BinHookTy::MouseLeave,
			BinHookData::MouseMove {
				..
			} => BinHookTy::MouseMove,
			BinHookData::MouseScroll {
				..
			} => BinHookTy::MouseScroll,
			BinHookData::Focused => BinHookTy::Focused,
			BinHookData::LostFocus => BinHookTy::LostFocus,
		}
	}

	fn is_active(&self) -> bool {
		match match self {
			BinHookData::Press {
				key_active,
				mouse_active,
				..
			} => Some((key_active, mouse_active)),
			BinHookData::Release {
				key_active,
				mouse_active,
				..
			} => Some((key_active, mouse_active)),
			BinHookData::Hold {
				key_active,
				mouse_active,
				..
			} => Some((key_active, mouse_active)),
			_ => None,
		} {
			Some((key_active, mouse_active)) => {
				for v in key_active.values() {
					if !(*v) {
						return false;
					}
				}

				for v in mouse_active.values() {
					if !(*v) {
						return false;
					}
				}

				true
			},
			None => true,
		}
	}
}

#[derive(Default)]
struct SmoothScroll {
	to: f32,
	at: f32,
}

#[derive(Clone, Debug)]
pub struct ScrollProps {
	pub smooth: bool,
	pub accel: bool,
	pub step_mult: f32,
	pub accel_factor: f32,
}

impl Default for ScrollProps {
	fn default() -> ScrollProps {
		ScrollProps {
			smooth: true,
			accel: true,
			step_mult: 2.5,
			accel_factor: 5.0,
		}
	}
}

pub(crate) struct HookManager {
	current_id: AtomicU64,
	events: Sender<BinHookEvent>,
	remove: Sender<BinHookID>,
	add: Sender<(
		BinHookID,
		(Weak<Bin>, BinHookData, Box<dyn FnMut(Arc<Bin>, &BinHookData) + Send + 'static>),
	)>,
}

impl HookManager {
	pub fn send_event(&self, event: BinHookEvent) {
		self.events.send(event).unwrap();
	}

	pub fn remove_hook(&self, hook_id: BinHookID) {
		self.remove.send(hook_id).unwrap();
	}

	#[inline]
	pub fn remove_hooks(&self, hook_ids: Vec<BinHookID>) {
		for hook_id in hook_ids {
			self.remove_hook(hook_id);
		}
	}

	pub fn add_hook(
		&self,
		bin: Arc<Bin>,
		hook: BinHook,
		func: Box<dyn FnMut(Arc<Bin>, &BinHookData) + Send + 'static>,
	) -> BinHookID {
		let current_id = self.current_id.fetch_add(1, atomic::Ordering::SeqCst);
		let id = BinHookID(current_id);
		self.add.send((id, (Arc::downgrade(&bin), hook.into_data(), func))).unwrap();
		id
	}

	pub fn new() -> (Self, Arc<Mutex<Option<Arc<Interface>>>>, Arc<Condvar>) {
		let (events_s, events_r) = channel::unbounded();
		let (remove_s, remove_r) = channel::unbounded();
		let (add_s, add_r) = channel::unbounded();

		let hman = HookManager {
			current_id: AtomicU64::default(),
			events: events_s,
			remove: remove_s,
			add: add_s,
		};

		let itf_op = Arc::new(Mutex::new(None));
		let itf_cond = Arc::new(Condvar::new());
		let itf_op_ret = itf_op.clone();
		let itf_cond_ret = itf_cond.clone();

		thread::spawn(move || {
			let interface: Arc<Interface> = {
				let mut interface_op = itf_op.lock();
				itf_cond.wait(&mut interface_op);
				interface_op.take().unwrap()
			};

			let mut last_tick = Instant::now();
			let tick_interval = Duration::from_millis(5);
			let mut m_window_x = 0.0;
			let mut m_window_y = 0.0;
			let mut m_delta_x = 0.0;
			let mut m_delta_y = 0.0;
			let mut m_moved = false;
			let mut key_state = HashMap::new();
			let mut mouse_state = HashMap::new();
			let mut smooth_scroll = SmoothScroll::default();
			let mut mouse_in: HashMap<BinID, Weak<Bin>> = HashMap::new();
			let mut scroll_props = ScrollProps::default();
			let mut focused: Option<BinID> = None;

			let mut hooks: BTreeMap<
				BinHookID,
				(
					Weak<Bin>,
					BinHookData,
					Box<dyn FnMut(Arc<Bin>, &BinHookData) + Send + 'static>,
				),
			> = BTreeMap::new();

			loop {
				let mut m_scroll_amt = 0.0;
				let mut events = Vec::new();
				let mut bad_hooks = Vec::new();

				while let Ok(hook_id) = remove_r.try_recv() {
					hooks.remove(&hook_id);
				}

				while let Ok((k, v)) = add_r.try_recv() {
					hooks.insert(k, v);
				}

				while let Ok(event) = events_r.try_recv() {
					match event {
						BinHookEvent::SetScrollProps(props) => {
							scroll_props = props;
						},
						BinHookEvent::MousePosition(x, y) => {
							m_window_x = x;
							m_window_y = y;
						},
						BinHookEvent::MouseDelta(x, y) => {
							m_delta_x += x;
							m_delta_y += y;
							m_moved = true;
						},
						BinHookEvent::Scroll(y) => {
							m_scroll_amt += y;
						},
						BinHookEvent::MousePress(button) => {
							let mut modified = false;

							mouse_state
								.entry(button)
								.and_modify(|v: &mut bool| {
									if !*v {
										*v = true;
										modified = true;
									}
								})
								.or_insert_with(|| {
									modified = true;
									true
								});

							if modified {
								events.push(BinHookEvent::MousePress(button));
							}
						},
						BinHookEvent::MouseRelease(button) => {
							let mut modified = false;

							mouse_state
								.entry(button)
								.and_modify(|v: &mut bool| {
									if *v {
										*v = false;
										modified = true;
									}
								})
								.or_insert_with(|| {
									modified = true;
									false
								});

							if modified {
								events.push(BinHookEvent::MouseRelease(button));
							}
						},
						BinHookEvent::KeyPress(key) => {
							let mut modified = false;

							key_state
								.entry(key)
								.and_modify(|v: &mut u16| {
									if *v == 0 {
										*v = 1;
										modified = true;
									}
								})
								.or_insert_with(|| {
									modified = true;
									1
								});

							if modified {
								events.push(BinHookEvent::KeyPress(key));
							}
						},
						BinHookEvent::KeyRelease(key) => {
							let mut modified = false;

							key_state
								.entry(key)
								.and_modify(|v: &mut u16| {
									if *v > 0 {
										*v = 0;
										modified = true;
									}
								})
								.or_insert_with(|| {
									modified = true;
									0
								});

							if modified {
								events.push(BinHookEvent::KeyRelease(key));
							}
						},
						BinHookEvent::Character(c) => events.push(BinHookEvent::Character(c)),
					}
				}

				if m_moved {
					let mut in_bins = Vec::new();

					if let Some(top_bin) = interface.get_bin_atop(m_window_x, m_window_y) {
						in_bins.push(top_bin.clone());
						in_bins.append(&mut top_bin.ancestors());

						for bin in &in_bins {
							mouse_in.entry(bin.id()).or_insert_with(|| {
								for (hook_id, (hb_wk, hook, func)) in hooks.iter_mut() {
									let hb = match hb_wk.upgrade() {
										Some(some) => some,
										None => {
											bad_hooks.push(*hook_id);
											continue;
										},
									};

									if bin.id() == hb.id() && hook.ty() == BinHookTy::MouseEnter
									{
										if let BinHookData::MouseEnter {
											mouse_x,
											mouse_y,
										} = hook
										{
											*mouse_x = m_window_x;
											*mouse_y = m_window_y;
										}

										func(hb.clone(), hook); // Call MouseEnter
									}
								}

								Arc::downgrade(bin)
							});
						}
					}

					for (hook_id, (hb_wk, hook, func)) in hooks.iter_mut() {
						if hook.ty() == BinHookTy::MouseMove {
							let hb = match hb_wk.upgrade() {
								Some(some) => some,
								None => {
									bad_hooks.push(*hook_id);
									continue;
								},
							};

							if mouse_in.contains_key(&hb.id()) {
								if let BinHookData::MouseMove {
									mouse_x,
									mouse_y,
									mouse_dx,
									mouse_dy,
								} = hook
								{
									*mouse_x = m_window_x;
									*mouse_y = m_window_y;
									*mouse_dx = m_delta_x;
									*mouse_dy = m_delta_y;
								}

								func(hb.clone(), hook); // Call MouseMove
							}
						}
					}

					let keys: Vec<BinID> = mouse_in.keys().cloned().collect();

					for bin_id in keys {
						if !in_bins.iter().any(|b| b.id() == bin_id)
							&& mouse_in.remove(&bin_id).is_some()
						{
							for (hook_id, (hb_wk, hook, func)) in hooks.iter_mut() {
								let hb = match hb_wk.upgrade() {
									Some(some) => some,
									None => {
										bad_hooks.push(*hook_id);
										continue;
									},
								};

								if hb.id() == bin_id && hook.ty() == BinHookTy::MouseLeave {
									if let BinHookData::MouseLeave {
										mouse_x,
										mouse_y,
										..
									} = hook
									{
										*mouse_x = m_window_x;
										*mouse_y = m_window_y;
									}

									func(hb.clone(), hook); // Call MouseLeave
								}
							}
						}
					}
				}

				if scroll_props.smooth {
					if m_scroll_amt != 0.0 {
						if scroll_props.accel {
							smooth_scroll.to += m_scroll_amt
								* scroll_props.step_mult * ((smooth_scroll.to)
								.abs()
								+ scroll_props.accel_factor)
								.log(scroll_props.accel_factor);
						} else {
							smooth_scroll.to += m_scroll_amt * scroll_props.step_mult;
						}
					}

					m_scroll_amt = 0.0;

					if smooth_scroll.at != 0.0 || smooth_scroll.to != 0.0 {
						if smooth_scroll.at == smooth_scroll.to {
							smooth_scroll.at = 0.0;
							smooth_scroll.to = 0.0;
						} else {
							let diff = smooth_scroll.to - smooth_scroll.at;
							let step = diff * 0.175;

							let amt = if f32::abs(step) < 0.005 {
								diff
							} else {
								step
							};

							smooth_scroll.at += amt;
							m_scroll_amt = amt;
						}
					}
				}

				if m_scroll_amt != 0.0 {
					if let Some(top_bin) = interface.get_bin_atop(m_window_x, m_window_y) {
						let mut in_bins = vec![top_bin.clone()];
						in_bins.append(&mut top_bin.ancestors());

						'bin_loop: for bin in in_bins {
							for (hook_id, (hb_wk, hook, func)) in hooks.iter_mut() {
								let hb = match hb_wk.upgrade() {
									Some(some) => some,
									None => {
										bad_hooks.push(*hook_id);
										continue;
									},
								};

								if hb.id() == bin.id() && hook.ty() == BinHookTy::MouseScroll {
									if let BinHookData::MouseScroll {
										scroll_amt,
										..
									} = hook
									{
										*scroll_amt = m_scroll_amt;
									}

									func(hb.clone(), hook); // Call MouseScroll
									break 'bin_loop;
								}
							}
						}
					}
				}

				for event in events {
					match event {
						BinHookEvent::MousePress(button) => {
							let top_bin_op = interface.get_bin_atop(m_window_x, m_window_y);

							if top_bin_op.as_ref().map(|v| v.id()) != focused {
								if let Some(bin_id) = &focused {
									for (hook_id, (hb_wk, hook, func)) in hooks.iter_mut() {
										let hb = match hb_wk.upgrade() {
											Some(some) => some,
											None => {
												bad_hooks.push(*hook_id);
												continue;
											},
										};

										if hb.id() == *bin_id {
											match hook.ty() {
												BinHookTy::LostFocus => {
													func(hb.clone(), hook);
												},

												BinHookTy::Press =>
													if let BinHookData::Press {
														key_active,
														mouse_active,
														..
													} = hook
													{
														for v in key_active.values_mut() {
															*v = false;
														}

														for v in mouse_active.values_mut() {
															*v = false;
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
													} = hook
													{
														for v in key_active.values_mut() {
															*v = false;
														}

														for v in mouse_active.values_mut() {
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
													} = hook
													{
														call = *pressed;

														for v in key_active.values_mut() {
															*v = false;
														}

														for v in mouse_active.values_mut() {
															*v = false;
														}
													}

													if call {
														func(hb.clone(), hook);
													}
												},

												_ => (),
											}
										}
									}
								}

								focused = top_bin_op.map(|v| v.id());

								if let Some(bin_id) = &focused {
									for (hook_id, (hb_wk, hook, func)) in hooks.iter_mut() {
										let hb = match hb_wk.upgrade() {
											Some(some) => some,
											None => {
												bad_hooks.push(*hook_id);
												continue;
											},
										};

										if hb.id() == *bin_id {
											if let BinHookData::Focused = hook {
												// Call Focused
												func(hb.clone(), hook)
											}
										}
									}
								}
							}

							if let Some(bin_id) = &focused {
								for (hook_id, (hb_wk, hook, func)) in hooks.iter_mut() {
									let hb = match hb_wk.upgrade() {
										Some(some) => some,
										None => {
											bad_hooks.push(*hook_id);
											continue;
										},
									};

									if hb.id() == *bin_id {
										match hook.ty() {
											BinHookTy::Press => {
												let mut check = false;

												if let BinHookData::Press {
													mouse_x,
													mouse_y,
													mouse_active,
													..
												} = hook
												{
													if let Some(v) =
														mouse_active.get_mut(&button)
													{
														if !*v {
															*v = true;
															*mouse_x = m_window_x;
															*mouse_y = m_window_y;
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

												if let BinHookData::Hold {
													mouse_active,
													..
												} = hook
												{
													if let Some(v) =
														mouse_active.get_mut(&button)
													{
														if !*v {
															*v = true;
															check = true;
														}
													}
												}

												if check && hook.is_active() {
													if let BinHookData::Hold {
														first_call,
														..
													} = hook
													{
														*first_call = Instant::now();
													}
												}
											},

											BinHookTy::Release => {
												let mut check = false;

												if let BinHookData::Release {
													mouse_active,
													..
												} = hook
												{
													if let Some(v) =
														mouse_active.get_mut(&button)
													{
														if !*v {
															*v = true;
															check = true;
														}
													}
												}

												if check && hook.is_active() {
													if let BinHookData::Release {
														pressed,
														..
													} = hook
													{
														*pressed = true;
													}
												}
											},

											_ => (),
										}
									}
								}
							}
						},

						BinHookEvent::MouseRelease(button) => {
							if let Some(bin_id) = &focused {
								for (hook_id, (hb_wk, hook, func)) in hooks.iter_mut() {
									let hb = match hb_wk.upgrade() {
										Some(some) => some,
										None => {
											bad_hooks.push(*hook_id);
											continue;
										},
									};

									if hb.id() == *bin_id {
										match hook.ty() {
											BinHookTy::Press => {
												if let BinHookData::Press {
													mouse_active,
													..
												} = hook
												{
													if let Some(v) =
														mouse_active.get_mut(&button)
													{
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
												} = hook
												{
													if let Some(v) =
														mouse_active.get_mut(&button)
													{
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

												if let BinHookData::Release {
													mouse_active,
													..
												} = hook
												{
													if let Some(v) =
														mouse_active.get_mut(&button)
													{
														if *v {
															*v = false;
															check = true;
														}
													}
												}

												if check && !hook.is_active() {
													let mut call = false;

													if let BinHookData::Release {
														pressed,
														..
													} = hook
													{
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

											_ => (),
										}
									}
								}
							}
						},

						BinHookEvent::Character(c) => {
							if let Some(bin_id) = &focused {
								for (hook_id, (hb_wk, hook, func)) in hooks.iter_mut() {
									let hb = match hb_wk.upgrade() {
										Some(some) => some,
										None => {
											bad_hooks.push(*hook_id);
											continue;
										},
									};

									if hb.id() == *bin_id {
										match hook.ty() {
											BinHookTy::Character => {
												func(hb.clone(), &BinHookData::Character(c)); // Call Character
											},
											_ => (),
										}
									}
								}
							}
						},

						BinHookEvent::KeyPress(key) => {
							if let Some(bin_id) = &focused {
								for (hook_id, (hb_wk, hook, func)) in hooks.iter_mut() {
									let hb = match hb_wk.upgrade() {
										Some(some) => some,
										None => {
											bad_hooks.push(*hook_id);
											continue;
										},
									};

									if hb.id() == *bin_id {
										match hook.ty() {
											BinHookTy::Press => {
												let mut check = false;

												if let BinHookData::Press {
													mouse_x,
													mouse_y,
													key_active,
													..
												} = hook
												{
													if let Some(v) = key_active.get_mut(&key) {
														if !*v {
															*v = true;
															*mouse_x = m_window_x;
															*mouse_y = m_window_y;
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

												if let BinHookData::Hold {
													key_active,
													..
												} = hook
												{
													if let Some(v) = key_active.get_mut(&key) {
														if !*v {
															*v = true;
															check = true;
														}
													}
												}

												if check && hook.is_active() {
													if let BinHookData::Hold {
														first_call,
														..
													} = hook
													{
														*first_call = Instant::now();
													}
												}
											},

											BinHookTy::Release => {
												let mut check = false;

												if let BinHookData::Release {
													key_active,
													..
												} = hook
												{
													if let Some(v) = key_active.get_mut(&key) {
														if !*v {
															*v = true;
															check = true;
														}
													}
												}

												if check && hook.is_active() {
													if let BinHookData::Release {
														pressed,
														..
													} = hook
													{
														*pressed = true;
													}
												}
											},

											_ => (),
										}
									}
								}
							}
						},

						BinHookEvent::KeyRelease(key) => {
							if let Some(bin_id) = &focused {
								for (hook_id, (hb_wk, hook, func)) in hooks.iter_mut() {
									let hb = match hb_wk.upgrade() {
										Some(some) => some,
										None => {
											bad_hooks.push(*hook_id);
											continue;
										},
									};

									if hb.id() == *bin_id {
										match hook.ty() {
											BinHookTy::Press => {
												if let BinHookData::Press {
													key_active,
													..
												} = hook
												{
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
												} = hook
												{
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

												if let BinHookData::Release {
													key_active,
													..
												} = hook
												{
													if let Some(v) = key_active.get_mut(&key) {
														if *v {
															*v = false;
															check = true;
														}
													}
												}

												if check && !hook.is_active() {
													let mut call = false;

													if let BinHookData::Release {
														pressed,
														..
													} = hook
													{
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

											_ => (),
										}
									}
								}
							}
						},

						_ => (),
					}
				}

				if let Some(bin_id) = &focused {
					for (hook_id, (hb_wk, hook, func)) in hooks.iter_mut() {
						let hb = match hb_wk.upgrade() {
							Some(some) => some,
							None => {
								bad_hooks.push(*hook_id);
								continue;
							},
						};

						if hb.id() == *bin_id {
							if let BinHookData::Hold {
								..
							} = hook
							{
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

								_ => false,
							} {
								func(hb.clone(), hook); // Call Hold

								if let BinHookData::Hold {
									last_call,
									mouse_x,
									mouse_y,
									..
								} = &mut *hook
								{
									*mouse_x = m_window_x;
									*mouse_y = m_window_y;
									*last_call = Instant::now();
								}
							}
						}
					}
				}

				for hook_id in bad_hooks {
					hooks.remove(&hook_id);
				}

				let elapsed = last_tick.elapsed();

				if elapsed < tick_interval {
					::std::thread::sleep(tick_interval - elapsed);
				}

				last_tick = Instant::now();
			}
		});

		(hman, itf_op_ret, itf_cond_ret)
	}
}
