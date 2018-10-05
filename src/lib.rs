#![feature(arbitrary_self_types)]
#![feature(duration_as_u128)]
#![recursion_limit="512"]

extern crate winit;
#[macro_use]
extern crate vulkano;
#[macro_use]
extern crate vulkano_shader_derive;
extern crate vulkano_win;
extern crate cgmath;
extern crate rand;
extern crate parking_lot;
extern crate crossbeam;
extern crate freetype_sys;
extern crate num_cpus;
extern crate image;
extern crate serde;
#[allow(unused_imports)]
#[macro_use]
extern crate serde_derive;
extern crate bincode;

pub mod keyboard;
pub mod camera;
pub mod mouse;
pub mod interface;
mod texture;
pub mod atlas;
pub mod buffers;
pub mod serialize;
mod misc;
mod shaders;
mod sync;
pub mod timer;

use keyboard::Keyboard;
use camera::Camera;
use mouse::Mouse;
use atlas::Atlas;
use interface::interface::Interface;

use vulkano_win::{VkSurfaceBuild};
use vulkano::sync::GpuFuture;
use vulkano::descriptor::descriptor_set::PersistentDescriptorSet;
use vulkano::instance::{Instance,PhysicalDevice};
use vulkano::device::{self,Device,DeviceExtensions};
use vulkano::buffer::cpu_access::CpuAccessibleBuffer;
use vulkano::buffer::immutable::ImmutableBuffer;
use vulkano::buffer::BufferUsage;
use vulkano::swapchain::{self,Swapchain};
use vulkano::image::attachment::AttachmentImage;
use vulkano::pipeline::GraphicsPipeline;
use vulkano::pipeline::vertex::SingleBufferDefinition;
use vulkano::pipeline::viewport::Viewport;
use vulkano::framebuffer::{Subpass,Framebuffer};
use vulkano::command_buffer::{self,AutoCommandBufferBuilder};
use vulkano::sampler::Sampler;
use std::sync::{Arc,Weak};
use std::time::Instant;
use parking_lot::{Mutex,RwLock};
use std::collections::BTreeMap;
use std::sync::atomic::{self,AtomicBool,AtomicUsize};
use std::collections::VecDeque;
use rand::random;
use vulkano::descriptor::descriptor_set::FixedSizeDescriptorSetsPool;
use shaders::*;
use buffers::basic::BasicBuf;
use cgmath::SquareMatrix;
use std::f32::consts::PI;
use std::thread;
use buffers::Buffer;
use buffers::multi_basic::MultiBasicBuf;
use std::sync::Barrier;
use vulkano::swapchain::SwapchainCreationError;
use vulkano::swapchain::Surface;
use winit::Window;
use std::thread::JoinHandle;

#[cfg(target_os = "linux")]
use winit::os::unix::WindowExt;

const ITF_VSYNC: bool = true;
const ITF_MSAA: u32 = 4;

const INITAL_WIN_SIZE: [u32; 2] = [1920, 1080];
const COLOR_FORMAT: vulkano::format::Format = vulkano::format::Format::R16G16B16A16Unorm;
const DEPTH_FORMAT: vulkano::format::Format = vulkano::format::Format::D32Sfloat;

#[derive(Debug)]
pub(crate) struct Limits {
	pub max_image_dimension_2d: u32,
	pub max_image_dimension_3d: u32,
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
}

#[derive(Clone)]
pub struct SSAOSettings {
	pub samples: i32,
	pub strength: f32,
	pub sample_scale: f32,
	pub range: f32,
}

impl SSAOSettings {
	pub fn normal() -> Self {
		SSAOSettings {
			samples: 64,
			strength: 1.0,
			sample_scale: 1.0,
			range: 1.0,
		}
	}
}

impl Initials {
	pub fn use_first_device() -> Result<Self, String> {
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
					.build_vk_surface(&events_loop, instance.clone())
				{
					Ok(ok) => ok,
					Err(e) => return Err(format!("Failed to build window: {}", e))
				};
				
				let bs_size = winit::dpi::PhysicalSize::new(INITAL_WIN_SIZE[0] as f64, INITAL_WIN_SIZE[1] as f64).to_logical(surface.window().get_hidpi_factor());
				surface.window().set_inner_size(bs_size);
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
				})
			})());
			
			window_res_barrier_copy.wait();
			event_mk_br_copy.wait();
			
			let engine = event_mk_copy.lock().take().unwrap();
			let keyboard = engine.keyboard();
			let mouse = engine.mouse();
			let mut last_inst = Instant::now();
			
			loop {
				let elapsed = last_inst.elapsed();
				
				if elapsed.as_secs() == 0 {
					let millis = elapsed.subsec_millis();
					
					if millis < 10 {
						::std::thread::sleep(::std::time::Duration::from_millis((10-millis) as u64));
					} 
				}
				
				last_inst = Instant::now();
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
								3 => mouse.scroll(value as f32),
								_ => println!("{} {}", axis, value),
							}
						}, _ => ()
					}
				});
			}
		});
		
		window_res_barrier.wait();
		let mut window_result_op = window_result.lock();
		window_result_op.take().unwrap()
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
	camera: Arc<Camera>,
	mouse_capture: AtomicBool,
	allow_mouse_cap: AtomicBool,
	buffers_: Mutex<(BTreeMap<u64, Weak<Buffer + Send + Sync>>, u64)>,
	triangles: AtomicUsize,
	fps: AtomicUsize,
	interface: Arc<Interface>,
	atlas: Arc<Atlas>,
	ssao_settings: Arc<Mutex<SSAOSettings>>,
	wants_exit: AtomicBool,
	force_resize: AtomicBool,
	show_triangles: AtomicBool,
	shadow_state: Mutex<i32>,
	#[allow(dead_code)]
	limits: Arc<Limits>,
	resize_requested: AtomicBool,
	resize_to: Mutex<Option<ResizeTo>>,
	loop_thread: Mutex<Option<JoinHandle<Result<(), String>>>>,
	pdevi: usize, 
}

