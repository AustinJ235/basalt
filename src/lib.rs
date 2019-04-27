#![feature(arbitrary_self_types)]
#![feature(integer_atomics)]

extern crate winit;
#[macro_use]
pub extern crate vulkano;
extern crate vulkano_win;
#[macro_use]
extern crate vulkano_shaders;
extern crate rand;
extern crate parking_lot;
extern crate crossbeam;
extern crate num_cpus;
extern crate image;
extern crate decorum;
extern crate freetype_sys;
extern crate zhttp;
#[cfg(target_os = "unix")]
extern crate x11_dl;

pub mod keyboard;
pub mod mouse;
pub mod interface;
pub mod texture;
pub mod atlas;
pub mod misc;
pub mod shaders;
pub mod timer;
pub mod bindings;
pub mod tmp_image_access;
pub mod input;

use keyboard::Keyboard;
use mouse::Mouse;
use atlas::Atlas;
use interface::interface::Interface;
use vulkano_win::{VkSurfaceBuild};
use vulkano::sync::GpuFuture;
use vulkano::instance::{Instance,PhysicalDevice};
use vulkano::device::{self,Device,DeviceExtensions};
use vulkano::swapchain::{self,Swapchain};
use vulkano::command_buffer::AutoCommandBufferBuilder;
use std::sync::Arc;
use std::time::Instant;
use parking_lot::{Mutex,RwLock};
use std::sync::atomic::{self,AtomicBool,AtomicUsize};
use std::collections::VecDeque;
use std::thread;
use std::sync::Barrier;
use vulkano::swapchain::Surface;
use winit::Window;
use std::thread::JoinHandle;
use std::time::Duration;
use interface::bin::{self,BinStyle};
use input::Input;

const SHOW_SWAPCHAIN_WARNINGS: bool = false;

#[derive(Debug)]
pub struct Limits {
	pub max_image_dimension_2d: u32,
	pub max_image_dimension_3d: u32,
}

pub enum EngineEvent {
	WindowResized,
	DPIChanged(f32),
}	

struct Initials {
	device: Arc<Device>,
	graphics_queue: Arc<device::Queue>,
	transfer_queue: Arc<device::Queue>,
	surface: Arc<Surface<Window>>,
	swap_caps: swapchain::Capabilities,
	limits: Arc<Limits>,
	event_mk: Arc<Mutex<Option<Arc<Engine>>>>,
	event_mk_br: Arc<Barrier>,
	pdevi: usize,
	window_size: [u32; 2],
}

