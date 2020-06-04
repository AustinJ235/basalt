#![feature(arbitrary_self_types)]
#![feature(integer_atomics)]
#![feature(optin_builtin_traits)]
#![feature(drain_filter)]

extern crate winit;
#[macro_use]
pub extern crate vulkano;
#[macro_use]
extern crate vulkano_shaders;
extern crate arc_swap;
extern crate crossbeam;
pub extern crate ilmenite;
extern crate image;
extern crate num_cpus;
extern crate ordered_float;
extern crate parking_lot;

pub mod atlas;
pub mod input;
pub mod interface;
pub mod misc;
pub mod shaders;
pub mod window;

use atlas::Atlas;
use input::Input;
use interface::{bin::BinUpdateStats, interface::Interface};
use parking_lot::{Condvar, Mutex};
use std::{
	collections::VecDeque,
	mem::MaybeUninit,
	sync::{
		atomic::{self, AtomicBool, AtomicUsize},
		Arc,
	},
	thread,
	thread::JoinHandle,
	time::{Duration, Instant},
};
use vulkano::{
	command_buffer::AutoCommandBufferBuilder,
	device::{self, Device, DeviceExtensions},
	image::ImageUsage,
	instance::{Instance, InstanceExtensions, PhysicalDevice, PhysicalDeviceType},
	swapchain::{self, ColorSpace, Surface, Swapchain, SwapchainCreationError},
	sync::GpuFuture,
};
use window::BasaltWindow;

const SHOW_SWAPCHAIN_WARNINGS: bool = true;

/// Options for Basalt's creation and operation.
#[derive(Debug, Clone)]
pub struct Options {
	ignore_dpi: bool,
	window_size: [u32; 2],
	title: String,
	scale: f32,
	app_loop: bool,
	exclusive_fullscreen: bool,
	itf_limit_draw: bool,
	prefer_integrated_gpu: bool,
	instance_extensions: InstanceExtensions,
	device_extensions: DeviceExtensions,
}

impl Default for Options {
	fn default() -> Self {
		Options {
			ignore_dpi: false,
			window_size: [1920, 1080],
			title: "vk-basalt".to_string(),
			scale: 1.0,
			app_loop: false,
			itf_limit_draw: true,
			exclusive_fullscreen: false,
			prefer_integrated_gpu: false,
			instance_extensions: {
				let ideal = InstanceExtensions {
					khr_surface: true,
					khr_xlib_surface: true,
					khr_xcb_surface: true,
					khr_wayland_surface: true,
					khr_android_surface: true,
					khr_win32_surface: true,
					mvk_ios_surface: true,
					mvk_macos_surface: true,
					khr_get_physical_device_properties2: true,
					khr_get_surface_capabilities2: true,
					..InstanceExtensions::none()
				};

				match InstanceExtensions::supported_by_core() {
					Ok(supported) => supported.intersection(&ideal),
					Err(_) => InstanceExtensions::none(),
				}
			},
			device_extensions: DeviceExtensions {
				khr_swapchain: true,
				// ext_full_screen_exclusive: true,
				khr_storage_buffer_storage_class: true,
				..DeviceExtensions::none()
			},
		}
	}
}

impl Options {
	/// Configure Basalt to run in app mode. The swapchain will be managed by Basalt and all
	/// renderering to the swapchain will be done by Basalt. Additional rendering to the
	/// swapchain will be unavailable. This is useful for applications that are UI only.
	pub fn app_loop(mut self) -> Self {
		self.app_loop = true;
		self
	}

	/// Defaults to `true`. Limits interface redraws where possible. In the app loop the
	/// application will only render frames when there are updates. In an external loop when
	/// `ItfRenderer` is not rendering to the swapchain directly it will avoid redrawing to
	/// the interface image if there are no updates needed.
	pub fn interface_limit_draw(mut self, to: bool) -> Self {
		self.itf_limit_draw = to;
		self
	}

	/// Defaults to `false`. Enables the device extension required for exclusive fullscreen.
	/// Generally this extension is only present on Windows. Basalt will return an error upon
	/// creation if this feature isn't supported. With this option enabled
	/// ``BasaltWindow::enable_fullscreen()`` will use exclusive fullscreen; otherwise,
	/// borderless window will be used.
	pub fn use_exclusive_fullscreen(mut self, to: bool) -> Self {
		self.exclusive_fullscreen = to;
		self.device_extensions.ext_full_screen_exclusive = true;
		self
	}

	/// Defaults to `false`. Ignore dpi hints provided by the platform.
	pub fn ignore_dpi(mut self, to: bool) -> Self {
		self.ignore_dpi = to;
		self
	}

	/// Set the inner size of the window to be created
	pub fn window_size(mut self, width: u32, height: u32) -> Self {
		self.window_size = [width, height];
		self
	}

	/// Set the title of the window to be created
	pub fn title<T: AsRef<str>>(mut self, title: T) -> Self {
		self.title = String::from(title.as_ref());
		self
	}

	/// Set the initial scale of the UI
	pub fn scale(mut self, to: f32) -> Self {
		self.scale = to;
		self
	}

	/// Prefer integrated graphics if they are available
	pub fn prefer_integrated_gpu(mut self) -> Self {
		self.prefer_integrated_gpu = true;
		self
	}

	/// Add additional instance extensions
	pub fn instance_ext_union(mut self, ext: &InstanceExtensions) -> Self {
		self.instance_extensions = self.instance_extensions.union(ext);
		self
	}

