extern crate winit;
#[macro_use]
pub extern crate vulkano;
#[macro_use]
pub extern crate vulkano_shaders;
extern crate arc_swap;
extern crate crossbeam;
pub extern crate ilmenite;
extern crate image;
extern crate num_cpus;
extern crate ordered_float;
extern crate parking_lot;

pub mod atlas;
pub mod image_view;
pub mod input;
pub mod interface;
pub mod misc;
pub mod window;

use crate::interface::{BstMSAALevel, ItfDrawTarget};
use atlas::Atlas;
use ilmenite::{ImtFillQuality, ImtSampleQuality};
use input::Input;
use interface::bin::BinUpdateStats;
use interface::Interface;
use parking_lot::{Condvar, Mutex};
use std::mem::MaybeUninit;
use std::str::FromStr;
use std::sync::atomic::{self, AtomicBool, AtomicUsize};
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use vulkano::command_buffer::{AutoCommandBufferBuilder, CommandBufferUsage};
use vulkano::device::physical::{PhysicalDevice, PhysicalDeviceType};
use vulkano::device::{self, Device, DeviceExtensions, Features as VkFeatures};
use vulkano::format::Format as VkFormat;
use vulkano::image::view::ImageView;
use vulkano::image::ImageUsage;
use vulkano::instance::{Instance, InstanceExtensions};
use vulkano::swapchain::{
	self, ColorSpace as VkColorSpace, CompositeAlpha, Surface, Swapchain,
	SwapchainCreationError,
};
use vulkano::sync::GpuFuture;
use window::BasaltWindow;

static BASALT_INIT_COMPLETE: Mutex<bool> = parking_lot::const_mutex(false);
static BASALT_INIT_COMPLETE_COND: Condvar = Condvar::new();

/// Vulkan features required in order for Basalt to function correctly.
pub fn basalt_required_vk_features() -> VkFeatures {
	VkFeatures {
		descriptor_indexing: true,
		shader_uniform_buffer_array_non_uniform_indexing: true,
		runtime_descriptor_array: true,
		descriptor_binding_variable_descriptor_count: true,
		descriptor_binding_partially_bound: true,
		..ilmenite::ilmenite_required_vk_features()
	}
}

/// Options for Basalt's creation and operation.
#[derive(Debug, Clone)]
pub struct Options {
	ignore_dpi: bool,
	window_size: [u32; 2],
	title: String,
	scale: f32,
	msaa: BstMSAALevel,
	app_loop: bool,
	exclusive_fullscreen: bool,
	prefer_integrated_gpu: bool,
	instance_extensions: InstanceExtensions,
	device_extensions: DeviceExtensions,
	instance_layers: Vec<String>,
	composite_alpha: CompositeAlpha,
	force_unix_backend_x11: bool,
	features: VkFeatures,
	imt_gpu_accelerated: bool,
	imt_fill_quality: Option<ImtFillQuality>,
	imt_sample_quality: Option<ImtSampleQuality>,
	validation: bool,
}