#[allow(dead_code)]
impl Engine {
	pub fn new() -> Result<Arc<Self>, String> {
		unsafe {
			let initials = match Initials::use_first_device() {
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
				camera: Arc::new(Camera::new()),
				mouse_capture: AtomicBool::new(false),
				allow_mouse_cap: AtomicBool::new(true),
				buffers_: Mutex::new((BTreeMap::new(), 0)),
				triangles: AtomicUsize::new(0),
				fps: AtomicUsize::new(0),
				interface: ::std::mem::uninitialized(),
				limits: initials.limits.clone(),
				atlas: Arc::new(Atlas::new(initials.limits)),
				ssao_settings: Arc::new(Mutex::new(SSAOSettings::normal())),
				wants_exit: AtomicBool::new(false),
				force_resize: AtomicBool::new(false),
				show_triangles: AtomicBool::new(false),
				shadow_state: Mutex::new(1),
				resize_requested: AtomicBool::new(false),
				resize_to: Mutex::new(None),
				loop_thread: Mutex::new(None),
				pdevi: initials.pdevi,
			});
			
			let mouse_ptr = &mut Arc::get_mut(&mut engine).unwrap().mouse as *mut _;
			let keyboard_ptr = &mut Arc::get_mut(&mut engine).unwrap().keyboard as *mut _;
			let interface_ptr = &mut Arc::get_mut(&mut engine).unwrap().interface as *mut _;
			::std::ptr::write(mouse_ptr, Arc::new(Mouse::new(engine.clone())));
			::std::ptr::write(keyboard_ptr, Keyboard::new(engine.clone()));
			::std::ptr::write(interface_ptr, Interface::new(engine.clone()));
			
			*initials.event_mk.lock() = Some(engine.clone());
			initials.event_mk_br.wait();
			
			engine.keyboard.on_press(vec![vec![keyboard::Qwery::Dash]], Arc::new(move |keyboard::CallInfo {
				engine,
				..
			}| {
				engine.interface_ref().decrease_scale(0.05);
			}));
			
			engine.keyboard.on_press(vec![vec![keyboard::Qwery::Equal]], Arc::new(move |keyboard::CallInfo {
				engine,
				..
			}| {
				engine.interface_ref().increase_scale(0.05);
			}));
				
				
			Ok(engine)
		}
	}
	
	/// This will only work if the engine is handling the loop thread. This
	/// is done via the methods ``spawn_exec_loop()`` or ``spawn_app_loop()``
	pub fn wait_for_exit(&self) -> Result<(), String> {
		match self.loop_thread.lock().take() {
			Some(handle) => match handle.join() {
				Ok(ok) => ok,
				Err(_) => Err(format!("Failed to join loop thread."))
			}, None => Ok(())
		}
	}
	
	pub fn spawn_exec_loop(self: &Arc<Self>) {
		let engine = self.clone();
		
		*self.loop_thread.lock() = Some(thread::spawn(move || {
			engine.exec_loop()
		}));
	}
	
	pub fn spawn_app_loop(self: &Arc<Self>) {
		let engine = self.clone();
		
		*self.loop_thread.lock() = Some(thread::spawn(move || {
			engine.app_loop()
		}));
	}
	
	pub fn resize(&self, w: u32, h: u32) {
		*self.resize_to.lock() = Some(ResizeTo::Dims(w, h));
		self.resize_requested.store(true, atomic::Ordering::Relaxed);
	}
	
	pub fn fullscreen(&self, fullscreen: bool) {
		*self.resize_to.lock() = Some(ResizeTo::FullScreen(fullscreen));
		self.resize_requested.store(true, atomic::Ordering::Relaxed);
	}
	
	#[cfg(target_os = "linux")]
	pub fn get_xlib_display(&self) -> Option<*mut ::std::os::raw::c_void> {
		self.surface.window().get_xlib_display()
	}
	
	#[cfg(target_os = "linux")]
	pub fn get_xlib_window(&self) -> Option<::std::os::raw::c_ulong> {
		self.surface.window().get_xlib_window()
	}
	
	#[cfg(target_os = "linux")]
	pub fn get_xlib_screen_id(&self) -> Option<::std::os::raw::c_int> {
		self.surface.window().get_xlib_screen_id()
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
	} pub fn camera(&self) -> &Arc<Camera> {
		&self.camera
	} pub fn exit(&self) {
		self.wants_exit.store(true, atomic::Ordering::Relaxed);
	} pub fn set_ssao_settings(&self, settings: SSAOSettings) {
		*self.ssao_settings.lock() = settings;
	} pub fn get_ssao_settings(&self) -> SSAOSettings {
		self.ssao_settings.lock().clone()
	} pub fn get_shadow_state(&self) -> i32 {
		*self.shadow_state.lock()
	} pub fn set_shadow_state(&self, state: i32) {
		*self.shadow_state.lock() = state;
	} pub fn do_every(&self, func: Arc<Fn() + Send + Sync>) {
		self.do_every.write().push(func);
	} pub fn mouse_captured(&self) -> bool {
		self.mouse_capture.load(atomic::Ordering::Relaxed)
	} pub fn allow_mouse_cap(&self, to: bool) {
		self.allow_mouse_cap.store(to, atomic::Ordering::Relaxed);
	} pub fn mouse_cap_allowed(&self) -> bool {
		self.allow_mouse_cap.load(atomic::Ordering::Relaxed)
	} pub fn triangles(&self) -> usize {
		self.triangles.load(atomic::Ordering::Relaxed)
	} pub fn fps(&self) -> usize {
		self.fps.load(atomic::Ordering::Relaxed)
	} pub (crate) fn graphics_queue(&self) -> Arc<device::Queue> {
		self.graphics_queue.clone()
	} pub (crate) fn graphics_queue_ref(&self) -> &Arc<device::Queue> {
		&self.graphics_queue
	} pub (crate) fn device(&self) -> Arc<Device> {
		self.device.clone()
	} pub (crate) fn device_ref(&self) -> &Arc<Device> {
		&self.device
	} pub fn atlas_ref(&self) -> &Arc<Atlas> {
		&self.atlas
	} pub (crate) fn transfer_queue(&self) -> Arc<device::Queue> {
		self.transfer_queue.clone()
	} pub (crate) fn transfer_queue_ref(&self) -> &Arc<device::Queue> {
		&self.transfer_queue
	}
	
	pub fn new_buffer(&self) -> Arc<BasicBuf> {
		let mut buffers = self.buffers_.lock();
		let id = buffers.1;
		buffers.1 += 1;
		let buffer = Arc::new(BasicBuf::new(self.atlas.clone()));
		let engine_copy = Arc::downgrade(&(buffer.clone() as Arc<Buffer + Send + Sync>));
		buffers.0.insert(id, engine_copy);
		buffer
	}
	
	pub fn new_multi_buffer(&self) -> Arc<MultiBasicBuf> {
		let mut buffers = self.buffers_.lock();
		let id = buffers.1;
		buffers.1 += 1;
		let buffer = Arc::new(MultiBasicBuf::new(self.atlas.clone()));
		let engine_copy = Arc::downgrade(&(buffer.clone() as Arc<Buffer + Send + Sync>));
		buffers.0.insert(id, engine_copy);
		buffer
	}
	
	fn delete_buffer_list(&self, ids: Vec<u64>) {
		if !ids.is_empty() {
			let mut buffers = self.buffers_.lock();
		
			for id in ids {
				buffers.0.remove(&id);
			}
		}
	}
	
	pub fn mouse_capture(&self, mut to: bool) {
		if !self.mouse_cap_allowed() {
			to = false;
		}
		
		self.mouse_capture.store(to, atomic::Ordering::Relaxed);
	}
	
	pub fn toggle_show_triangles(&self) {
		if self.show_triangles.load(atomic::Ordering::Relaxed) {
			self.show_triangles.store(false, atomic::Ordering::Relaxed);
		} else {
			self.show_triangles.store(true, atomic::Ordering::Relaxed);
		} self.force_resize.store(true, atomic::Ordering::Relaxed);
	}
	
	pub fn exec_loop(&self) -> Result<(), String> {
		let mut win_size_x;
		let mut win_size_y;
		
		let vs = vs::Shader::load(self.device.clone()).expect("failed to create shader module");
		let fs = fs::Shader::load(self.device.clone()).expect("failed to create shader module");
		let square_vs = square_vs::Shader::load(self.device.clone()).expect("failed to create shader module");
		let deferred_fs = deferred_fs::Shader::load(self.device.clone()).expect("failed to create shader module");
		let final_fs = final_fs::Shader::load(self.device.clone()).expect("failed to create shader module");
		let interface_vs = interface_vs::Shader::load(self.device.clone()).expect("failed to create shader module");
		let interface_fs = interface_fs::Shader::load(self.device.clone()).expect("failed to create shader module");
		let shadow_vs = shadow_vs::Shader::load(self.device.clone()).expect("failed to create shader module");
		let shadow_fs = shadow_fs::Shader::load(self.device.clone()).expect("failed to create shader module");
		
		let uniform_buffer = vulkano::buffer::cpu_pool::CpuBufferPool::<vs::ty::Data>::new(
			self.device.clone(),
			vulkano::buffer::BufferUsage::all(),
		);
		
		let shadow_uniform_buffer = vulkano::buffer::cpu_pool::CpuBufferPool::<shadow_vs::ty::Data>::new(
			self.device.clone(),
			vulkano::buffer::BufferUsage::all(),
		);

		let square_buf = {
			#[derive(Debug, Clone)]
			struct Vertex { position: [f32; 2] }
			impl_vertex!(Vertex, position);

			CpuAccessibleBuffer::from_iter(self.device.clone(), BufferUsage::all(), [
				Vertex { position: [-1.0, -1.0] },
				Vertex { position: [1.0, -1.0] },
				Vertex { position: [1.0, 1.0] },
				Vertex { position: [1.0, 1.0] },
				Vertex { position: [-1.0, 1.0] },
				Vertex { position: [-1.0, -1.0] }
			].iter().cloned()).expect("failed to create buffer")
		};
		
		let mut skybox_data = Vec::with_capacity(512*512*6*4);
		
		for path in [
			"./assets/skybox/skybox_left.png",
			"./assets/skybox/skybox_right.png",
			"./assets/skybox/skybox_bottom.png",
			"./assets/skybox/skybox_top.png",
			"./assets/skybox/skybox_back.png",
			"./assets/skybox/skybox_front.png"
		].into_iter() {
			let mut data = texture::load_image(path).unwrap().data;
			
			for chunk in data.chunks(4).rev() {
				for v in chunk {
					skybox_data.push(v.clone());
				}
			}
		}
		
		let skybox_tex = vulkano::image::immutable::ImmutableImage::from_iter(
			skybox_data.into_iter(),
			vulkano::image::Dimensions::Cubemap {
				size: 512,
			}, vulkano::format::Format::R8G8B8A8Srgb,
			self.transfer_queue.clone()
		).unwrap().0;
		
		let deferred_uni = {
			let mut samples = [[0.0; 3]; 512];

			for i in 0..512 {
				let radius = f32::abs(random::<f32>());
				let theta = ((random::<f32>() / 2.0) + 0.5) * (2.0 * PI);
				let phi = random::<f32>() * 2.0 * PI;
				let x = radius * f32::cos(theta) * f32::cos(phi);
				let y = radius * f32::sin(phi);
				let z = radius * f32::sin(theta) * f32::cos(phi);
				samples[i] = [x, y, z];
			}
			
			ImmutableBuffer::<deferred_fs::ty::Data>::from_data(
				deferred_fs::ty::Data {
					samples: samples.into()
				}, vulkano::buffer::BufferUsage::all(), self.graphics_queue.clone()
			).unwrap().0
		};
		
		let deferred_uni_other_pool = vulkano::buffer::cpu_pool::CpuBufferPool::<deferred_fs::ty::Other>::new(
			self.device.clone(),
			vulkano::buffer::BufferUsage::uniform_buffer(),
		);

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
		
		println!("Using {:?} for the swapchain format.", swapchain_format);
		let mut itf_cmds = Vec::new();
		
		'resize: loop {
			let [x, y] = self.surface.capabilities(PhysicalDevice::from_index(
				self.surface.instance(), self.pdevi).unwrap()).unwrap().current_extent.unwrap();
			win_size_x = x;
			win_size_y = y;
		
			if swapchain_.is_none() {
				swapchain_ = Some(Swapchain::new(
					self.device.clone(), self.surface.clone(),
					self.swap_caps.min_image_count, swapchain_format,
					self.swap_caps.current_extent.unwrap(), 1, self.swap_caps.supported_usage_flags,
					&self.graphics_queue, swapchain::SurfaceTransform::Identity,
					swapchain::CompositeAlpha::Opaque, swapchain::PresentMode::Immediate,
					true, None
				).expect("failed to create swapchain"))
			} else {
				let swapchain = swapchain_.as_mut().unwrap();
				*swapchain = match swapchain.0.recreate_with_dimension([x, y]) {
					Ok(ok) => ok,
					Err(e) => {
						match e {
							SwapchainCreationError::OldSwapchainAlreadyUsed => return Err(format!("Swapchain already in use.")),
							_ => ()
						}
						
						println!("swapchain recreation error: {:?}", e);
						continue;
					}
				};
			}
			
			let (swapchain, images) = (&swapchain_.as_ref().unwrap().0, &swapchain_.as_ref().unwrap().1);
			
			let shadow_dims = [images[0].dimensions()[0] * 4, images[0].dimensions()[0] * 4];
			//let shadow_dims = [self.limits.max_image_dimension_2d; 2];

			let basic_depth_buf = AttachmentImage::with_usage(
				self.device.clone(), images[0].dimensions(),
				DEPTH_FORMAT,
				vulkano::image::ImageUsage {
					depth_stencil_attachment: true,
					transfer_source: true,
					sampled: true,
					.. vulkano::image::ImageUsage::none()
				}
			).unwrap();
			
			let mut depth_write_queue = VecDeque::new();
			let mut depth_read_queue = VecDeque::new();
			let depth_queue_size = 1_usize;
			
			for _ in 0..depth_queue_size {
				depth_write_queue.push_back(CpuAccessibleBuffer::from_iter(
					self.device.clone(), BufferUsage::all(),
					(0 .. images[0].dimensions()[0] * images[0].dimensions()[1]).map(|_| 0.0_f32)
				).unwrap());
			}
			
			let shadow_depth_buf = AttachmentImage::with_usage(
				self.device.clone(), shadow_dims,
				DEPTH_FORMAT,
				vulkano::image::ImageUsage {
					depth_stencil_attachment: true,
					sampled: true,
					.. vulkano::image::ImageUsage::none()
				}
			).unwrap();
	
			let basic_color_buf = AttachmentImage::with_usage(
				self.device.clone(),
				images[0].dimensions(),
				COLOR_FORMAT,
				vulkano::image::ImageUsage {
					sampled: true,
					color_attachment: true,
					.. vulkano::image::ImageUsage::none()
				}
			).unwrap();

			let basic_normal_buf = AttachmentImage::with_usage(
				self.device.clone(),
				images[0].dimensions(),
				vulkano::format::Format::R16G16B16A16Sfloat,
				vulkano::image::ImageUsage {
					sampled: true,
					color_attachment: true,
					.. vulkano::image::ImageUsage::none()
				}
			).unwrap();
			
			let basic_trans_depth_buf = AttachmentImage::with_usage(
				self.device.clone(), images[0].dimensions(),
				DEPTH_FORMAT,
				vulkano::image::ImageUsage {
					depth_stencil_attachment: true,
					sampled: true,
					.. vulkano::image::ImageUsage::none()
				}
			).unwrap();
	
			let basic_trans_color_buf = AttachmentImage::with_usage(
				self.device.clone(),
				images[0].dimensions(),
				COLOR_FORMAT,
				vulkano::image::ImageUsage {
					sampled: true,
					color_attachment: true,
					.. vulkano::image::ImageUsage::none()
				}
			).unwrap();

			let basic_trans_normal_buf = AttachmentImage::with_usage(
				self.device.clone(),
				images[0].dimensions(),
				vulkano::format::Format::R16G16B16A16Sfloat,
				vulkano::image::ImageUsage {
					sampled: true,
					color_attachment: true,
					.. vulkano::image::ImageUsage::none()
				}
			).unwrap();
		
			let deferred_color_buf = AttachmentImage::with_usage(
				self.device.clone(),
				images[0].dimensions(),
				COLOR_FORMAT,
				vulkano::image::ImageUsage {
					color_attachment: true,
					sampled: true,
					.. vulkano::image::ImageUsage::none()
				}
			).unwrap();
			
			let itf_color_buf = AttachmentImage::with_usage(
				self.device.clone(),
				images[0].dimensions(),
				COLOR_FORMAT,
				vulkano::image::ImageUsage {
					color_attachment: true,
					sampled: true,
					.. vulkano::image::ImageUsage::none()
				}
			).unwrap();
			
			let itf_depth_buf = AttachmentImage::with_usage(
				self.device.clone(), images[0].dimensions(),
				DEPTH_FORMAT,
				vulkano::image::ImageUsage {
					depth_stencil_attachment: true,
					transfer_source: true,
					sampled: true,
					.. vulkano::image::ImageUsage::none()
				}
			).unwrap();
			
			let clear_pass1 = vec![
				1.0.into(),
				[0.0, 0.0, 1.0, 1.0].into(),
				[0.0, 0.0, 0.0, 0.0].into(),
				1.0.into(),
				[0.0, 0.0, 0.0, 0.0].into(),
				[0.0, 0.0, 0.0, 0.0].into(),
			];
			
			let clear_pass15 = vec![
				1.0.into()
			];

			let r_pass1 = Arc::new(
				ordered_passes_renderpass!(self.device.clone(),
					attachments: {
						depth: {
							load: Clear,
							store: Store,
							format: DEPTH_FORMAT,
							samples: 1,
						}, basic_color_buf: {
							load: Clear,
							store: Store,
							format: COLOR_FORMAT,
							samples: 1,
						}, basic_normal_buf: {
							load: Clear,
							store: Store,
							format: vulkano::format::Format::R16G16B16A16Sfloat,
							samples: 1,
						}, trans_depth: {
							load: Clear,
							store: Store,
							format: DEPTH_FORMAT,
							samples: 1,
						}, basic_trans_color_buf: {
							load: Clear,
							store: Store,
							format: COLOR_FORMAT,
							samples: 1,
						}, basic_trans_normal_buf: {
							load: Clear,
							store: Store,
							format: vulkano::format::Format::R16G16B16A16Sfloat,
							samples: 1,
						}
					}, passes: [
						{
							color: [basic_color_buf, basic_normal_buf],
							depth_stencil: {depth},
							input: []
						}, {
							color: [basic_trans_color_buf, basic_trans_normal_buf],
							depth_stencil: {trans_depth},
							input: []
						}
					]
				).unwrap()
			);
			
			let r_pass15 = Arc::new(ordered_passes_renderpass!(self.device.clone(),
					attachments: {
						shadow: {
							load: Clear,
							store: Store,
							format: DEPTH_FORMAT,
							samples: 1,
						}
					}, passes: [
						{
							color: [],
							depth_stencil: {shadow},
							input: []
						}
					]
				).unwrap()
			);
			
			let r_pass2 = Arc::new(
				ordered_passes_renderpass!(self.device.clone(),
					attachments: {
						deferred_color_buf: {
							load: Clear,
							store: Store,
							format: COLOR_FORMAT,
							samples: 1,
						}, itf_color_buf: {
							load: Clear,
							store: Store,
							format: COLOR_FORMAT,
							samples: 1,
						}, itf_depth_buf: {
							load: Clear,
							store: Store,
							format: DEPTH_FORMAT,
							samples: 1,
						}
					}, passes: [
						{
							color: [deferred_color_buf],
							depth_stencil: {},
							input: []
						}, {
							color: [itf_color_buf],
							depth_stencil: {itf_depth_buf},
							input: []
						}
					]
				).unwrap()
			);
			
			let r_pass3 = Arc::new(
				ordered_passes_renderpass!(self.device.clone(),
					attachments: {
						color: {
							load: Clear,
							store: Store,
							format: swapchain.format(),
							samples: 1,
						}
					}, passes: [
						{
							color: [color],
							depth_stencil: {},
							input: []
						}
					]
				).unwrap()
			);
			
			let show_triangles = self.show_triangles.load(atomic::Ordering::Relaxed);
			
			let mut pipeline_basic = GraphicsPipeline::start()
				.vertex_input(SingleBufferDefinition::new())
				.vertex_shader(vs.main_entry_point(), ())
				.triangle_list()
				.viewports(::std::iter::once(Viewport {
					origin: [0.0, 0.0],
					depth_range: 0.0 .. 1.0,
					dimensions: [images[0].dimensions()[0] as f32, images[0].dimensions()[1] as f32],
				}))
				.fragment_shader(fs.main_entry_point(), ())
				.depth_stencil_simple_depth()
				//.cull_mode_back()
				.render_pass(Subpass::from(r_pass1.clone(), 0).unwrap())
			; if show_triangles {
				pipeline_basic = pipeline_basic.polygon_mode_line();
			} else {
				pipeline_basic = pipeline_basic.polygon_mode_fill();
			}
			
			let pipeline_basic = Arc::new(pipeline_basic.build(self.device.clone()).unwrap());
	
			//use vulkano::pipeline::blend::BlendOp;
			//use vulkano::pipeline::blend::BlendFactor;
			
			let pipeline_basic_trans = Arc::new(GraphicsPipeline::start()
				.vertex_input(SingleBufferDefinition::new())
				.vertex_shader(vs.main_entry_point(), ())
				.triangle_list()
				.viewports(::std::iter::once(Viewport {
					origin: [0.0, 0.0],
					depth_range: 0.0 .. 1.0,
					dimensions: [images[0].dimensions()[0] as f32, images[0].dimensions()[1] as f32],
				}))
				.fragment_shader(fs.main_entry_point(), ())
				.depth_stencil_simple_depth()
				.polygon_mode_fill()
				/*.blend_collective(vulkano::pipeline::blend::AttachmentBlend {
					enabled: true,
					color_op: BlendOp::Add,
					color_source: BlendFactor::SrcAlpha,
					color_destination: BlendFactor::OneMinusSrcAlpha,
					alpha_op: BlendOp::Add,
					alpha_source: BlendFactor::SrcAlpha,
					alpha_destination: BlendFactor::OneMinusSrcAlpha,
					mask_red: true,
					mask_green: true,
					mask_blue: true,
					mask_alpha: true
				})*/.render_pass(Subpass::from(r_pass1.clone(), 1).unwrap())
				.build(self.device.clone())
				.unwrap()
			);
			
			let pipeline_shadow = Arc::new(GraphicsPipeline::start()
				.vertex_input(SingleBufferDefinition::new())
				.vertex_shader(shadow_vs.main_entry_point(), ())
				.triangle_list()
				.viewports(::std::iter::once(Viewport {
					origin: [0.0, 0.0],
					depth_range: 0.0 .. 1.0,
					dimensions: [shadow_dims[0] as f32, shadow_dims[1] as f32],
				}))
				.fragment_shader(shadow_fs.main_entry_point(), ())
				.depth_stencil_simple_depth()
				.polygon_mode_fill()
				.render_pass(Subpass::from(r_pass15.clone(), 0).unwrap())
				.build(self.device.clone())
				.unwrap()
			);
			
			let mut basic_set_pool = FixedSizeDescriptorSetsPool::new(pipeline_basic.clone(), 0);
			let mut shadow_set_pool = FixedSizeDescriptorSetsPool::new(pipeline_shadow.clone(), 0);
	
			let pipeline_deferred = Arc::new(GraphicsPipeline::start()
				.vertex_input(SingleBufferDefinition::new())
				.vertex_shader(square_vs.main_entry_point(), ())
				.triangle_list()
				.viewports(::std::iter::once(Viewport {
					origin: [0.0, 0.0],
					depth_range: 0.0 .. 1.0,
					dimensions: [images[0].dimensions()[0] as f32, images[0].dimensions()[1] as f32],
				}))
				.fragment_shader(deferred_fs.main_entry_point(), ())
				.depth_stencil_disabled()
				.render_pass(Subpass::from(r_pass2.clone(), 0).unwrap())
				.build(self.device.clone()).unwrap()
			);
			
			let mut deferred_set_b1_pool = FixedSizeDescriptorSetsPool::new(pipeline_deferred.clone(), 1);
			
			let mut pipeline_itf = GraphicsPipeline::start()
				.vertex_input(SingleBufferDefinition::new())
				.vertex_shader(interface_vs.main_entry_point(), ())
				.triangle_list()
				.viewports(::std::iter::once(Viewport {
					origin: [0.0, 0.0],
					depth_range: 0.0 .. 1.0,
					dimensions: [images[0].dimensions()[0] as f32, images[0].dimensions()[1] as f32],
				}))
				.fragment_shader(interface_fs.main_entry_point(), ())
				//.depth_stencil_disabled()
				.blend_collective(vulkano::pipeline::blend::AttachmentBlend {
					alpha_op: vulkano::pipeline::blend::BlendOp::Max,
					.. vulkano::pipeline::blend::AttachmentBlend::alpha_blending()
				}).render_pass(Subpass::from(r_pass2.clone(), 1).unwrap())
			; if show_triangles {
				pipeline_itf = pipeline_itf.polygon_mode_line();
			} else {
				pipeline_itf = pipeline_itf.polygon_mode_fill();
			}
			
			let pipeline_itf = Arc::new(pipeline_itf.build(self.device.clone()).unwrap());
			let mut itf_set_pool = FixedSizeDescriptorSetsPool::new(pipeline_itf.clone(), 0);
			
			let pipeline_final = Arc::new(GraphicsPipeline::start()
				.vertex_input(SingleBufferDefinition::new())
				.vertex_shader(square_vs.main_entry_point(), ())
				.triangle_list()
				.viewports(::std::iter::once(Viewport {
					origin: [0.0, 0.0],
					depth_range: 0.0 .. 1.0,
					dimensions: [images[0].dimensions()[0] as f32, images[0].dimensions()[1] as f32],
				}))
				.fragment_shader(final_fs.main_entry_point(), ())
				.depth_stencil_disabled()
				.render_pass(Subpass::from(r_pass3.clone(), 0).unwrap())
				.build(self.device.clone()).unwrap()
			);
			
			let sampler = Sampler::simple_repeat_linear_no_mipmap(self.device.clone());
			
			let depth_sampler = Sampler::new(
				self.device.clone(),
				vulkano::sampler::Filter::Linear,
				vulkano::sampler::Filter::Linear,
				vulkano::sampler::MipmapMode::Linear,
				vulkano::sampler::SamplerAddressMode::ClampToEdge,
				vulkano::sampler::SamplerAddressMode::ClampToEdge,
				vulkano::sampler::SamplerAddressMode::ClampToEdge,
				0.0, 8.0, 0.0, 0.0
			).unwrap();
			
			let shadow_sampler = Sampler::compare(
				self.device.clone(),
				vulkano::sampler::Filter::Linear,
				vulkano::sampler::Filter::Linear,
				vulkano::sampler::MipmapMode::Linear,
				vulkano::sampler::SamplerAddressMode::ClampToEdge,
				vulkano::sampler::SamplerAddressMode::ClampToEdge,
				vulkano::sampler::SamplerAddressMode::ClampToEdge,
				0.0, 8.0, 0.0, 0.0,
				vulkano::pipeline::depth_stencil::Compare::LessOrEqual
			).unwrap();
			
			let set_deferred = Arc::new(PersistentDescriptorSet::start(pipeline_deferred.clone(), 0)
				.add_sampled_image(basic_color_buf.clone(), sampler.clone()).unwrap()
				.add_sampled_image(basic_normal_buf.clone(), sampler.clone()).unwrap()
				.add_sampled_image(basic_depth_buf.clone(), depth_sampler.clone()).unwrap()
				.add_sampled_image(basic_trans_color_buf.clone(), sampler.clone()).unwrap()
				.add_sampled_image(basic_trans_normal_buf.clone(), sampler.clone()).unwrap()
				.add_sampled_image(basic_trans_depth_buf.clone(), depth_sampler.clone()).unwrap()
				.add_sampled_image(shadow_depth_buf.clone(), shadow_sampler.clone()).unwrap()
				.add_sampled_image(skybox_tex.clone(), sampler.clone()).unwrap()
				.add_buffer(deferred_uni.clone()).unwrap()
				.build().unwrap()
			);
			
			let set_final = Arc::new(PersistentDescriptorSet::start(pipeline_final.clone(), 0)
				.add_sampled_image(deferred_color_buf.clone(), sampler.clone()).unwrap()
				.add_sampled_image(itf_color_buf.clone(), sampler.clone()).unwrap()
				.build().unwrap()
			);
			
			let fb_pass1 = images.iter().map(|_| {
				Arc::new(Framebuffer::start(r_pass1.clone())
					.add(basic_depth_buf.clone()).unwrap()
					.add(basic_color_buf.clone()).unwrap()
					.add(basic_normal_buf.clone()).unwrap()
					.add(basic_trans_depth_buf.clone()).unwrap()
					.add(basic_trans_color_buf.clone()).unwrap()
					.add(basic_trans_normal_buf.clone()).unwrap()
					.build().unwrap()
				)
			}).collect::<Vec<_>>();
			
			let fb_pass15 = images.iter().map(|_| {
				Arc::new(Framebuffer::start(r_pass15.clone())
					.add(shadow_depth_buf.clone()).unwrap()
					.build().unwrap()
				)
			}).collect::<Vec<_>>();
			
			let fb_pass2 = images.iter().map(|_| {
				Arc::new(Framebuffer::start(r_pass2.clone())
					.add(deferred_color_buf.clone()).unwrap()
					.add(itf_color_buf.clone()).unwrap()
					.add(itf_depth_buf.clone()).unwrap()
					.build().unwrap()
				)
			}).collect::<Vec<_>>();
			
			let fb_pass3 = images.iter().map(|image| {
				Arc::new(Framebuffer::start(r_pass3.clone())
					.add(image.clone()).unwrap()
					.build().unwrap()
				)
			}).collect::<Vec<_>>();
			
			let mut previous_frame = Box::new(vulkano::sync::now(self.device.clone())) as Box<GpuFuture>;
			let mut fps_avg = VecDeque::new();
			let mut first = true;

			loop {
				previous_frame.cleanup_finished();
				
				for cmd in self.atlas_ref().update(self.device(), self.graphics_queue()).into_iter() {
					itf_cmds.push(Arc::new(cmd));
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
				
				if depth_read_queue.len() >= depth_queue_size {
					use cgmath;
					use cgmath::InnerSpace;
				
					let buf: Arc<vulkano::buffer::cpu_access::CpuAccessibleBuffer<[f32]>> = depth_read_queue.pop_front().unwrap();
					let read_res: Result<vulkano::buffer::cpu_access::ReadLock<[f32]>, vulkano::buffer::cpu_access::ReadLockError> = buf.read();
					
					match read_res {
						Ok(read_res) => {
							let cxi = win_size_x / 2;
							let cyi = win_size_y / 2;
							let cd = read_res[((cyi * win_size_x) + cxi) as usize];
							let (cx, cy, cz) = self.camera.screen_to_world(cxi, cyi, cd, win_size_x, win_size_y);
							
							let xpxi = cxi + 1;
							let xpyi = cyi;
							let xpd = read_res[((xpyi * win_size_x) + xpxi) as usize];
							let (xpx, xpy, xpz) = self.camera.screen_to_world(xpxi, xpyi, xpd, win_size_x, win_size_y);
							
							let ypxi = cxi;
							let ypyi = cyi + 1;
							let ypd = read_res[((ypyi * win_size_x) + ypxi) as usize];
							let (ypx, ypy, ypz) = self.camera.screen_to_world(ypxi, ypyi, ypd, win_size_x, win_size_y);
							
							let p1 = cgmath::Vector3::new(ypx, ypy, ypz);
							let p2 = cgmath::Vector3::new(cx, cy, cz);
							let p3 = cgmath::Vector3::new(xpx, xpy, xpz);
							let u = p2 - p1;
							let v = p3 - p1;
							let mut normal = cgmath::Vector3::new((u.y * v.z) - (u.z * v.y), (u.z * v.x) - (u.x * v.z), (u.x * v.y) - (u.y * v.x));
							normal = normal.normalize();
							
							self.mouse.set_center_world_pos(cx, cy, cz, normal.x, normal.y, normal.z);
							depth_write_queue.push_back(buf.clone());
						}, Err(_) => {
							depth_read_queue.push_front(buf.clone());
						
							depth_write_queue.push_back(CpuAccessibleBuffer::from_iter(
								self.device.clone(), BufferUsage::all(),
								(0 .. images[0].dimensions()[0] * images[0].dimensions()[1]).map(|_| 0.0_f32)
							).unwrap());
							
							println!("Failed to read from depth buffer. Increasing queue length. RQ Len: {}, WQ Len: {}", depth_read_queue.len(), depth_write_queue.len());
						}
					};
				}
				
				self.camera.process_queue();
				
				let uni_sub_buf = {
					let data = vs::ty::Data {
						view : self.camera.view_matrix().into(),
						proj : self.camera.projection_matrix(win_size_x, win_size_y).into(),
					}; uniform_buffer.next(data).unwrap()
				};
				
				let shadow_uni_sub_buf = {
					let data = shadow_vs::ty::Data {
						vp : self.camera.shadow_vp(win_size_x, win_size_y).into(),
					}; shadow_uniform_buffer.next(data).unwrap()
				};
				
				if self.force_resize.swap(false, atomic::Ordering::Relaxed) {
					resized = true;
					continue 'resize;
				}
		
				let (image_num, acquire_future) = match swapchain::acquire_next_image(swapchain.clone(),None) {
					Ok(ok) => ok,
					Err(e) => {
							
						println!("swapchain::acquire_next_image() Err: {:?}", e);
						resized = true;
						continue 'resize;
					}
				};
		
				let mut cmd_buf = AutoCommandBufferBuilder::primary_one_time_submit(self.device.clone(), self.graphics_queue.family()).unwrap();
				
				if !first {
					if depth_write_queue.len() > 0 {	
						let buf = depth_write_queue.pop_front().unwrap();
						cmd_buf = cmd_buf.copy_image_to_buffer(basic_depth_buf.clone(), buf.clone()).unwrap();
						depth_read_queue.push_back(buf);
					}
				} else {
					first = false;
				}
				
				cmd_buf = cmd_buf.begin_render_pass(
					fb_pass1[image_num].clone(),
					false,
					clear_pass1.clone()
				).unwrap();
				
				let mut triangles = 0;
				let mut remove_bufs = Vec::new();
				
				let buffers: Vec<_> = self.buffers_.lock().0.iter().filter_map(|v| {
					let upgraded = v.1.upgrade();
					
					if upgraded.is_none() {
						remove_bufs.push(v.0.clone());
					}
					
					upgraded
				}).collect();
				
				self.delete_buffer_list(remove_bufs);
				
				for buffer in &buffers {
					triangles += buffer.triangles();
					
					if let Some((model, verts_bufs, _)) = buffer.draw(self.device.clone(), self.transfer_queue.clone()) {
						for (image, sampler, vert_buf) in verts_bufs {
							let set = Arc::new(basic_set_pool.next()
								.add_buffer(uni_sub_buf.clone()).unwrap()
								.add_buffer(model.clone()).unwrap()
								.add_sampled_image(image, sampler).unwrap()
								.build().unwrap()
							); cmd_buf = cmd_buf.draw(
								pipeline_basic.clone(),
								&vulkano::command_buffer::DynamicState::none(),
								vert_buf,
								set.clone(), ()
							).unwrap()
						}
					}
				}
				
				cmd_buf = cmd_buf.next_subpass(false).unwrap();
				
				for buffer in &buffers {
					triangles += buffer.triangles();
					
					if let Some((model, _, trans_verts_bufs)) = buffer.draw(self.device.clone(), self.transfer_queue.clone()) {
						for (image, sampler, vert_buf) in trans_verts_bufs {
							let set = Arc::new(basic_set_pool.next()
								.add_buffer(uni_sub_buf.clone()).unwrap()
								.add_buffer(model.clone()).unwrap()
								.add_sampled_image(image, sampler).unwrap()
								.build().unwrap()
							); cmd_buf = cmd_buf.draw(
								pipeline_basic_trans.clone(),
								&vulkano::command_buffer::DynamicState::none(),
								vert_buf,
								set.clone(), ()
							).unwrap()
						}
					}
				}
				
				cmd_buf = cmd_buf.end_render_pass().unwrap().begin_render_pass(
					fb_pass15[image_num].clone(),
					false,
					clear_pass15.clone()
				).unwrap();
				
				if *self.shadow_state.lock() != 0 {
					for buffer in buffers {
						if let Some((model, verts_bufs, _)) = buffer.draw(self.device.clone(), self.transfer_queue.clone()) {
							for (_, _, vert_buf) in verts_bufs {
								let set = Arc::new(shadow_set_pool.next()
									.add_buffer(shadow_uni_sub_buf.clone()).unwrap()
									.add_buffer(model.clone()).unwrap()
									.build().unwrap()
								); cmd_buf = cmd_buf.draw(
									pipeline_shadow.clone(),
									&vulkano::command_buffer::DynamicState::none(),
									vert_buf,
									set.clone(), ()
								).unwrap()
							}
						}
					}
				}
				
				self.triangles.store(triangles, atomic::Ordering::Relaxed);
				let ssao_settings = self.ssao_settings.lock().clone();
				
				let deferred_uni_other = deferred_uni_other_pool.next(deferred_fs::ty::Other {
					win_size_x: win_size_x as i32,
					win_size_y: win_size_y as i32,
					aspect_ratio: (win_size_x as f32 / win_size_y as f32),		
					view: self.camera.view_matrix().into(),
					inverse_view: self.camera.view_matrix().invert().unwrap().into(),
					projection: self.camera.projection_matrix(win_size_x, win_size_y).into(),
					inverse_projection: self.camera.projection_matrix(win_size_x, win_size_y).invert().unwrap().into(),	
					shadow_vp: self.camera.shadow_vp(win_size_x, win_size_y).into(),
					shadow_state: self.shadow_state.lock().clone(),
					sun_direction: self.camera.sun_direction().into(),
					samples: ssao_settings.samples,
					strength: ssao_settings.strength,
					sample_scale: ssao_settings.sample_scale,
					range: ssao_settings.range,
					_dummy0: [0; 4],
					_dummy1: [0; 12],
				}).unwrap();
				
				let deferred_set_b1 = deferred_set_b1_pool.next()
					.add_buffer(deferred_uni_other).unwrap()
					.build().unwrap()
				;
			
				let mut cmd_buf = cmd_buf.end_render_pass().unwrap().begin_render_pass(
					fb_pass2[image_num].clone(),
					false,
					vec![
						[0.0, 0.0, 1.0, 1.0].into(),
						[0.0, 0.0, 0.0, 0.0].into(),
						(1.0).into(),
					]
				).unwrap().draw(
					pipeline_deferred.clone(),
					&command_buffer::DynamicState::none(),
					square_buf.clone(),
					(set_deferred.clone(), deferred_set_b1), ()
				).unwrap();
				
				cmd_buf = cmd_buf.next_subpass(false).unwrap();
				
				for (vert_buf, atlas_img, img_sampler, range_op) in self.interface.draw_bufs([win_size_x, win_size_y], resized) {
					let set_itf = Arc::new(itf_set_pool.next()
						.add_sampled_image(atlas_img, img_sampler).unwrap()
						.build().unwrap()
					); 
					
					match range_op {
						Some((min, max)) => {
							cmd_buf = cmd_buf.draw_vertex_range(
								pipeline_itf.clone(),
								command_buffer::DynamicState::none(),
								vert_buf,
								set_itf, (), min as u32, max as u32
							).unwrap();
						}, None => {
							cmd_buf = cmd_buf.draw(
								pipeline_itf.clone(),
								&command_buffer::DynamicState::none(),
								vert_buf,
								set_itf, ()
							).unwrap();
						}
					}
				}
				
				let cmd_buf = cmd_buf.end_render_pass().unwrap().begin_render_pass(
					fb_pass3[image_num].clone(),
					false,
					vec![
						[0.0, 0.0, 1.0, 1.0].into(),
					]
				).unwrap().draw(
					pipeline_final.clone(),
					&command_buffer::DynamicState::none(),
					square_buf.clone(),
					set_final.clone(), ()
				).unwrap().end_render_pass().unwrap().build().unwrap();
				
				let mut future: Box<GpuFuture> = Box::new(previous_frame.join(acquire_future)) as Box<_>;
				
				for cmd in itf_cmds.clone() {
					future = Box::new(future.then_execute(self.graphics_queue.clone(), cmd).unwrap()) as Box<_>;
				}
				
				let future = match future.then_execute(self.graphics_queue.clone(), cmd_buf).unwrap()
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
				itf_cmds.clear();
				previous_frame = Box::new(future) as Box<_>;
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
	
	pub fn app_loop(&self) -> Result<(), String> {
		let mut win_size_x;
		let mut win_size_y;
		
		let interface_vs = interface_vs::Shader::load(self.device.clone()).expect("failed to create shader module");
		let interface_fs = interface_fs::Shader::load(self.device.clone()).expect("failed to create shader module");

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
		
		let mut itf_cmds = Vec::new();
		
		'resize: loop {
			let [x, y] = self.surface.capabilities(PhysicalDevice::from_index(
				self.surface.instance(), self.pdevi).unwrap()).unwrap().current_extent.unwrap();
			win_size_x = x;
			win_size_y = y;
		
			if swapchain_.is_none() {
				let present_mode = match ITF_VSYNC {
					true => swapchain::PresentMode::Relaxed,
					false => swapchain::PresentMode::Immediate
				};
			
				swapchain_ = Some(Swapchain::new(
					self.device.clone(), self.surface.clone(),
					self.swap_caps.min_image_count, swapchain_format,
					self.swap_caps.current_extent.unwrap(), 1, self.swap_caps.supported_usage_flags,
					&self.graphics_queue, swapchain::SurfaceTransform::Identity,
					swapchain::CompositeAlpha::Opaque, present_mode,
					true, None
				).expect("failed to create swapchain"))
			} else {
				let swapchain = swapchain_.as_mut().unwrap();
				*swapchain = match swapchain.0.recreate_with_dimension([x, y]) {
					Ok(ok) => ok,
					Err(e) => {
						println!("swapchain recreation error: {:?}", e);
						continue;
					}
				};
			}
			
			let (swapchain, images) = (&swapchain_.as_ref().unwrap().0, &swapchain_.as_ref().unwrap().1);
			let (rpass1_clear_vals, rpass1, fb_pass1) = if ITF_MSAA > 1 {
				let itf_depth_buf = AttachmentImage::transient_multisampled(
					self.device.clone(),
					images[0].dimensions(),
					ITF_MSAA,
					DEPTH_FORMAT
				).unwrap();
				
				let itf_msaa_color_buf = AttachmentImage::transient_multisampled(
					self.device.clone(),
					images[0].dimensions(),
					ITF_MSAA,
					swapchain.format()
				).unwrap();
				
				let rpass1_clear_vals = vec![
					[1.0, 1.0, 1.0, 1.0].into(),
					[1.0, 1.0, 1.0, 1.0].into(),
					(1.0).into()
				];
				
				let rpass1 = Arc::new(
					single_pass_renderpass!(self.device.clone(),
						attachments: {
							itf_msaa_color_buf: {
								load: Clear,
								store: Store,
								format: swapchain.format(),
								samples: ITF_MSAA,
							}, itf_color_buf: {
								load: Clear,
								store: Store,
								format: swapchain.format(),
								samples: 1,
							}, itf_depth_buf: {
								load: Clear,
								store: Store,
								format: DEPTH_FORMAT,
								samples: ITF_MSAA,
							}
						}, pass: {
							color: [itf_msaa_color_buf],
							depth_stencil: {itf_depth_buf},
							resolve: [itf_color_buf]
						}
					).unwrap()
				) as Arc<vulkano::framebuffer::RenderPassAbstract + Send + Sync>;
				
				let fb_pass1 = images.iter().map(|image| {
					Arc::new(Framebuffer::start(rpass1.clone())
						.add(itf_msaa_color_buf.clone()).unwrap()
						.add(image.clone()).unwrap()
						.add(itf_depth_buf.clone()).unwrap()
						.build().unwrap()
					) as Arc<vulkano::framebuffer::FramebufferAbstract + Send + Sync>
				}).collect::<Vec<_>>();
				
				(rpass1_clear_vals, rpass1, fb_pass1)
			} else {
				let itf_depth_buf = AttachmentImage::with_usage(
					self.device.clone(), images[0].dimensions(),
					DEPTH_FORMAT,
					vulkano::image::ImageUsage {
						depth_stencil_attachment: true,
						transfer_source: true,
						sampled: true,
						.. vulkano::image::ImageUsage::none()
					}
				).unwrap();
				
				let rpass1_clear_vals = vec![
					[1.0, 1.0, 1.0, 1.0].into(),
					(1.0).into()
				];
				
				let rpass1 = Arc::new(
					single_pass_renderpass!(self.device.clone(),
						attachments: {
							itf_color_buf: {
								load: Clear,
								store: Store,
								format: swapchain.format(),
								samples: 1,
							}, itf_depth_buf: {
								load: Clear,
								store: Store,
								format: DEPTH_FORMAT,
								samples: 1,
							}
						}, pass: {
							color: [itf_color_buf],
							depth_stencil: {itf_depth_buf},
							resolve: []
						}
					).unwrap()
				) as Arc<vulkano::framebuffer::RenderPassAbstract + Send + Sync>;
				
				let fb_pass1 = images.iter().map(|image| {
					Arc::new(Framebuffer::start(rpass1.clone())
						.add(image.clone()).unwrap()
						.add(itf_depth_buf.clone()).unwrap()
						.build().unwrap()
					) as Arc<vulkano::framebuffer::FramebufferAbstract + Send + Sync>
				}).collect::<Vec<_>>();
				
				(rpass1_clear_vals, rpass1, fb_pass1)
			};
			
			let show_triangles = self.show_triangles.load(atomic::Ordering::Relaxed);
			let mut pipeline_itf = GraphicsPipeline::start()
				.vertex_input(SingleBufferDefinition::new())
				.vertex_shader(interface_vs.main_entry_point(), ())
				.triangle_list()
				.depth_stencil_disabled()
				.viewports(::std::iter::once(Viewport {
					origin: [0.0, 0.0],
					depth_range: 0.0 .. 1.0,
					dimensions: [images[0].dimensions()[0] as f32, images[0].dimensions()[1] as f32],
				}))
				.fragment_shader(interface_fs.main_entry_point(), ())
				.depth_stencil_simple_depth()
				.blend_collective(vulkano::pipeline::blend::AttachmentBlend {
					alpha_op: vulkano::pipeline::blend::BlendOp::Max,
					.. vulkano::pipeline::blend::AttachmentBlend::alpha_blending()
				}).render_pass(Subpass::from(rpass1.clone(), 0).unwrap())
			; if show_triangles {
				pipeline_itf = pipeline_itf.polygon_mode_line();
			} else {
				pipeline_itf = pipeline_itf.polygon_mode_fill();
			}
			
			let pipeline_itf = Arc::new(pipeline_itf.build(self.device.clone()).unwrap());
			let mut itf_set_pool = FixedSizeDescriptorSetsPool::new(pipeline_itf.clone(), 0);
			
			let mut previous_frame = Box::new(vulkano::sync::now(self.device.clone())) as Box<GpuFuture>;
			let mut fps_avg = VecDeque::new();
			
			loop {
				previous_frame.cleanup_finished();
				
				for cmd in self.atlas_ref().update(self.device(), self.graphics_queue()).into_iter() {
					itf_cmds.push(Arc::new(cmd));
				}
				
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
								let bs_size = winit::dpi::PhysicalSize::new(w as f64, h as f64).to_logical(self.surface.window().get_hidpi_factor());
								self.surface.window().set_inner_size(bs_size);
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
		
				let (image_num, acquire_future) = match swapchain::acquire_next_image(swapchain.clone(),None) {
					Ok(ok) => ok,
					Err(e) => {
						println!("swapchain::acquire_next_image() Err: {:?}", e);
						resized = true;
						continue 'resize;
					}
				};
		
				let mut cmd_buf = AutoCommandBufferBuilder::primary_one_time_submit(self.device.clone(), self.graphics_queue.family()).unwrap();
				
				cmd_buf = cmd_buf.begin_render_pass(
					fb_pass1[image_num].clone(),
					false,
					rpass1_clear_vals.clone()
				).unwrap();
				
				for (vert_buf, atlas_img, img_sampler, range_op) in self.interface.draw_bufs([win_size_x, win_size_y], resized) {
					let set_itf = Arc::new(itf_set_pool.next()
						.add_sampled_image(atlas_img, img_sampler).unwrap()
						.build().unwrap()
					);
					
					match range_op {
						Some((min, max)) => {
							cmd_buf = cmd_buf.draw_vertex_range(
								pipeline_itf.clone(),
								command_buffer::DynamicState::none(),
								vert_buf,
								set_itf, (), min as u32, max as u32
							).unwrap();
						}, None => {
							cmd_buf = cmd_buf.draw(
								pipeline_itf.clone(),
								&command_buffer::DynamicState::none(),
								vert_buf,
								set_itf, ()
							).unwrap();
						}
					}
				}	
				
				let cmd_buf = cmd_buf.end_render_pass().unwrap().build().unwrap();
				let mut future: Box<GpuFuture> = Box::new(previous_frame.join(acquire_future)) as Box<_>;
				
				for cmd in itf_cmds.clone() {
					future = Box::new(future.then_execute(self.graphics_queue.clone(), cmd).unwrap()) as Box<_>;
				}
				
				let future = match future.then_execute(self.graphics_queue.clone(), cmd_buf).unwrap()
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
				
				itf_cmds.clear();
				future.wait(None).unwrap();
				previous_frame = Box::new(future) as Box<_>;
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