impl Initials {
	pub fn use_first_device(options: Options) -> Result<Self, String> {
		let mut device_num = 0;
		let mut show_devices = false;
		
		for arg in ::std::env::args() {
			if arg.starts_with("--use-device=") {
				let split_by_eq: Vec<_> = arg.split("=").collect();
				
				if split_by_eq.len() < 2 {
					println!("Incorrect '--use-device' usage. Example: '--use-device=2'");
					break;
				} else {
					device_num = match split_by_eq[1].parse() {
						Ok(ok) => ok,
						Err(_) => {
							println!("Incorrect '--use-device' usage. Example: '--use-device=2'");
							continue;
						}
					};
					
					println!("Using device: {}", device_num);
				}
			} else if arg.starts_with("--show-devices") {
				show_devices = true;
			}
		}
	
		let extensions = vulkano_win::required_extensions();
		let device_ext = DeviceExtensions { khr_swapchain: true, .. DeviceExtensions::none() };
		
		let window_result = Arc::new(Mutex::new(None));
		let window_result_copy = window_result.clone();
		let window_res_barrier = Arc::new(Barrier::new(2));
		let window_res_barrier_copy = window_res_barrier.clone();
		
		let event_mk = Arc::new(Mutex::new(None));
		let event_mk_copy = event_mk.clone();
		let event_mk_br = Arc::new(Barrier::new(2));
		let event_mk_br_copy = event_mk_br.clone();
		
		thread::spawn(move || {
			let mut events_loop = winit::EventsLoop::new();
			
			*window_result_copy.lock() = Some((|| -> _ {
				let instance = match Instance::new(None, &extensions, None) {
					Ok(ok) => ok,
					Err(e) => return Err(format!("Failed to create instance: {}", e))
				};
				
				let mut physical_devs: Vec<_> = PhysicalDevice::enumerate(&instance).collect();
				
				if show_devices {
					println!("Devices:");
					for (i, dev) in physical_devs.iter().enumerate() {
						println!("  {}: {}", i, dev.name());
					}
				}
				
				match physical_devs.get(device_num) {
					Some(_) => (),
					None => if device_num == 0 {
						return Err(format!("No physical devices available."))
					} else {
						return Err(format!("Phyiscal device not found."))
					}
				};
				
				let physical = physical_devs.remove(device_num);
				
				let surface = match winit::WindowBuilder::new()
					.with_dimensions((800, 400).into())
					.with_title(options.title.clone())
					.build_vk_surface(&events_loop, instance.clone())
				{
					Ok(ok) => ok,
					Err(e) => return Err(format!("Failed to build window: {}", e))
				};
				
				let window_size;
				
				surface.window().set_inner_size(if options.ignore_dpi {
					window_size = options.window_size.clone();
					
					winit::dpi::PhysicalSize::new(
						options.window_size[0] as f64,
						options.window_size[1] as f64
					).to_logical(surface.window().get_hidpi_factor())
				} else {
					let hidpi_factor = surface.window().get_hidpi_factor();
					window_size = [
						(options.window_size[0] as f64 * hidpi_factor).floor() as u32,
						(options.window_size[0] as f64 * hidpi_factor).floor() as u32
					];
				
					winit::dpi::LogicalSize::new(
						options.window_size[0] as f64,
						options.window_size[1] as f64
					)
				});
				
				let mut queue_family_opts = Vec::new();
			
				for family in physical.queue_families() {
					for _ in 0..family.queues_count() {
						queue_family_opts.push(family);
					}
				}
				
				let mut graphics_queue_ = None;
				let mut transfer_queue_ = None;
				
				for i in 0..queue_family_opts.len() {
					if
						queue_family_opts[i].supports_graphics() &&
						surface.is_supported(queue_family_opts[i]).unwrap_or(false)
					{	
						graphics_queue_ = Some((queue_family_opts[i], 0.8));
						queue_family_opts.remove(i);
						break;
					}
				} if graphics_queue_.is_none() {
					return Err(format!("Couldn't find a suitable queue for graphics."));
				}
				
				for i in 0..queue_family_opts.len() {
					if queue_family_opts[i].supports_transfers() {	
						transfer_queue_ = Some((queue_family_opts[i], 0.2));
						queue_family_opts.remove(i);
						break;
					}
				} if transfer_queue_.is_none() {
					println!("Couldn't find a suitable queue for transfers.\
						\nUsing graphics queue for transfers also.");
				}
				
				let mut req_queues = Vec::new();
				req_queues.push(graphics_queue_.unwrap());
				
				if let Some(transfer_queue) = transfer_queue_ {
					req_queues.push(transfer_queue);
				}
				
				let (device, mut queues) = match Device::new(
					physical, physical.supported_features(), 
					&device_ext, req_queues)
				{
					Ok(ok) => ok,
					Err(e) => return Err(format!("Failed to create device: {}", e))
				}; let graphics_queue = match queues.next() {
					Some(some) => some,
					None => return Err(format!("Device didn't have any queues"))
				}; let transfer_queue = match queues.next() {
					Some(some) => some,
					None => graphics_queue.clone()
				}; let swap_caps = match surface.capabilities(physical) {
					Ok(ok) => ok,
					Err(e) => return Err(format!("Failed to get surface capabilities: {}", e))
				};
				
				let phy_limits = physical.limits();
				
				let limits = Limits {
					max_image_dimension_2d: phy_limits.max_image_dimension_2d(),
					max_image_dimension_3d: phy_limits.max_image_dimension_3d(),
				};
				
				Ok(Initials {
					device: device,
					graphics_queue: graphics_queue,
					transfer_queue: transfer_queue,
					surface: surface.clone(),
					swap_caps: swap_caps,
					limits: Arc::new(limits),
					event_mk: event_mk,
					event_mk_br: event_mk_br,
					pdevi: device_num,
					window_size,
				})
			})());
			
			window_res_barrier_copy.wait();
			event_mk_br_copy.wait();
			
			let engine = event_mk_copy.lock().take().unwrap();
			input::winit::run(engine.clone(), &mut events_loop);
			
			/*let keyboard = engine.keyboard();
			let mouse = engine.mouse();
			let mut last_inst = Instant::now();
			let mut cursor_inside = true;
			
			loop {
				let elapsed = last_inst.elapsed();
				
				if elapsed.as_secs() == 0 {
					let millis = elapsed.subsec_millis();
					
					if millis < 10 {
						::std::thread::sleep(::std::time::Duration::from_millis((10-millis) as u64));
					} 
				}
				
				last_inst = Instant::now();
				let mut last_dpi_change = Instant::now() - Duration::from_secs(1);
				let mut ws_pre_dpi_change = [0; 2];
				events_loop.poll_events(|ev| {
					match ev {
						winit::Event::WindowEvent { event: winit::WindowEvent::CloseRequested, .. } => { engine.exit(); },
						winit::Event::WindowEvent { window_id: _, event: winit::WindowEvent::CursorMoved { position, .. } } => {
							let winit::dpi::PhysicalPosition { x, y } = position.to_physical(engine.surface.window().get_hidpi_factor());
							mouse.set_position(x as f32, y as f32);

							if engine.mouse_capture.load(atomic::Ordering::Relaxed) {
								let (win_size_x, win_size_y): (u32, u32) = engine.surface.window().get_inner_size().unwrap().into();
								let _ = engine.surface.window().set_cursor_position(((win_size_x/2) as i32, (win_size_y/2) as i32).into());
							}
						}, winit::Event::WindowEvent { window_id: _, event: winit::WindowEvent::KeyboardInput { device_id: _, input} } => {
							match input.state {
								winit::ElementState::Released => keyboard.release(input.scancode),
								winit::ElementState::Pressed => keyboard.press(input.scancode)
							}
						}, winit::Event::WindowEvent { window_id: _, event: winit::WindowEvent::MouseInput { state, button, .. } } => {
							match state {
								winit::ElementState::Released => mouse.release(mouse::Button::from_winit(button)),
								winit::ElementState::Pressed => mouse.press(mouse::Button::from_winit(button))
							}
						}, winit::Event::DeviceEvent { device_id: _, event: winit::DeviceEvent::Motion { axis, value } } => {
							match axis {
								0 => mouse.add_delta(-value as f32, 0.0),
								1 => mouse.add_delta(0.0, -value as f32),
								3 => if cursor_inside {
									mouse.scroll(value as f32)
								}, _ => println!("{} {}", axis, value),
							}
						},
						
						#[cfg(target_os = "windows")]
						winit::Event::WindowEvent { event: winit::WindowEvent::MouseWheel { delta, .. }, ..} => {
							if cursor_inside {
								match delta {
									winit::MouseScrollDelta::LineDelta(_, y) => {
										mouse.scroll(-y);
									}, winit::MouseScrollDelta::PixelDelta(data) => {
										println!("WARNING winit::MouseScrollDelta::PixelDelta is untested!");
										mouse.scroll(data.y as f32);
									}
								}
							}
						},
						
						winit::Event::WindowEvent { event: winit::WindowEvent::CursorEntered { .. }, .. } => { cursor_inside = true; },
						winit::Event::WindowEvent { event: winit::WindowEvent::CursorLeft { .. }, .. } => { cursor_inside = false; },
						
						winit::Event::WindowEvent { event: winit::WindowEvent::Resized(_ ), .. } => {
							if options.ignore_dpi {
								if last_dpi_change.elapsed() < Duration::from_millis(250) {
									last_dpi_change -= Duration::from_secs(1);
									engine.surface.window().set_inner_size(winit::dpi::PhysicalSize::new(
										ws_pre_dpi_change[0] as f64,
										ws_pre_dpi_change[1] as f64
									).to_logical(engine.surface.window().get_hidpi_factor()));
								} else {
									engine.force_resize.store(true, atomic::Ordering::Relaxed);
								}
							} else {
								engine.force_resize.store(true, atomic::Ordering::Relaxed);
							}
						},
						
						winit::Event::WindowEvent { event: winit::WindowEvent::HiDpiFactorChanged(dpi), .. } => {
							if !options.ignore_dpi {
								engine.interface_ref().set_scale(dpi as f32 * *engine.custom_scale.lock());
							} else {
								ws_pre_dpi_change = *engine.window_size.lock();
								last_dpi_change = Instant::now();
							}
						},
						
						winit::Event::WindowEvent { event: winit::WindowEvent::Focused(focused), .. } => {
							engine.keyboard_ref().window_focused(focused);
						},
						
						_ => ()
					}
				});
			}*/
		});
		
		window_res_barrier.wait();
		let mut window_result_op = window_result.lock();
		window_result_op.take().unwrap()
	}
}