impl Default for Options {
	fn default() -> Self {
		Options {
			ignore_dpi: false,
			window_size: [1920, 1080],
			title: "vk-basalt".to_string(),
			scale: 1.0,
			msaa: BstMSAALevel::Four,
			app_loop: false,
			exclusive_fullscreen: false,
			prefer_integrated_gpu: false,
			force_unix_backend_x11: false,
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
			instance_layers: Vec::new(),
			device_extensions: DeviceExtensions {
				khr_swapchain: true,
				khr_storage_buffer_storage_class: true,
				..DeviceExtensions::none()
			},
			features: basalt_required_vk_features(),
			composite_alpha: CompositeAlpha::Opaque,
			imt_gpu_accelerated: true,
			imt_fill_quality: None,
			imt_sample_quality: None,
			validation: false,
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

	/// Set the the amount of MSAA of the UI
	pub fn msaa(mut self, to: BstMSAALevel) -> Self {
		self.msaa = to;
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

	pub fn enable_validation(mut self) -> Self {
		if self.validation {
			return self;
		}

		self.instance_extensions = InstanceExtensions {
			ext_debug_utils: true,
			..self.instance_extensions
		};

		self.instance_layers.push(String::from("VK_LAYER_KHRONOS_validation"));
		self.validation = true;
		self
	}

	/// Specifify a custom set of vulkan features. This should be used with
	/// `basalt_required_vk_features()` to ensure Basalt functions correctly. For example:
	/// ```no_run
	/// .with_features(
	///     Features {
	///         storage_buffer16_bit_access: true,
	///         .. basalt_required_vk_features()
	///     }
	/// )
	/// ```
	pub fn with_features(mut self, features: VkFeatures) -> Self {
		self.features = features;
		self
	}

	/// Set the composite alpha mode used when creating the swapchain. Only effective when using
	/// app loop.
	pub fn composite_alpha(mut self, to: CompositeAlpha) -> Self {
		self.composite_alpha = to;
		self
	}

	/// Setting this to true, will set the environment variable `WINIT_UNIX_BACKEND=x11` forcing
	/// winit to use x11 over wayland. This is `false` by default, but it is recommended to set
	/// this to `true` if you intend to use `Basalt::capture_cursor()`. With winit on wayland,
	/// `MouseMotion` will not be emitted.
	pub fn force_unix_backend_x11(mut self, to: bool) -> Self {
		self.force_unix_backend_x11 = to;
		self
	}

	/// Basalt uses ilmenite in the backend for text. Setting this option to true will allow
	/// ilmenite to use a gpu code path which will have some performance gain; however, this
	/// code path may be broken on some systems. This defaults to true.
	pub fn imt_gpu_accelerated(mut self, to: bool) -> Self {
		self.imt_gpu_accelerated = to;
		self
	}

	/// Basalt uses ilmenite in the backend for text. This option allows for modifying the
	/// fill quality (the amount of casted rays) that ilmenite will use. This defaults to
	/// `ImtFillQuality::Normal".
	pub fn imt_fill_quality(mut self, q: ImtFillQuality) -> Self {
		self.imt_fill_quality = Some(q);
		self
	}

	/// Basalt uses ilmenite in the backend for text. This option allows for modifying the
	/// sample quality (the amount of samples in a subpixel) that ilmenite will use. This
	/// defaults to `ImtSampleQuality::Normal.
	pub fn imt_sample_quality(mut self, q: ImtSampleQuality) -> Self {
		self.imt_sample_quality = Some(q);
		self
	}
}

/// Device limitations
#[derive(Debug)]
pub struct Limits {
	pub max_image_dimension_2d: u32,
	pub max_image_dimension_3d: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BstFormatsInUse {
	pub atlas: VkFormat,
	pub interface: VkFormat,
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
	formats_in_use: BstFormatsInUse,
}

impl Initials {
	pub fn use_first_device(
		mut options: Options,
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
			} else if arg.starts_with("--scale=") {
				let by_equal: Vec<_> = arg.split("=").collect();

				if by_equal.len() != 2 {
					println!("Incorrect '--scale' usage. Example: '--scale=2.0'");
					break;
				} else {
					match f32::from_str(by_equal[1]) {
						Ok(scale) => {
							options.scale = scale;
							println!("[Basalt]: Using custom scale from args, {}x", scale);
						},
						Err(_) => {
							println!("Incorrect '--scale' usage. Example: '--scale=2.0'");
						},
					}
				}
			}
		}

		let instance = match Instance::new(
			None,
			vulkano::Version::V1_2,
			&options.instance_extensions,
			options.instance_layers.iter().map(|l| l.as_str()),
		)
		.map_err(|e| format!("Failed to create instance: {}", e))
		{
			Ok(ok) => ok,
			Err(e) => return result_fn(Err(e)),
		};

		// window::open_surface() does not return so it should keep this callback alive.
		let _validation_callback = if options.validation {
			use vulkano::instance::debug::{DebugCallback, MessageSeverity, MessageType};

			let msg_sev = MessageSeverity {
				error: true,
				warning: true,
				..MessageSeverity::none()
			};

			let msg_ty = MessageType {
				general: false,
				validation: true,
				performance: true,
			};

			Some(DebugCallback::new(&instance, msg_sev, msg_ty, |msg| {
				println!(
					"[Basalt][VkDebug][{}][{}]: {}",
					if msg.severity.error {
						"Error"
					} else if msg.severity.warning {
						"Warning"
					} else {
						"Unknown"
					},
					if msg.ty.validation {
						"Validation"
					} else if msg.ty.performance {
						"Performance"
					} else {
						"Unknown"
					},
					msg.description
				);
			}))
		} else {
			None
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
							"  {}: {:?} | Type: {:?} | API: {}",
							i,
							dev.properties().device_name,
							dev.properties().device_type,
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
										match d.properties().device_type {
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
										match d.properties().device_type {
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

				let mut g_optimal = misc::drain_filter(&mut queue_families, |family| {
					family.supports_graphics() && !family.supports_compute()
				});

				let mut c_optimal = misc::drain_filter(&mut queue_families, |family| {
					family.supports_compute() && !family.supports_graphics()
				});

				let mut t_optimal = misc::drain_filter(&mut queue_families, |family| {
					family.explicitly_supports_transfers()
						&& !family.supports_compute()
						&& !family.supports_graphics()
				});

				// TODO: Use https://github.com/rust-lang/rust/issues/43244 when stable

				// let mut g_optimal: Vec<_> = queue_families
				// .drain_filter(|family| {
				// family.supports_graphics() && !family.supports_compute()
				// })
				// .collect();
				// let mut c_optimal: Vec<_> = queue_families
				// .drain_filter(|family| {
				// family.supports_compute() && !family.supports_graphics()
				// })
				// .collect();
				// let mut t_optimal: Vec<_> = queue_families
				// .drain_filter(|family| {
				// family.explicitly_supports_transfers()
				// && !family.supports_compute()
				// && !family.supports_graphics()
				// })
				// .collect();

				let (g_primary, mut g_secondary) = match g_optimal.len() {
					0 => {
						// let mut g_suboptimal: Vec<_> = queue_families
						// .drain_filter(&mut queue_families, |family|
						// family.supports_graphics()) .collect();

						let mut g_suboptimal =
							misc::drain_filter(&mut queue_families, |family| {
								family.supports_graphics()
							});

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
						// let mut g_suboptimal: Vec<_> = queue_families
						// .drain_filter(|family| family.supports_graphics())
						// .collect();

						let mut g_suboptimal =
							misc::drain_filter(&mut queue_families, |family| {
								family.supports_graphics()
							});

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
						// let mut c_suboptimal: Vec<_> = queue_families
						// .drain_filter(|family| family.supports_compute())
						// .collect();

						let mut c_suboptimal =
							misc::drain_filter(&mut queue_families, |family| {
								family.supports_compute()
							});

						match c_suboptimal.len() {
							0 => {
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
								}
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
						// let mut c_suboptimal: Vec<_> = queue_families
						// .drain_filter(|family| family.supports_compute())
						// .collect();

						let mut c_suboptimal =
							misc::drain_filter(&mut queue_families, |family| {
								family.supports_compute()
							});

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

				// If we don't do this, there will be the folowing error.
				// Failed to create device: a restriction for the feature
				// attachment_fragment_shading_rate was not met: requires feature
				// shading_rate_image to be disabled
				// if supported_features.shading_rate_image{
				// supported_features.attachment_fragment_shading_rate=false;
				// supported_features.pipeline_fragment_shading_rate=false;
				// supported_features.primitive_fragment_shading_rate=false;
				// }

				let (device, mut queues) = match Device::new(
					*physical_device,
					&options.features,
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

				let limits = Arc::new(Limits {
					max_image_dimension_2d: physical_device.properties().max_image_dimension2_d,
					max_image_dimension_3d: physical_device.properties().max_image_dimension3_d,
				});

				// Format Selection
				let mut atlas_formats = vec![
					VkFormat::R16G16B16A16_UNORM,
					VkFormat::R8G8B8A8_UNORM,
					VkFormat::B8G8R8A8_UNORM,
					VkFormat::A8B8G8R8_UNORM_PACK32,
				];

				let mut interface_formats = vec![
					VkFormat::R16G16B16A16_UNORM,
					VkFormat::A2B10G10R10_UNORM_PACK32,
					VkFormat::R8G8B8A8_UNORM,
					VkFormat::B8G8R8A8_UNORM,
					VkFormat::A8B8G8R8_UNORM_PACK32,
				];

				atlas_formats.retain(|f| {
					let properties = f.properties(*physical_device);
					let optimal = &properties.optimal_tiling_features;
					optimal.sampled_image
						&& optimal.storage_image && optimal.blit_dst
						&& optimal.transfer_dst
				});

				interface_formats.retain(|f| {
					let properties = f.properties(*physical_device);
					let optimal = &properties.optimal_tiling_features;
					optimal.sampled_image && optimal.color_attachment
				});

				if atlas_formats.is_empty() {
					return result_fn(Err(format!(
						"Unable to find a suitable format for the atlas."
					)));
				}

				if interface_formats.is_empty() {
					return result_fn(Err(format!(
						"Unable to find a suitable format for the interface."
					)));
				}

				let formats_in_use = BstFormatsInUse {
					atlas: atlas_formats.remove(0),
					interface: interface_formats.remove(0),
				};

				let mut present_queue_families = Vec::with_capacity(2);
				present_queue_families.push(graphics_queue.family());

				if let Some(queue) = secondary_graphics_queue.as_ref() {
					present_queue_families.push(queue.family());
				}

				present_queue_families.dedup();

				for family in present_queue_families {
					match surface.is_supported(family) {
						Ok(supported) if !supported =>
							return result_fn(Err(format!(
								"Queue family doesn't support presentation on surface."
							))),
						Err(e) =>
							return result_fn(Err(format!(
								"Failed to check presentation support for queue family: {:?}",
								e
							))),
						_ => (),
					}
				}

				println!("[Basalt]: Atlas Format: {:?}", formats_in_use.atlas);
				println!("[Basalt]: Interface Format: {:?}", formats_in_use.interface);

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
					formats_in_use,
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
pub enum BstEvent {
	BstWinEv(BstWinEv),
}

impl BstEvent {
	pub fn requires_swapchain_recreate(&self) -> bool {
		match self {
			Self::BstWinEv(win_ev) => win_ev.requires_swapchain_recreate(),
		}
	}
}

#[derive(Debug, Clone, PartialEq)]
pub enum BstWinEv {
	Resized(u32, u32),
	ScaleChanged,
	RedrawRequest,
	FullscreenExclusive(bool),
}

impl BstWinEv {
	pub fn requires_swapchain_recreate(&self) -> bool {
		match self {
			Self::Resized(..) => true,
			Self::ScaleChanged => true,
			Self::RedrawRequest => true, // TODO: Is swapchain recreate required or just a
			// new frame?
			Self::FullscreenExclusive(_) => true,
		}
	}
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BstAppEvent {
	Normal(BstEvent),
	SwapchainPropertiesChanged,
	ExternalForceUpdate,
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
	gpu_time: AtomicUsize,
	cpu_time: AtomicUsize,
	bin_time: AtomicUsize,
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
	options: Options,
	ignore_dpi_data: Mutex<Option<(usize, Instant, u32, u32)>>,
	bin_stats: bool,
	events: Mutex<Vec<BstEvent>>,
	events_internal: Mutex<Vec<BstEvent>>,
	app_events: Mutex<Vec<BstAppEvent>>,
	app_events_cond: Condvar,
	formats_in_use: BstFormatsInUse,
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
		if options.force_unix_backend_x11 && cfg!(unix) {
			std::env::set_var("WINIT_UNIX_BACKEND", "x11");
		}

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
				cpu_time: AtomicUsize::new(0),
				gpu_time: AtomicUsize::new(0),
				bin_time: AtomicUsize::new(0),
				interface: { MaybeUninit::uninit() }.assume_init(),
				limits: initials.limits.clone(),
				atlas: { MaybeUninit::uninit() }.assume_init(),
				input: { MaybeUninit::uninit() }.assume_init(),
				wants_exit: AtomicBool::new(false),
				loop_thread: Mutex::new(None),
				pdevi: initials.pdevi,
				vsync: Mutex::new(true),
				window_size: Mutex::new(initials.window_size),
				options: initials.options,
				ignore_dpi_data: Mutex::new(None),
				bin_stats: initials.bin_stats,
				events: Mutex::new(Vec::new()),
				events_internal: Mutex::new(Vec::new()),
				app_events: Mutex::new(Vec::new()),
				app_events_cond: Condvar::new(),
				formats_in_use: initials.formats_in_use,
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
					println!(
						"FPS: {}, GPU Time: {:.2} ms, CPU Time: {:.2} ms, BIN Time: {:.2} ms",
						basalt.fps(),
						basalt.gpu_time.load(atomic::Ordering::Relaxed) as f32 / 1000.0,
						basalt.cpu_time.load(atomic::Ordering::Relaxed) as f32 / 1000.0,
						basalt.bin_time.load(atomic::Ordering::Relaxed) as f32 / 1000.0,
					);
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
					let msaa = basalt.interface_ref().decrease_msaa();
					println!("MSAA set to {}X", msaa.as_u32());
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
					let msaa = basalt.interface_ref().increase_msaa();
					println!("MSAA set to {}X", msaa.as_u32());
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
					basalt.send_app_event(BstAppEvent::SwapchainPropertiesChanged);

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
					let mut scale = basalt.interface_ref().current_scale();
					scale -= 0.05;

					if scale < 0.05 {
						scale = 0.05;
					}

					basalt.interface_ref().set_scale(scale);
					println!("[Basalt]: Current Inteface Scale: {:.1} %", scale * 100.0);
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
					let mut scale = basalt.interface_ref().current_scale();
					scale += 0.05;

					if scale > 4.0 {
						scale = 4.0;
					}

					basalt.interface_ref().set_scale(scale);
					println!("[Basalt]: Current Inteface Scale: {:.1} %", scale * 100.0);
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

			*BASALT_INIT_COMPLETE.lock() = true;
			BASALT_INIT_COMPLETE_COND.notify_all();
			Ok(basalt_ret)
		}
	}

	pub(crate) fn send_event(&self, ev: BstEvent) {
		if self.options.app_loop {
			self.app_events.lock().push(BstAppEvent::Normal(ev.clone()));
			self.app_events_cond.notify_one();
		} else {
			self.events.lock().push(ev.clone());
		}

		self.events_internal.lock().push(ev);
	}

	pub(crate) fn send_app_event(&self, ev: BstAppEvent) {
		self.app_events.lock().push(ev);
		self.app_events_cond.notify_one();
	}

	pub(crate) fn store_bin_time(&self, t: usize) {
		self.bin_time.store(t, atomic::Ordering::Relaxed);
	}

	/// Panics if the current cofiguration is an app_loop.
	pub fn poll_events(&self) -> Vec<BstEvent> {
		if self.options.app_loop {
			panic!("Basalt::poll_events() only allowed in non-app_loop aapplications.");
		}

		self.events.lock().drain(..).collect()
	}

	pub(crate) fn poll_events_internal<F>(&self, mut retain_fn: F)
	where
		F: FnMut(&BstEvent) -> bool,
	{
		self.events_internal.lock().retain(|ev| retain_fn(ev));
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

	pub fn formats_in_use(&self) -> BstFormatsInUse {
		self.formats_in_use.clone()
	}

	pub fn swap_caps(&self) -> swapchain::Capabilities {
		self.surface
			.capabilities(
				PhysicalDevice::from_index(self.surface.instance(), self.pdevi).unwrap(),
			)
			.unwrap()
	}

	/// Get the current extent of the surface. In the case current extent is none, the window's
	/// inner dimensions will be used instead. This function is equivlent to:
	/// ```no_run
	/// basalt
	/// 	.surface()
	/// 	.capabilities(
	/// 		PhysicalDevice::from_index(basalt.instance(), basalt.physical_device_index())
	/// 			.unwrap(),
	/// 	)
	/// 	.unwrap()
	/// 	.current_extent
	/// 	.unwrap_or(basalt.surface_ref().window().inner_dimmension())
	/// ```
	pub fn current_extent(&self) -> [u32; 2] {
		self.swap_caps()
			.current_extent
			.unwrap_or(self.surface_ref().window().inner_dimensions())
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
		self.app_events.lock().push(BstAppEvent::ExternalForceUpdate);
		self.app_events_cond.notify_one();
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
		let mut swapchain_ = None;

		let pref_format_colorspace = vec![
			(VkFormat::B8G8R8A8_SRGB, VkColorSpace::SrgbNonLinear),
			(VkFormat::B8G8R8A8_SRGB, VkColorSpace::SrgbNonLinear),
		];

		let mut swapchain_format_op = None;

		for (a, b) in &pref_format_colorspace {
			for &(ref c, ref d) in &self.swap_caps.supported_formats {
				if a == c && b == d {
					swapchain_format_op = Some((*a, *b));
					break;
				}
			}
			if swapchain_format_op.is_some() {
				break;
			}
		}

		let (swapchain_format, swapchain_colorspace) = swapchain_format_op.ok_or(format!(
			"Failed to find capatible format for swapchain. Avaible formats: {:?}",
			self.swap_caps.supported_formats
		))?;
		println!("[Basalt]: Swapchain {:?}/{:?}", swapchain_format, swapchain_colorspace);

		let mut previous_frame_future: Option<Box<dyn GpuFuture>> = None;
		let mut acquire_fullscreen_exclusive = false;

		'resize: loop {
			self.app_events.lock().clear();

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
					old_swapchain
						.recreate()
						.num_images(min_image_count)
						.format(swapchain_format)
						.dimensions([x, y])
						.usage(ImageUsage::color_attachment())
						.transform(swapchain::SurfaceTransform::Identity)
						.composite_alpha(self.options.composite_alpha)
						.present_mode(present_mode)
						.fullscreen_exclusive(swapchain::FullscreenExclusive::AppControlled)
						.build(),
				None =>
					Swapchain::start(self.device.clone(), self.surface.clone())
						.num_images(min_image_count)
						.format(swapchain_format)
						.dimensions([x, y])
						.usage(ImageUsage::color_attachment())
						.transform(swapchain::SurfaceTransform::Identity)
						.composite_alpha(self.options.composite_alpha)
						.present_mode(present_mode)
						.fullscreen_exclusive(swapchain::FullscreenExclusive::AppControlled)
						.build(),
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
			let images: Vec<_> =
				images.into_iter().map(|i| ImageView::new(i.clone()).unwrap()).collect();

			let mut gpu_times: [u128; 10] = [0; 10];
			let mut cpu_times: [u128; 10] = [0; 10];
			let mut cpu_times_i = 0;
			let mut gpu_times_i = 0;

			let inc_times_i = |mut i: usize| -> usize {
				i += 1;

				if i >= 10 {
					i = 0;
				}

				i
			};

			let calc_hertz = |arr: &[u128; 10]| -> usize {
				(1.0 / arr.iter().map(|t| *t as f64 / 100000000.0).sum::<f64>() / 10.0).ceil()
					as usize
			};

			let calc_time = |arr: &[u128; 10]| -> usize {
				(arr.iter().map(|t| *t).sum::<u128>() / 10) as usize
			};

			let mut last_present = Instant::now();

			loop {
				previous_frame_future.as_mut().map(|future| future.cleanup_finished());
				let mut cpu_time_start = Instant::now();
				let mut recreate_swapchain_now = false;

				for ev in self.app_events.lock().drain(..) {
					match ev {
						BstAppEvent::Normal(ev) =>
							match ev {
								BstEvent::BstWinEv(win_ev) =>
									match win_ev {
										BstWinEv::Resized(w, h) => {
											if w != win_size_x || h != win_size_y {
												recreate_swapchain_now = true;
											}
										},
										BstWinEv::ScaleChanged => {
											recreate_swapchain_now = true;
										},
										BstWinEv::RedrawRequest => {
											let [w, h] = self.current_extent();

											if w != win_size_x || h != win_size_y {
												recreate_swapchain_now = true;
											}
										},
										BstWinEv::FullscreenExclusive(exclusive) => {
											if exclusive {
												acquire_fullscreen_exclusive = true;
											} else {
												swapchain
													.release_fullscreen_exclusive()
													.unwrap();
											}
										},
									},
							},
						BstAppEvent::SwapchainPropertiesChanged => {
							recreate_swapchain_now = true;
						},
						BstAppEvent::ExternalForceUpdate => {
							recreate_swapchain_now = true;
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

				cpu_times[cpu_times_i] = cpu_time_start.elapsed().as_micros();

				let (image_num, suboptimal, acquire_future) =
					match swapchain::acquire_next_image(
						swapchain.clone(),
						Some(::std::time::Duration::new(1, 0)),
					) {
						Ok(ok) => ok,
						Err(_) => continue 'resize,
					};

				cpu_time_start = Instant::now();

				let cmd_buf = AutoCommandBufferBuilder::primary(
					self.device.clone(),
					self.graphics_queue.family(),
					CommandBufferUsage::OneTimeSubmit,
				)
				.unwrap();

				let (cmd_buf, _) = self.interface.draw(cmd_buf, ItfDrawTarget::Swapchain {
					images: images.clone(),
					image_num,
				});

				let cmd_buf = cmd_buf.build().unwrap();
				cpu_times[cpu_times_i] += cpu_time_start.elapsed().as_micros();

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
							vulkano::sync::FlushError::OutOfDate => continue 'resize,
							_ => panic!("then_signal_fence_and_flush() {:?}", e),
						},
				};

				if suboptimal {
					continue 'resize;
				}

				if self.wants_exit.load(atomic::Ordering::Relaxed) {
					break 'resize;
				}

				gpu_times[gpu_times_i] = last_present.elapsed().as_micros();
				last_present = Instant::now();
				cpu_times_i = inc_times_i(cpu_times_i);
				gpu_times_i = inc_times_i(gpu_times_i);
				self.fps.store(calc_hertz(&gpu_times), atomic::Ordering::Relaxed);
				self.gpu_time.store(calc_time(&gpu_times), atomic::Ordering::Relaxed);
				self.cpu_time.store(calc_time(&cpu_times), atomic::Ordering::Relaxed);
			}
		}

		Ok(())
	}
}