	/// Add additional device extensions
	pub fn device_ext_union(mut self, ext: &DeviceExtensions) -> Self {
		self.device_extensions = self.device_extensions.union(ext);
		self
	}
}

/// Device limitations
#[derive(Debug)]
pub struct Limits {
	pub max_image_dimension_2d: u32,
	pub max_image_dimension_3d: u32,
}

struct Initials {
	device: Arc<Device>,
	graphics_queue: Arc<device::Queue>,
	transfer_queue: Arc<device::Queue>,
	compute_queue: Arc<device::Queue>,
	secondary_graphics_queue: Option<Arc<device::Queue>>,
	secondary_transfer_queue: Option<Arc<device::Queue>>,
	secondary_compute_queue: Option<Arc<device::Queue>>,
	surface: Arc<Surface<Arc<dyn BasaltWindow + Send + Sync>>>,
	swap_caps: swapchain::Capabilities,
	limits: Arc<Limits>,
	pdevi: usize,
	window_size: [u32; 2],
	bin_stats: bool,
	options: Options,
}

impl Initials {
	pub fn use_first_device(
		options: Options,
		result_fn: Box<dyn Fn(Result<Arc<Basalt>, String>) + Send + Sync>,
	) {
		let mut device_num: Option<usize> = None;
		let mut show_devices = false;
		let mut bin_stats = false;

		for arg in ::std::env::args() {
			if arg.starts_with("--use-device=") {
				let split_by_eq: Vec<_> = arg.split("=").collect();

				if split_by_eq.len() < 2 {
					println!("Incorrect '--use-device' usage. Example: '--use-device=2'");
					break;
				} else {
					device_num = Some(match split_by_eq[1].parse() {
						Ok(ok) => ok,
						Err(_) => {
							println!(
								"Incorrect '--use-device' usage. Example: '--use-device=2'"
							);
							continue;
						},
					});

					println!("Using device: {}", device_num.as_ref().unwrap());
				}
			} else if arg.starts_with("--show-devices") {
				show_devices = true;
			} else if arg.starts_with("--binstats") {
				bin_stats = true;
			}
		}

		let instance = match Instance::new(None, &options.instance_extensions, None)
			.map_err(|e| format!("Failed to create instance: {}", e))
		{
			Ok(ok) => ok,
			Err(e) => return result_fn(Err(e)),
		};

		window::open_surface(
			options.clone(),
			instance.clone(),
			Box::new(move |surface_result| {
				let surface = match surface_result {
					Ok(ok) => ok,
					Err(e) =>
						return result_fn(Err(format!("Failed to create surface: {}", e))),
				};

				let physical_devices: Vec<_> = PhysicalDevice::enumerate(&instance).collect();

				if show_devices {
					println!("Devices:");

					for (i, dev) in physical_devices.iter().enumerate() {
						println!(
							"  {}: {} | Type: {:?} | API: {}",
							i,
							dev.name(),
							dev.ty(),
							dev.api_version()
						);
					}
				}

				let physical_device = match device_num {
					Some(device_i) =>
						match physical_devices.get(device_i) {
							Some(some) => some,
							None =>
								return result_fn(Err(format!(
									"No device found at index {}.",
									device_i
								))),
						},
					None =>
						if options.prefer_integrated_gpu {
							let mut ranked: Vec<_> = physical_devices
								.iter()
								.map(|d| {
									(
										match d.ty() {
											PhysicalDeviceType::DiscreteGpu => 300,
											PhysicalDeviceType::IntegratedGpu => 400,
											PhysicalDeviceType::VirtualGpu => 200,
											PhysicalDeviceType::Other => 100,
											PhysicalDeviceType::Cpu => 0,
										} + physical_devices.len() - d.index(),
										d,
									)
								})
								.collect();

							ranked.sort_by_key(|k| k.0);

							match ranked.pop().ok_or("No suitable device found.") {
								Ok(ok) => ok.1,
								Err(e) => return result_fn(Err(e.to_string())),
							}
						} else {
							let mut ranked: Vec<_> = physical_devices
								.iter()
								.map(|d| {
									(
										match d.ty() {
											PhysicalDeviceType::DiscreteGpu => 400,
											PhysicalDeviceType::IntegratedGpu => 300,
											PhysicalDeviceType::VirtualGpu => 200,
											PhysicalDeviceType::Other => 100,
											PhysicalDeviceType::Cpu => 0,
										} + physical_devices.len() - d.index(),
										d,
									)
								})
								.collect();

							ranked.sort_by_key(|k| k.0);

							match ranked.pop().ok_or("No suitable device found.") {
								Ok(ok) => ok.1,
								Err(e) => return result_fn(Err(e.to_string())),
							}
						},
				};

				let mut queue_families: Vec<_> = physical_device
					.queue_families()
					.flat_map(|family| {
						(0..family.queues_count()).into_iter().map(move |_| family.clone())
					})
					.collect();

				let mut g_optimal: Vec<_> = queue_families
					.drain_filter(|family| {
						family.supports_graphics() && !family.supports_compute()
					})
					.collect();
				let mut c_optimal: Vec<_> = queue_families
					.drain_filter(|family| {
						family.supports_compute() && !family.supports_graphics()
					})
					.collect();
				let mut t_optimal: Vec<_> = queue_families
					.drain_filter(|family| {
						family.explicitly_supports_transfers()
							&& !family.supports_compute()
							&& !family.supports_graphics()
					})
					.collect();

				let (g_primary, mut g_secondary) = match g_optimal.len() {
					0 => {
						let mut g_suboptimal: Vec<_> = queue_families
							.drain_filter(|family| family.supports_graphics())
							.collect();

						match g_suboptimal.len() {
							0 =>
								return result_fn(Err(format!(
									"Unable to find queue family suitable for graphics."
								))),
							1 => (Some(g_suboptimal.pop().unwrap()), None),
							2 =>
								(
									Some(g_suboptimal.pop().unwrap()),
									Some(g_suboptimal.pop().unwrap()),
								),
							_ => {
								let ret = (
									Some(g_suboptimal.pop().unwrap()),
									Some(g_suboptimal.pop().unwrap()),
								);
								queue_families.append(&mut g_suboptimal);
								ret
							},
						}
					},
					1 => {
						let mut g_suboptimal: Vec<_> = queue_families
							.drain_filter(|family| family.supports_graphics())
							.collect();

						match g_suboptimal.len() {
							0 => (Some(g_optimal.pop().unwrap()), None),
							1 =>
								(
									Some(g_optimal.pop().unwrap()),
									Some(g_suboptimal.pop().unwrap()),
								),
							_ => {
								let ret = (
									Some(g_optimal.pop().unwrap()),
									Some(g_suboptimal.pop().unwrap()),
								);
								queue_families.append(&mut g_suboptimal);
								ret
							},
						}
					},
					2 => (Some(g_optimal.pop().unwrap()), Some(g_optimal.pop().unwrap())),
					_ => {
						let ret =
							(Some(g_optimal.pop().unwrap()), Some(g_optimal.pop().unwrap()));
						queue_families.append(&mut g_optimal);
						ret
					},
				};

				let (c_primary, mut c_secondary) = match c_optimal.len() {
					0 => {
						let mut c_suboptimal: Vec<_> = queue_families
							.drain_filter(|family| family.supports_compute())
							.collect();

						match c_suboptimal.len() {
							0 =>
								if g_secondary
									.as_ref()
									.map(|f| f.supports_compute())
									.unwrap_or(false)
								{
									(Some(g_secondary.take().unwrap()), None)
								} else {
									if !g_primary.as_ref().unwrap().supports_compute() {
										return result_fn(Err(format!(
											"Unable to find queue family suitable for compute."
										)));
									}

									(None, None)
								},
							1 => (Some(c_suboptimal.pop().unwrap()), None),
							2 =>
								(
									Some(c_suboptimal.pop().unwrap()),
									Some(c_suboptimal.pop().unwrap()),
								),
							_ => {
								let ret = (
									Some(c_suboptimal.pop().unwrap()),
									Some(c_suboptimal.pop().unwrap()),
								);
								queue_families.append(&mut c_suboptimal);
								ret
							},
						}
					},
					1 => {
						let mut c_suboptimal: Vec<_> = queue_families
							.drain_filter(|family| family.supports_compute())
							.collect();

						match c_suboptimal.len() {
							0 => (Some(c_optimal.pop().unwrap()), None),
							1 =>
								(
									Some(c_optimal.pop().unwrap()),
									Some(c_suboptimal.pop().unwrap()),
								),
							_ => {
								let ret = (
									Some(c_optimal.pop().unwrap()),
									Some(c_suboptimal.pop().unwrap()),
								);
								queue_families.append(&mut c_suboptimal);
								ret
							},
						}
					},
					2 => (Some(c_optimal.pop().unwrap()), Some(c_optimal.pop().unwrap())),
					_ => {
						let ret =
							(Some(c_optimal.pop().unwrap()), Some(c_optimal.pop().unwrap()));
						queue_families.append(&mut c_optimal);
						ret
					},
				};

				let (t_primary, t_secondary) = match t_optimal.len() {
					0 =>
						match queue_families.len() {
							0 =>
								match c_secondary.take() {
									Some(some) => (Some(some), None),
									None => (None, None),
								},
							1 => (Some(queue_families.pop().unwrap()), None),
							_ =>
								(
									Some(queue_families.pop().unwrap()),
									Some(queue_families.pop().unwrap()),
								),
						},
					1 =>
						match queue_families.len() {
							0 => (Some(t_optimal.pop().unwrap()), None),
							_ =>
								(
									Some(t_optimal.pop().unwrap()),
									Some(queue_families.pop().unwrap()),
								),
						},
					_ => (Some(t_optimal.pop().unwrap()), Some(t_optimal.pop().unwrap())),
				};

				let g_count: usize = 1 + g_secondary.as_ref().map(|_| 1).unwrap_or(0);
				let c_count: usize = c_primary.as_ref().map(|_| 1).unwrap_or(0)
					+ c_secondary.as_ref().map(|_| 1).unwrap_or(0);
				let t_count: usize = t_primary.as_ref().map(|_| 1).unwrap_or(0)
					+ t_secondary.as_ref().map(|_| 1).unwrap_or(0);
				let weight: f32 = 0.30 / (g_count + c_count + t_count - 1) as f32;

				println!("[Basalt]: VK Queues [{}/{}/{}]", g_count, c_count, t_count);

				let queue_request: Vec<_> = vec![
					(g_primary, 0.70),
					(g_secondary, weight),
					(c_primary, weight),
					(c_secondary, weight),
					(t_primary, weight),
					(t_secondary, weight),
				]
				.into_iter()
				.filter_map(|(v, w)| v.map(|v| (v, w)))
				.collect();

				let (device, mut queues) = match Device::new(
					*physical_device,
					physical_device.supported_features(),
					&options.device_extensions,
					queue_request.into_iter(),
				)
				.map_err(|e| format!("Failed to create device: {}", e))
				{
					Ok(ok) => ok,
					Err(e) => return result_fn(Err(e)),
				};

				let graphics_queue = match queues.next() {
					Some(some) => some,
					None =>
						return result_fn(Err(format!(
							"Expected primary graphics queue to be present."
						))),
				};

				let secondary_graphics_queue = if g_count == 2 {
					match queues.next() {
						Some(some) => Some(some),
						None =>
							return result_fn(Err(format!(
								"Expected secondary graphics queue to be present."
							))),
					}
				} else {
					None
				};

				let compute_queue = if c_count > 0 {
					match queues.next() {
						Some(some) => some,
						None =>
							return result_fn(Err(format!(
								"Expected primary compute queue to be present."
							))),
					}
				} else {
					println!(
						"[Basalt]: Warning graphics queue and compute queue are the same."
					);
					graphics_queue.clone()
				};

				let secondary_compute_queue = if c_count == 2 {
					match queues.next() {
						Some(some) => Some(some),
						None =>
							return result_fn(Err(format!(
								"Expected secondary compute queue to be present."
							))),
					}
				} else {
					None
				};

				let transfer_queue = if t_count > 0 {
					match queues.next() {
						Some(some) => some,
						None =>
							return result_fn(Err(format!(
								"Expected primary transfer queue to be present."
							))),
					}
				} else {
					println!(
						"[Basalt]: Warning compute queue and transfer queue are the same."
					);
					compute_queue.clone()
				};

				let secondary_transfer_queue = if t_count == 2 {
					match queues.next() {
						Some(some) => Some(some),
						None =>
							return result_fn(Err(format!(
								"Expected secondary transfer queue to be present."
							))),
					}
				} else {
					None
				};

				let swap_caps = match surface.capabilities(*physical_device) {
					Ok(ok) => ok,
					Err(e) =>
						return result_fn(Err(format!(
							"Failed to get surface capabilities: {}",
							e
						))),
				};

				let physical_device_limits = physical_device.limits();

				let limits = Arc::new(Limits {
					max_image_dimension_2d: physical_device_limits.max_image_dimension_2d(),
					max_image_dimension_3d: physical_device_limits.max_image_dimension_3d(),
				});

				let basalt = match Basalt::from_initials(Initials {
					device,
					graphics_queue,
					transfer_queue,
					compute_queue,
					secondary_graphics_queue,
					secondary_transfer_queue,
					secondary_compute_queue,
					surface,
					swap_caps,
					limits,
					pdevi: physical_device.index(),
					window_size: options.window_size,
					bin_stats,
					options: options.clone(),
				}) {
					Ok(ok) => ok,
					Err(e) =>
						return result_fn(Err(format!("Failed to initialize Basalt: {}", e))),
				};

				if options.app_loop {
					let bst = basalt.clone();
					*basalt.loop_thread.lock() = Some(thread::spawn(move || bst.app_loop()));
				}

				result_fn(Ok(basalt))
			}),
		)
	}
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SwapchainRecreateReason {
	Scale,
	Resize(u32, u32),
	Redraw,
	Properties,
	External,
	Exclusive(bool),
	ODBUpdated,
}

#[allow(dead_code)]
pub struct Basalt {
	device: Arc<Device>,
	graphics_queue: Arc<device::Queue>,
	transfer_queue: Arc<device::Queue>,
	compute_queue: Arc<device::Queue>,
	secondary_graphics_queue: Option<Arc<device::Queue>>,
	secondary_transfer_queue: Option<Arc<device::Queue>>,
	secondary_compute_queue: Option<Arc<device::Queue>>,
	surface: Arc<Surface<Arc<dyn BasaltWindow + Send + Sync>>>,
	swap_caps: swapchain::Capabilities,
	fps: AtomicUsize,
	interface: Arc<Interface>,
	atlas: Arc<Atlas>,
	input: Arc<Input>,
	wants_exit: AtomicBool,
	#[allow(dead_code)]
	limits: Arc<Limits>,
	loop_thread: Mutex<Option<JoinHandle<Result<(), String>>>>,
	pdevi: usize,
	vsync: Mutex<bool>,
	window_size: Mutex<[u32; 2]>,
	custom_scale: Mutex<f32>,
	options: Options,
	ignore_dpi_data: Mutex<Option<(usize, Instant, u32, u32)>>,
	bin_stats: bool,
	swapchain_recreate: Mutex<Vec<SwapchainRecreateReason>>,
	swapchain_recreate_cond: Condvar,
}

#[allow(dead_code)]
impl Basalt {
	/// Begin initializing Basalt, this thread will be taken for window event polling and the
	/// function provided in `result_fn` will be executed after Basalt initialization has
	/// completed or errored.
	pub fn initialize(
		options: Options,
		result_fn: Box<dyn Fn(Result<Arc<Self>, String>) + Send + Sync>,
	) {
		Initials::use_first_device(options, result_fn)
	}