#[derive(Debug,Clone)]
pub enum InputSource {
	Native,
	Winit,
	Custom
}

#[derive(Debug,Clone)]
pub struct Options {
	ignore_dpi: bool,
	window_size: [u32; 2],
	title: String,
	scale: f32,
	input_src: InputSource,
}

impl Default for Options {
	fn default() -> Self {
		Options {
			ignore_dpi: false,
			window_size: [1920, 1080],
			title: "vk-engine".to_string(),
			scale: 1.0,
			input_src: InputSource::Winit,
		}
	}
}

impl Options {
	pub fn ignore_dpi(mut self, to: bool) -> Self {
		self.ignore_dpi = to;
		self
	}
	
	pub fn window_size(mut self, width: u32, height: u32) -> Self {
		self.window_size = [width, height];
		self
	}
	
	pub fn title<T: AsRef<str>>(mut self, title: T) -> Self {
		self.title = String::from(title.as_ref());
		self
	}
	
	pub fn scale(mut self, to: f32) -> Self {
		self.scale = to;
		self
	}
}

pub enum ResizeTo {
	Dims(u32, u32),
	FullScreen(bool),
}

#[allow(dead_code)]
pub struct Engine {
	device: Arc<Device>,
	graphics_queue: Arc<device::Queue>,
	transfer_queue: Arc<device::Queue>,
	surface: Arc<Surface<Window>>,
	swap_caps: swapchain::Capabilities,
	do_every: RwLock<Vec<Arc<Fn() + Send + Sync>>>,
	keyboard: Arc<Keyboard>,
	mouse: Arc<Mouse>,
	mouse_capture: AtomicBool,
	allow_mouse_cap: AtomicBool,
	fps: AtomicUsize,
	interface: Arc<Interface>,
	atlas: Arc<Atlas>,
	input: Arc<Input>,
	wants_exit: AtomicBool,
	force_resize: AtomicBool,
	#[allow(dead_code)]
	limits: Arc<Limits>,
	resize_requested: AtomicBool,
	resize_to: Mutex<Option<ResizeTo>>,
	loop_thread: Mutex<Option<JoinHandle<Result<(), String>>>>,
	pdevi: usize,
	vsync: Mutex<bool>,
	wait_on_futures: Mutex<Vec<(Box<GpuFuture + Send + Sync>, Arc<Barrier>)>>,
	window_size: Mutex<[u32; 2]>,
	custom_scale: Mutex<f32>,
	options: Options,
	ignore_dpi_data: Mutex<Option<(usize, Instant, u32, u32)>>,
}

#[allow(dead_code)]
impl Engine {
	pub fn new(options: Options) -> Result<Arc<Self>, String> {
		unsafe {
			let initials = match Initials::use_first_device(options.clone()) {
				Ok(ok) => ok,
				Err(e) => return Err(e)
			};
			
			let mut engine = Arc::new(Engine {
				device: initials.device,
				graphics_queue: initials.graphics_queue,
				transfer_queue: initials.transfer_queue,
				surface: initials.surface,
				swap_caps: initials.swap_caps,
				do_every: RwLock::new(Vec::new()),
				keyboard: ::std::mem::uninitialized(),
				mouse: ::std::mem::uninitialized(),
				mouse_capture: AtomicBool::new(false),
				allow_mouse_cap: AtomicBool::new(true),
				fps: AtomicUsize::new(0),
				interface: ::std::mem::uninitialized(),
				limits: initials.limits.clone(),
				atlas: ::std::mem::uninitialized(),
				input: ::std::mem::uninitialized(),
				wants_exit: AtomicBool::new(false),
				force_resize: AtomicBool::new(false),
				resize_requested: AtomicBool::new(false),
				resize_to: Mutex::new(None),
				loop_thread: Mutex::new(None),
				pdevi: initials.pdevi,
				vsync: Mutex::new(true),
				wait_on_futures: Mutex::new(Vec::new()),
				window_size: Mutex::new(initials.window_size),
				custom_scale: Mutex::new(options.scale),
				options,
				ignore_dpi_data: Mutex::new(None),
			});
			
			let atlas_ptr = &mut Arc::get_mut(&mut engine).unwrap().atlas as *mut _;
			let mouse_ptr = &mut Arc::get_mut(&mut engine).unwrap().mouse as *mut _;
			let keyboard_ptr = &mut Arc::get_mut(&mut engine).unwrap().keyboard as *mut _;
			let interface_ptr = &mut Arc::get_mut(&mut engine).unwrap().interface as *mut _;
			let input_ptr = &mut Arc::get_mut(&mut engine).unwrap().input as *mut _;
			::std::ptr::write(atlas_ptr, Atlas::new(engine.clone()));
			::std::ptr::write(mouse_ptr, Arc::new(Mouse::new(engine.clone())));
			::std::ptr::write(keyboard_ptr, Keyboard::new(engine.clone()));
			::std::ptr::write(interface_ptr, Interface::new(engine.clone()));
			::std::ptr::write(input_ptr, Input::new(engine.clone()));
			
			if !engine.options.ignore_dpi {
				engine.interface_ref().set_scale(engine.surface.window().get_hidpi_factor() as f32 * engine.options.scale);
			} else if engine.options.scale != 1.0 {
				engine.interface_ref().set_scale(engine.options.scale);
			}
			
			*initials.event_mk.lock() = Some(engine.clone());
			initials.event_mk_br.wait();
			
			let help_bin = engine.interface.new_bin();
			help_bin.engine_use();
			
			help_bin.style_update(BinStyle {
				hidden: Some(true),
				pos_from_t: Some(0.0),
				pos_from_l: Some(0.0),
				pos_from_b: Some(0.0),
				width: Some(250.0),
				back_color: Some(bin::Color::srgb_hex("000000")),
				pad_t: Some(15.0),
				pad_b: Some(15.0),
				pad_l: Some(15.0),
				pad_r: Some(15.0),
				text_size: Some(14),
				text_color: Some(bin::Color::srgb_hex("ffffff")),
				text: format!("Ctrl + F1: Toggle Help"),//\r\nCtrl + F2 Display Atlas Image 0"),
				.. BinStyle::default()
			});
			
			engine.keyboard.on_press(vec![vec![keyboard::Qwery::LCtrl, keyboard::Qwery::F1]], Arc::new(move |_| {
				help_bin.toggle_hidden();
			}));
			
			engine.keyboard.on_press(vec![vec![keyboard::Qwery::F7]], Arc::new(move |keyboard::CallInfo {
				engine,
				..
			}| {
				engine.interface_ref().decrease_msaa();
				println!("MSAA set to {}X", engine.interface_ref().msaa());
			}));
			
			engine.keyboard.on_press(vec![vec![keyboard::Qwery::F8]], Arc::new(move |keyboard::CallInfo {
				engine,
				..
			}| {
				engine.interface_ref().increase_msaa();
				println!("MSAA set to {}X", engine.interface_ref().msaa());
			}));
			
			engine.keyboard.on_press(vec![vec![keyboard::Qwery::F10]], Arc::new(move |keyboard::CallInfo {
				engine,
				..
			}| {
				let mut vsync = engine.vsync.lock();
				*vsync = !*vsync;
				engine.force_resize.store(true, atomic::Ordering::Relaxed);
				
				if *vsync {
					println!("VSync Enabled!");
				} else {
					println!("VSync Disabled!");
				}
			}));
			
			engine.keyboard.on_press(vec![vec![keyboard::Qwery::LCtrl, keyboard::Qwery::Dash]], Arc::new(move |keyboard::CallInfo {
				engine,
				..
			}| {
				engine.add_scale(-0.05);
				
				if engine.options.ignore_dpi {
					println!("Current Scale: {:.1} %", engine.current_scale() * 100.0);
				} else {
					println!("Current Scale: {:.1} %", engine.current_scale_with_dpi() * 100.0);
				}
			}));
			
			engine.keyboard.on_press(vec![vec![keyboard::Qwery::LCtrl, keyboard::Qwery::Equal]], Arc::new(move |keyboard::CallInfo {
				engine,
				..
			}| {
				engine.add_scale(0.05);
				
				if engine.options.ignore_dpi {
					println!("Current Scale: {:.1} %", engine.current_scale() * 100.0);
				} else {
					println!("Current Scale: {:.1} %", engine.current_scale_with_dpi() * 100.0);
				}
			}));
			
			Ok(engine)
		}
	}
	