	fn from_initials(initials: Initials) -> Result<Arc<Self>, String> {
		unsafe {
			let mut basalt_ret = Arc::new(Basalt {
				device: initials.device,
				graphics_queue: initials.graphics_queue,
				transfer_queue: initials.transfer_queue,
				compute_queue: initials.compute_queue,
				secondary_graphics_queue: initials.secondary_graphics_queue,
				secondary_transfer_queue: initials.secondary_transfer_queue,
				secondary_compute_queue: initials.secondary_compute_queue,
				surface: initials.surface,
				swap_caps: initials.swap_caps,
				fps: AtomicUsize::new(0),
				interface: { MaybeUninit::uninit() }.assume_init(),
				limits: initials.limits.clone(),
				atlas: { MaybeUninit::uninit() }.assume_init(),
				input: { MaybeUninit::uninit() }.assume_init(),
				wants_exit: AtomicBool::new(false),
				loop_thread: Mutex::new(None),
				pdevi: initials.pdevi,
				vsync: Mutex::new(true),
				window_size: Mutex::new(initials.window_size),
				custom_scale: Mutex::new(initials.options.scale),
				options: initials.options,
				ignore_dpi_data: Mutex::new(None),
				bin_stats: initials.bin_stats,
				swapchain_recreate: Mutex::new(Vec::new()),
				swapchain_recreate_cond: Condvar::new(),
			});

			let atlas_ptr = &mut Arc::get_mut(&mut basalt_ret).unwrap().atlas as *mut _;
			let interface_ptr = &mut Arc::get_mut(&mut basalt_ret).unwrap().interface as *mut _;
			let input_ptr = &mut Arc::get_mut(&mut basalt_ret).unwrap().input as *mut _;
			::std::ptr::write(atlas_ptr, Atlas::new(basalt_ret.clone()));
			::std::ptr::write(interface_ptr, Interface::new(basalt_ret.clone()));
			::std::ptr::write(input_ptr, Input::new(basalt_ret.clone()));
			basalt_ret.surface.window().attach_basalt(basalt_ret.clone());

			basalt_ret.input_ref().add_hook(
				input::InputHook::Press {
					global: false,
					keys: vec![input::Qwery::F1],
					mouse_buttons: Vec::new(),
				},
				Arc::new(move |_| {
					let mut output = String::new();
					output.push_str("-----[ Build in Basalt Bindings ]-----\r\n");
					output.push_str(" F1: Prints keys used by basalt\r\n");
					output.push_str(" F2: Prints fps while held (app_loop only)\r\n");
					output.push_str(" F3: Prints bin update stats\r\n");
					output.push_str(" F7: Decreases msaa level\r\n");
					output.push_str(" F8: Increases msaa level\r\n");
					output.push_str(" F10: Toggles vsync (app_loop only)\r\n");
					output.push_str(" F11: Toggles fullscreen\r\n");
					output.push_str(" LCtrl + Dash: Decreases ui scale\r\n");
					output.push_str(" LCtrl + Equal: Increaes ui scale\r\n");
					output.push_str("--------------------------------------");
					println!("{}", output);
					input::InputHookRes::Success
				}),
			);

			let basalt = basalt_ret.clone();
			basalt_ret.input_ref().add_hook(
				input::InputHook::Hold {
					global: false,
					keys: vec![input::Qwery::F2],
					mouse_buttons: Vec::new(),
					initial_delay: Duration::from_millis(0),
					interval: Duration::from_millis(100),
					accel: 0.0,
				},
				Arc::new(move |_| {
					println!("FPS: {}", basalt.fps());
					input::InputHookRes::Success
				}),
			);

			let basalt = basalt_ret.clone();
			basalt_ret.input_ref().add_hook(
				input::InputHook::Press {
					global: false,
					keys: vec![input::Qwery::F11],
					mouse_buttons: Vec::new(),
				},
				Arc::new(move |_| {
					basalt.surface.window().toggle_fullscreen();
					input::InputHookRes::Success
				}),
			);

			let basalt = basalt_ret.clone();
			basalt_ret.input_ref().add_hook(
				input::InputHook::Press {
					global: false,
					keys: vec![input::Qwery::F3],
					mouse_buttons: Vec::new(),
				},
				Arc::new(move |_| {
					let bins = basalt.interface_ref().bins();
					let count = bins.len();

					let sum =
						BinUpdateStats::sum(&bins.iter().map(|v| v.update_stats()).collect());

					let avg = sum.divide(count as f32);

					println!("Total Bins: {}", count);
					println!("Bin Update Time Sum: {:?}\r\n", sum);
					println!("Bin Update Time Average: {:?}\r\n", avg);
					input::InputHookRes::Success
				}),
			);

			let basalt = basalt_ret.clone();
			basalt_ret.input_ref().add_hook(
				input::InputHook::Press {
					global: false,
					keys: vec![input::Qwery::F7],
					mouse_buttons: Vec::new(),
				},
				Arc::new(move |_| {
					basalt.interface_ref().decrease_msaa();
					println!("MSAA set to {}X", basalt.interface_ref().msaa());
					input::InputHookRes::Success
				}),
			);

			let basalt = basalt_ret.clone();
			basalt_ret.input_ref().add_hook(
				input::InputHook::Press {
					global: false,
					keys: vec![input::Qwery::F8],
					mouse_buttons: Vec::new(),
				},
				Arc::new(move |_| {
					basalt.interface_ref().increase_msaa();
					println!("MSAA set to {}X", basalt.interface_ref().msaa());
					input::InputHookRes::Success
				}),
			);

			let basalt = basalt_ret.clone();
			basalt_ret.input_ref().add_hook(
				input::InputHook::Press {
					global: false,
					keys: vec![input::Qwery::F10],
					mouse_buttons: Vec::new(),
				},
				Arc::new(move |_| {
					let mut vsync = basalt.vsync.lock();
					*vsync = !*vsync;
					basalt.recreate_swapchain(SwapchainRecreateReason::Properties);

					if *vsync {
						println!("VSync Enabled!");
					} else {
						println!("VSync Disabled!");
					}

					input::InputHookRes::Success
				}),
			);

			let basalt = basalt_ret.clone();
			basalt_ret.input_ref().add_hook(
				input::InputHook::Press {
					global: false,
					keys: vec![input::Qwery::LCtrl, input::Qwery::Dash],
					mouse_buttons: Vec::new(),
				},
				Arc::new(move |_| {
					basalt.add_scale(-0.05);
					println!("Current Scale: {:.1} %", basalt.current_scale() * 100.0);
					input::InputHookRes::Success
				}),
			);

			let basalt = basalt_ret.clone();
			basalt_ret.input_ref().add_hook(
				input::InputHook::Press {
					global: false,
					keys: vec![input::Qwery::LCtrl, input::Qwery::Equal],
					mouse_buttons: Vec::new(),
				},
				Arc::new(move |_| {
					basalt.add_scale(0.05);
					println!("Current Scale: {:.1} %", basalt.current_scale() * 100.0);
					input::InputHookRes::Success
				}),
			);

			let basalt = basalt_ret.clone();
			let bin = Mutex::new(None);

			basalt_ret.input_ref().add_hook(
				input::InputHook::Press {
					global: false,
					keys: vec![input::Qwery::F4],
					mouse_buttons: Vec::new(),
				},
				Arc::new(move |_| {
					let mut bin_op = bin.lock();

					if bin_op.is_none() {
						*bin_op = Some(basalt.interface_ref().new_bin());
						let bin = bin_op.as_ref().unwrap();
						bin.basalt_use();

						bin.style_update(interface::bin::BinStyle {
							pos_from_t: Some(0.0),
							pos_from_r: Some(0.0),
							width: Some(500.0),
							height: Some(500.0),
							back_image_atlas: Some(atlas::Coords {
								img_id: 1,
								sub_img_id: 1,
								x: 0,
								y: 0,
								w: basalt.limits().max_image_dimension_2d,
								h: basalt.limits().max_image_dimension_2d,
							}),
							..interface::bin::BinStyle::default()
						});
					} else {
						*bin_op = None;
					}

					input::InputHookRes::Success
				}),
			);

			Ok(basalt_ret)
		}
	}