	pub fn send_event(&self, event: EngineEvent) {
		match event {
			EngineEvent::WindowResized => {
				if self.options.ignore_dpi {
					if let Some((count, last, w, h)) = &mut *self.ignore_dpi_data.lock() {
						println!("{} {} {} {:?}", count, w, h, *self.window_size.lock());
						
						if *count == 1 {
							self.surface.window().set_inner_size(winit::dpi::PhysicalSize::new(
								*w as f64,
								*h as f64
							).to_logical(self.surface.window().get_hidpi_factor()));
						} else if *count == 3 && last.elapsed() < Duration::from_millis(1000) { // TODO: Only if right click released
							self.surface.window().set_inner_size(winit::dpi::PhysicalSize::new(
								*w as f64,
								*h as f64
							).to_logical(self.surface.window().get_hidpi_factor()));
						} else {
							self.force_resize.store(true, atomic::Ordering::Relaxed);
						}
						
						*count += 1;
					} else {
						self.force_resize.store(true, atomic::Ordering::Relaxed);
					}
				} else {
					self.force_resize.store(true, atomic::Ordering::Relaxed);
				}
			},
			
			EngineEvent::DPIChanged(dpi) => {
				if self.options.ignore_dpi {
					let ws = self.window_size.lock();
					*self.ignore_dpi_data.lock() = Some((0, Instant::now(), ws[0], ws[1]));
				} else {
					self.interface_ref().set_scale(dpi as f32 * *self.custom_scale.lock());
				}
			}
		}
	}
	
	pub fn input_ref(&self) -> &Arc<Input> {
		&self.input
	}
	
	pub fn limits(&self) -> Arc<Limits> {
		self.limits.clone()
	}
	
	pub fn current_scale(&self) -> f32 {
		*self.custom_scale.lock()
	}
	
	pub fn current_scale_with_dpi(&self) -> f32 {
		*self.custom_scale.lock() * self.surface.window().get_hidpi_factor() as f32
	}
	
	pub fn set_scale(&self, to: f32) {
		let mut custom_scale = self.custom_scale.lock();
		*custom_scale = to;
		
		if self.options.ignore_dpi {
			self.interface_ref().set_scale(*custom_scale);
		} else {
			self.interface_ref().set_scale(*custom_scale
				* self.surface.window().get_hidpi_factor() as f32);
		}
	}
	
	pub fn add_scale(&self, amt: f32) {
		let mut custom_scale = self.custom_scale.lock();
		*custom_scale += amt;
		
		if self.options.ignore_dpi {
			self.interface_ref().set_scale(*custom_scale);
		} else {
			self.interface_ref().set_scale(*custom_scale
				* self.surface.window().get_hidpi_factor() as f32);
		}
	}
	
	
	/// This will only work if the engine is handling the loop thread. This
	/// is done via the method ``spawn_app_loop()``
	pub fn wait_for_exit(&self) -> Result<(), String> {
		match self.loop_thread.lock().take() {
			Some(handle) => match handle.join() {
				Ok(ok) => ok,
				Err(_) => Err(format!("Failed to join loop thread."))
			}, None => Ok(())
		}
	}
	
	pub fn spawn_app_loop(self: &Arc<Self>) {
		let engine = self.clone();
		
		*self.loop_thread.lock() = Some(thread::spawn(move || {
			engine.app_loop()
		}));
	}
	
	/// only works with app loop
	pub fn resize(&self, w: u32, h: u32) {
		*self.resize_to.lock() = Some(ResizeTo::Dims(w, h));
		self.resize_requested.store(true, atomic::Ordering::Relaxed);
	}
	
	/// only works with app loop
	pub fn fullscreen(&self, fullscreen: bool) {
		*self.resize_to.lock() = Some(ResizeTo::FullScreen(fullscreen));
		self.resize_requested.store(true, atomic::Ordering::Relaxed);
	}
	
	/// only works with app loop
	pub fn exit(&self) {
		self.wants_exit.store(true, atomic::Ordering::Relaxed);
	}
	
	/// only works with app loop
	pub fn do_every(&self, func: Arc<Fn() + Send + Sync>) {
		self.do_every.write().push(func);
	}
	
	/// only works with app loop
	pub fn fps(&self) -> usize {
		self.fps.load(atomic::Ordering::Relaxed)
	}
	
	/// only works with app loop
	pub fn wait_on_gpu_future(&self, future: Box<GpuFuture + Send + Sync>, barrier: Arc<Barrier>) {
		self.wait_on_futures.lock().push((future, barrier));
	}
	