	pub(crate) fn recreate_swapchain(&self, reason: SwapchainRecreateReason) {
		self.swapchain_recreate.lock().push(reason);
		self.swapchain_recreate_cond.notify_one();
	}

	pub(crate) fn show_bin_stats(&self) -> bool {
		self.bin_stats
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

	pub fn set_scale(&self, to: f32) {
		let mut custom_scale = self.custom_scale.lock();
		*custom_scale = to;
		self.interface_ref().set_scale(*custom_scale);
	}

	pub fn add_scale(&self, amt: f32) {
		let mut custom_scale = self.custom_scale.lock();
		*custom_scale += amt;
		self.interface_ref().set_scale(*custom_scale);
	}

	pub fn interface(&self) -> Arc<Interface> {
		self.interface.clone()
	}

	pub fn interface_ref(&self) -> &Arc<Interface> {
		&self.interface
	}

	pub fn atlas(&self) -> Arc<Atlas> {
		self.atlas.clone()
	}

	pub fn atlas_ref(&self) -> &Arc<Atlas> {
		&self.atlas
	}

	pub fn device(&self) -> Arc<Device> {
		self.device.clone()
	}

	pub fn device_ref(&self) -> &Arc<Device> {
		&self.device
	}

	/// Note: This queue may be the same as the graphics queue in cases where the device only
	/// has a single queue present.
	pub fn compute_queue(&self) -> Arc<device::Queue> {
		self.compute_queue.clone()
	}

	/// Note: This queue may be the same as the graphics queue in cases where the device only
	/// has a single queue present.
	pub fn compute_queue_ref(&self) -> &Arc<device::Queue> {
		&self.compute_queue
	}

	/// Note: This queue may be the same as the compute queue in cases where the device only
	/// has two queues present. In cases where there is only one queue the graphics, compute,
	/// and transfer queues will all be the same queue.
	pub fn transfer_queue(&self) -> Arc<device::Queue> {
		self.transfer_queue.clone()
	}

	/// Note: This queue may be the same as the compute queue in cases where the device only
	/// has two queues present. In cases where there is only one queue the graphics, compute,
	/// and transfer queues will all be the same queue.
	pub fn transfer_queue_ref(&self) -> &Arc<device::Queue> {
		&self.transfer_queue
	}

	pub fn graphics_queue(&self) -> Arc<device::Queue> {
		self.graphics_queue.clone()
	}

	pub fn graphics_queue_ref(&self) -> &Arc<device::Queue> {
		&self.graphics_queue
	}

	pub fn secondary_compute_queue(&self) -> Option<Arc<device::Queue>> {
		self.secondary_compute_queue.clone()
	}

	pub fn secondary_compute_queue_ref(&self) -> Option<&Arc<device::Queue>> {
		self.secondary_compute_queue.as_ref()
	}

	pub fn secondary_transfer_queue(&self) -> Option<Arc<device::Queue>> {
		self.secondary_transfer_queue.clone()
	}

	pub fn secondary_transfer_queue_ref(&self) -> Option<&Arc<device::Queue>> {
		self.secondary_transfer_queue.as_ref()
	}

	pub fn secondary_graphics_queue(&self) -> Option<Arc<device::Queue>> {
		self.secondary_graphics_queue.clone()
	}

	pub fn secondary_graphics_queue_ref(&self) -> Option<&Arc<device::Queue>> {
		self.secondary_graphics_queue.as_ref()
	}

	pub fn physical_device_index(&self) -> usize {
		self.pdevi
	}

	pub fn instance(&self) -> Arc<Instance> {
		self.surface.instance().clone()
	}

	pub fn instance_ref(&self) -> &Arc<Instance> {
		self.surface.instance()
	}

	pub fn surface(&self) -> Arc<Surface<Arc<dyn BasaltWindow + Send + Sync>>> {
		self.surface.clone()
	}

	pub fn surface_ref(&self) -> &Arc<Surface<Arc<dyn BasaltWindow + Send + Sync>>> {
		&self.surface
	}

	pub fn swap_caps(&self) -> swapchain::Capabilities {
		self.surface
			.capabilities(
				PhysicalDevice::from_index(self.surface.instance(), self.pdevi).unwrap(),
			)
			.unwrap()
	}

	pub fn wants_exit(&self) -> bool {
		self.wants_exit.load(atomic::Ordering::Relaxed)
	}

	pub fn window(&self) -> Arc<dyn BasaltWindow + Send + Sync> {
		self.surface().window().clone()
	}

	pub fn options(&self) -> Options {
		self.options.clone()
	}

	pub fn options_ref(&self) -> &Options {
		&self.options
	}

	pub fn resize(&self, w: u32, h: u32) {
		self.surface.window().request_resize(w, h);
	}

	pub fn enable_fullscreen(&self) {
		self.surface.window().enable_fullscreen();
	}

	pub fn disable_fullscreen(&self) {
		self.surface.window().disable_fullscreen();
	}

	pub fn toggle_fullscreen(&self) {
		self.surface.window().toggle_fullscreen();
	}

	pub fn exit(&self) {
		self.wants_exit.store(true, atomic::Ordering::Relaxed);
	}

	/// only works with app loop
	pub fn fps(&self) -> usize {
		self.fps.load(atomic::Ordering::Relaxed)
	}

	/// only works with app loop
	pub fn force_recreate_swapchain(&self) {
		self.swapchain_recreate.lock().push(SwapchainRecreateReason::External);
		self.swapchain_recreate_cond.notify_one();
	}

	/// only works with app loop
	pub fn wait_for_exit(&self) -> Result<(), String> {
		match self.loop_thread.lock().take() {
			Some(handle) =>
				match handle.join() {
					Ok(ok) => ok,
					Err(_) => Err(format!("Failed to join loop thread.")),
				},
			None => Ok(()),
		}
	}

	fn app_loop(self: &Arc<Self>) -> Result<(), String> {
		let mut win_size_x;
		let mut win_size_y;
		let mut frames = 0_usize;
		let mut last_out = Instant::now();
		let mut swapchain_ = None;
		let mut itf_resize = true;

		let preferred_swap_formats =
			vec![vulkano::format::Format::R8G8B8A8Srgb, vulkano::format::Format::B8G8R8A8Srgb];

		let mut swapchain_format_ = None;

		for a in &preferred_swap_formats {
			for &(ref b, _) in &self.swap_caps.supported_formats {
				if a == b {
					swapchain_format_ = Some(*a);
					break;
				}
			}
			if swapchain_format_.is_some() {
				break;
			}
		}

		let swapchain_format = match swapchain_format_ {
			Some(some) => some,
			None =>
				return Err(format!(
					"Failed to find capatible format for swapchain. Avaible formats: {:?}",
					self.swap_caps.supported_formats
				)),
		};

		let mut itf_renderer = interface::render::ItfRenderer::new(self.clone());
		let mut previous_frame_future: Option<Box<dyn GpuFuture>> = None;
		let mut acquire_fullscreen_exclusive = false;

		'resize: loop {
			self.swapchain_recreate.lock().clear();

			let current_capabilities = self
				.surface
				.capabilities(
					PhysicalDevice::from_index(self.surface.instance(), self.pdevi).unwrap(),
				)
				.unwrap();

			let [x, y] = current_capabilities
				.current_extent
				.unwrap_or(self.surface().window().inner_dimensions());
			win_size_x = x;
			win_size_y = y;
			*self.window_size.lock() = [x, y];

			if win_size_x == 0 || win_size_y == 0 {
				thread::sleep(Duration::from_millis(30));
				continue;
			}

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

			let mut min_image_count = current_capabilities.min_image_count;
			let max_image_count = current_capabilities.max_image_count.unwrap_or(0);

			if max_image_count == 0 || min_image_count + 1 <= max_image_count {
				min_image_count += 1;
			}

			swapchain_ = match match swapchain_
				.as_ref()
				.map(|v: &(Arc<Swapchain<_>>, _)| v.0.clone())
			{
				Some(old_swapchain) =>
					Swapchain::with_old_swapchain(
						self.device.clone(),
						self.surface.clone(),
						min_image_count,
						swapchain_format,
						[x, y],
						1,
						ImageUsage::color_attachment(),
						&self.graphics_queue,
						swapchain::SurfaceTransform::Identity,
						swapchain::CompositeAlpha::Opaque,
						present_mode,
						swapchain::FullscreenExclusive::AppControlled,
						true,
						ColorSpace::SrgbNonLinear,
						old_swapchain,
					),
				None =>
					Swapchain::new(
						self.device.clone(),
						self.surface.clone(),
						min_image_count,
						swapchain_format,
						[x, y],
						1,
						ImageUsage::color_attachment(),
						&self.graphics_queue,
						swapchain::SurfaceTransform::Identity,
						swapchain::CompositeAlpha::Opaque,
						present_mode,
						swapchain::FullscreenExclusive::AppControlled,
						true,
						ColorSpace::SrgbNonLinear,
					),
			} {
				Ok(ok) => Some(ok),
				Err(e) =>
					match e {
						SwapchainCreationError::UnsupportedDimensions => continue,
						e => return Err(format!("Basalt failed to recreate swapchain: {}", e)),
					},
			};

			let (swapchain, images) =
				(&swapchain_.as_ref().unwrap().0, &swapchain_.as_ref().unwrap().1);
			let mut fps_avg = VecDeque::new();
			let mut wait_for_update = false;

			loop {
				previous_frame_future.as_mut().map(|future| future.cleanup_finished());
				let mut recreate_swapchain_now = false;

				for reason in self.swapchain_recreate.lock().split_off(0) {
					match reason {
						SwapchainRecreateReason::Scale => {
							itf_resize = true;
							wait_for_update = false;
						},
						SwapchainRecreateReason::Resize(w, h) => {
							if w != win_size_x || h != win_size_y {
								itf_resize = true;
								wait_for_update = false;
								recreate_swapchain_now = true;
							}
						},
						SwapchainRecreateReason::Redraw => {
							let [w, h] = self
								.surface
								.capabilities(
									PhysicalDevice::from_index(
										self.surface.instance(),
										self.pdevi,
									)
									.unwrap(),
								)
								.unwrap()
								.current_extent
								.unwrap_or(self.surface().window().inner_dimensions());
							wait_for_update = false;

							if w != win_size_x || h != win_size_y {
								itf_resize = true;
								recreate_swapchain_now = true;
							}
						},
						SwapchainRecreateReason::Properties
						| SwapchainRecreateReason::External => {
							itf_resize = true;
							wait_for_update = false;
							recreate_swapchain_now = true;
						},
						SwapchainRecreateReason::Exclusive(ex) =>
							if ex {
								acquire_fullscreen_exclusive = true;
							} else {
								swapchain.release_fullscreen_exclusive().unwrap();
							},
						SwapchainRecreateReason::ODBUpdated => {
							wait_for_update = false;
						},
					}
				}

				if recreate_swapchain_now {
					continue 'resize;
				}

				if acquire_fullscreen_exclusive {
					if swapchain.acquire_fullscreen_exclusive().is_ok() {
						acquire_fullscreen_exclusive = false;
						println!("Exclusive fullscreen acquired!");
					}
				}

				if self.options.itf_limit_draw {
					if wait_for_update {
						let mut lck = self.swapchain_recreate.lock();
						self.swapchain_recreate_cond.wait(&mut lck);
						continue;
					} else {
						wait_for_update = true;
					}
				}

				let duration = last_out.elapsed();
				let millis = (duration.as_secs() * 1000) as f32
					+ (duration.subsec_nanos() as f32 / 1000000.0);

				if millis >= 50.0 {
					let fps = frames as f32 / (millis / 1000.0);
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

				let (image_num, suboptimal, acquire_future) =
					match swapchain::acquire_next_image(
						swapchain.clone(),
						Some(::std::time::Duration::new(1, 0)),
					) {
						Ok(ok) => ok,
						Err(e) => {
							if SHOW_SWAPCHAIN_WARNINGS {
								println!(
									"Recreating swapchain due to acquire_next_image() error: \
									 {:?}.",
									e
								)
							}
							itf_resize = true;
							continue 'resize;
						},
					};

				let cmd_buf = AutoCommandBufferBuilder::primary_one_time_submit(
					self.device.clone(),
					self.graphics_queue.family(),
				)
				.unwrap();
				let (cmd_buf, _) = itf_renderer.draw(
					cmd_buf,
					[win_size_x, win_size_y],
					itf_resize,
					images,
					true,
					image_num,
				);
				let cmd_buf = cmd_buf.build().unwrap();

				previous_frame_future = match match previous_frame_future.take() {
					Some(future) => Box::new(future.join(acquire_future)) as Box<dyn GpuFuture>,
					None => Box::new(acquire_future) as Box<dyn GpuFuture>,
				}
				.then_execute(self.graphics_queue.clone(), cmd_buf)
				.unwrap()
				.then_swapchain_present(
					self.graphics_queue.clone(),
					swapchain.clone(),
					image_num,
				)
				.then_signal_fence_and_flush()
				{
					Ok(ok) => Some(Box::new(ok)),
					Err(e) =>
						match e {
							vulkano::sync::FlushError::OutOfDate => {
								itf_resize = true;
								if SHOW_SWAPCHAIN_WARNINGS {
									println!(
										"Recreating swapchain due to \
										 then_signal_fence_and_flush() error: {:?}.",
										e
									)
								}
								continue 'resize;
							},
							_ => panic!("then_signal_fence_and_flush() {:?}", e),
						},
				};

				if suboptimal {
					itf_resize = true;
					continue 'resize;
				}

				itf_resize = false;
				if self.wants_exit.load(atomic::Ordering::Relaxed) {
					break 'resize;
				}
			}
		}

		Ok(())
	}
}