	pub fn mouse(&self) -> Arc<Mouse> {
		self.mouse.clone()
	} pub fn mouse_ref(&self) -> &Arc<Mouse> {
		&self.mouse
	} pub fn keyboard(&self) -> Arc<Keyboard> {
		self.keyboard.clone()
	} pub fn keyboard_ref(&self) -> &Arc<Keyboard> {
		&self.keyboard
	} pub fn interface(&self) -> Arc<Interface> {
		self.interface.clone()
	} pub fn interface_ref(&self) -> &Arc<Interface> {
		&self.interface
	} pub fn atlas(&self) -> Arc<Atlas> {
		self.atlas.clone()
	} pub fn mouse_captured(&self) -> bool {
		self.mouse_capture.load(atomic::Ordering::Relaxed)
	} pub fn allow_mouse_cap(&self, to: bool) {
		self.allow_mouse_cap.store(to, atomic::Ordering::Relaxed);
	} pub fn mouse_cap_allowed(&self) -> bool {
		self.allow_mouse_cap.load(atomic::Ordering::Relaxed)
	} pub fn atlas_ref(&self) -> &Arc<Atlas> {
		&self.atlas
	} pub fn device(&self) -> Arc<Device> {
		self.device.clone()
	} pub fn device_ref(&self) -> &Arc<Device> {
		&self.device
	} pub fn transfer_queue(&self) -> Arc<device::Queue> {
		self.transfer_queue.clone()
	} pub fn transfer_queue_ref(&self) -> &Arc<device::Queue> {
		&self.transfer_queue
	} pub fn graphics_queue(&self) -> Arc<device::Queue> {
		self.graphics_queue.clone()
	} pub fn graphics_queue_ref(&self) -> &Arc<device::Queue> {
		&self.graphics_queue
	} pub fn physical_device_index(&self) -> usize {
		self.pdevi
	} pub fn surface(&self) -> Arc<Surface<Window>> {
		self.surface.clone()
	} pub fn surface_ref(&self) -> &Arc<Surface<Window>> {
		&self.surface
	} pub fn swap_caps(&self) -> &swapchain::Capabilities {
		&self.swap_caps
	} pub fn wants_exit(&self) -> bool {
		self.wants_exit.load(atomic::Ordering::Relaxed)
	}
	
	pub fn mouse_capture(&self, mut to: bool) {
		if !self.mouse_cap_allowed() {
			to = false;
		} self.mouse_capture.store(to, atomic::Ordering::Relaxed);
	}
	
	pub fn app_loop(self: &Arc<Self>) -> Result<(), String> {
		let mut win_size_x;
		let mut win_size_y;
		let mut frames = 0_usize;
		let mut last_out = Instant::now();
		let mut window_grab_cursor = false;
		let mut swapchain_ = None;
		let mut resized = false;
		
		let preferred_swap_formats = vec![
			vulkano::format::Format::R8G8B8A8Srgb,
			vulkano::format::Format::B8G8R8A8Srgb,
		];
		
		let mut swapchain_format_ = None;
		
		for a in &preferred_swap_formats {
			for &(ref b, _) in &self.swap_caps.supported_formats {
				if a == b {
					swapchain_format_ = Some(*a);
					break;
				}
			} if swapchain_format_.is_some() {
				break;
			}
		}
		
		let swapchain_format = match swapchain_format_ {
			Some(some) => some,
			None => return Err(format!("Failed to find capatible format for swapchain. Avaible formats: {:?}", self.swap_caps.supported_formats))
		};
		
		let mut itf_renderer = interface::render::ItfRenderer::new(self.clone());
		
		'resize: loop {
			let [x, y] = self.surface.capabilities(PhysicalDevice::from_index(
				self.surface.instance(), self.pdevi).unwrap()).unwrap().current_extent.unwrap();
			win_size_x = x;
			win_size_y = y;
			*self.window_size.lock() = [x, y];
			
			let present_mode = if *self.vsync.lock() {
				if self.swap_caps.present_modes.relaxed {
					swapchain::PresentMode::Relaxed
				} else {
					swapchain::PresentMode::Fifo
				}
			} else {
				if self.swap_caps.present_modes.mailbox {
					swapchain::PresentMode::Mailbox
				} else if self.swap_caps.present_modes.immediate {
					swapchain::PresentMode::Immediate
				} else {
					swapchain::PresentMode::Fifo
				}
			};
			
			let old_swapchain = swapchain_.as_ref().map(|v: &(Arc<Swapchain<_>>, _)| v.0.clone());
					
			swapchain_ = Some(match Swapchain::new(
				self.device.clone(), self.surface.clone(),
				self.swap_caps.min_image_count, swapchain_format,
				[x, y], 1, self.swap_caps.supported_usage_flags,
				&self.graphics_queue, swapchain::SurfaceTransform::Identity,
				swapchain::CompositeAlpha::Opaque, present_mode,
				true, old_swapchain.as_ref()
			) {
				Ok(ok) => ok,
				Err(e) => {
					if SHOW_SWAPCHAIN_WARNINGS { println!("swapchain recreation error: {:?}", e); }
					continue;
				}
			});
			
			let (swapchain, images) = (&swapchain_.as_ref().unwrap().0, &swapchain_.as_ref().unwrap().1);
			let mut previous_frame = Box::new(vulkano::sync::now(self.device.clone())) as Box<GpuFuture>;
			let mut fps_avg = VecDeque::new();
			
			loop {
				if self.resize_requested.load(atomic::Ordering::Relaxed) {
					self.resize_requested.store(true, atomic::Ordering::Relaxed);
					
					if let Some(resize_to) = self.resize_to.lock().take() {
						match resize_to {
							ResizeTo::FullScreen(f) => match f {
								true => {
									self.surface.window().set_fullscreen(Some(self.surface.window().get_current_monitor()));
								}, false => {
									self.surface.window().set_fullscreen(None);
								}
							}, ResizeTo::Dims(w, h) => {
								self.surface.window().set_inner_size(if self.options.ignore_dpi {
									winit::dpi::PhysicalSize::new(
										w as f64,
										h as f64
									).to_logical(self.surface.window().get_hidpi_factor())
								} else {
									winit::dpi::LogicalSize::new(
										w as f64,
										h as f64
									)
								});
							}
						}
						
						resized = true;
						continue 'resize;
					}
				}
				
				let duration = last_out.elapsed();
				let millis = (duration.as_secs()*1000) as f32 + (duration.subsec_nanos() as f32/1000000.0);
		
				if millis >= 50.0 {
					let fps = frames as f32 / (millis/1000.0);
					fps_avg.push_back(fps);
					
					if fps_avg.len() > 20 {
						fps_avg.pop_front();
					}
					
					let mut sum = 0.0;
					
					for num in &fps_avg {
						sum += *num;
					}
					
					let avg_fps = f32::floor(sum / fps_avg.len() as f32) as usize;
					self.fps.store(avg_fps, atomic::Ordering::Relaxed);
					frames = 0;
					last_out = Instant::now();
				}
		
				frames += 1;
			
				for func in &*self.do_every.read() {
					func()
				}
				
				if self.force_resize.swap(false, atomic::Ordering::Relaxed) {
					resized = true;
					continue 'resize;
				}
		
				let (image_num, acquire_future) = match swapchain::acquire_next_image(swapchain.clone(), Some(::std::time::Duration::new(1, 0))) {
					Ok(ok) => ok,
					Err(e) => {
						if SHOW_SWAPCHAIN_WARNINGS { println!("swapchain::acquire_next_image() Err: {:?}", e); }
						resized = true;
						continue 'resize;
					}
				};
				
				let cmd_buf = AutoCommandBufferBuilder::primary_one_time_submit(self.device.clone(), self.graphics_queue.family()).unwrap();
				let (cmd_buf, _) = itf_renderer.draw(cmd_buf, [win_size_x, win_size_y], resized, images, true, image_num);
				let cmd_buf = cmd_buf.build().unwrap();	
				
				let mut future: Box<GpuFuture> = Box::new(previous_frame.join(acquire_future)) as Box<_>;
				
				for (to_join, barrier) in self.wait_on_futures.lock().split_off(0) {
					barrier.wait();
					future = Box::new(future.join(to_join));
				}
				
				let mut future = match future.then_execute(self.graphics_queue.clone(), cmd_buf).expect("1")
					.then_swapchain_present(self.graphics_queue.clone(), swapchain.clone(), image_num)
					.then_signal_fence_and_flush()
				{
					Ok(ok) => ok,
					Err(e) => match e {
						vulkano::sync::FlushError::OutOfDate => {
							resized = true;
							continue 'resize;
						}, _ => panic!("then_signal_fence_and_flush() {:?}", e)
					}
				};
				
				future.wait(None).unwrap();
				future.cleanup_finished();
				previous_frame = Box::new(future);
				
				let grab_cursor = self.mouse_capture.load(atomic::Ordering::Relaxed);
			
				if grab_cursor != window_grab_cursor {
					self.surface.window().hide_cursor(grab_cursor);
					let _ = self.surface.window().grab_cursor(grab_cursor);
					window_grab_cursor = grab_cursor;
				}
				
				resized = false;
				if self.wants_exit.load(atomic::Ordering::Relaxed) { break 'resize }
			}
		}
		
		Ok(())
	}
}

