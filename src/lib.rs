#![allow(clippy::significant_drop_in_scrutinee)]
#![allow(clippy::type_complexity)]
// TODO: Remove this
#![allow(dead_code)]

pub mod image_cache;
pub mod input;
pub mod interface;
pub mod interval;
pub mod misc;
pub mod renderer;
pub mod window;

use std::num::NonZeroUsize;
use std::str::FromStr;
use std::sync::atomic::{self, AtomicBool};
use std::sync::Arc;
use std::thread;
use std::thread::{available_parallelism, JoinHandle};

use interface::Interface;
use parking_lot::Mutex;
use vulkano::device::physical::{PhysicalDevice, PhysicalDeviceType};
use vulkano::device::{
    self, Device, DeviceCreateInfo, DeviceExtensions, Features as VkFeatures, QueueCreateInfo,
    QueueFlags,
};
use vulkano::instance::{Instance, InstanceCreateInfo, InstanceExtensions, Version};
use vulkano::swapchain::CompositeAlpha;
use vulkano::VulkanLibrary;

use crate::image_cache::ImageCache;
use crate::input::Input;
use crate::interface::BstMSAALevel;
use crate::interval::Interval;
use crate::window::WindowManager;

/// Vulkan features required in order for Basalt to function correctly.
pub fn basalt_required_vk_features() -> VkFeatures {
    VkFeatures {
        descriptor_indexing: true,
        shader_uniform_buffer_array_non_uniform_indexing: true,
        runtime_descriptor_array: true,
        descriptor_binding_variable_descriptor_count: true,
        descriptor_binding_partially_bound: true,
        ..VkFeatures::empty()
    }
}

/// Options for Basalt's creation and operation.
#[derive(Clone)]
pub struct BstOptions {
    ignore_dpi: bool,
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
    conservative_draw: bool,
    bin_parallel_threads: NonZeroUsize,
    additional_fonts: Vec<Arc<dyn AsRef<[u8]> + Sync + Send>>,
}

impl Default for BstOptions {
    fn default() -> Self {
        Self {
            ignore_dpi: false,
            scale: 1.0,
            msaa: BstMSAALevel::Four,
            app_loop: false,
            exclusive_fullscreen: false,
            prefer_integrated_gpu: false,
            force_unix_backend_x11: false,
            instance_extensions: InstanceExtensions::empty(),
            instance_layers: Vec::new(),
            device_extensions: DeviceExtensions {
                khr_swapchain: true,
                khr_storage_buffer_storage_class: true,
                ..DeviceExtensions::empty()
            },
            features: basalt_required_vk_features(),
            composite_alpha: CompositeAlpha::Opaque,
            conservative_draw: false,
            bin_parallel_threads: NonZeroUsize::new(
                (available_parallelism()
                    .unwrap_or(NonZeroUsize::new(4).unwrap())
                    .get() as f64
                    / 3.0)
                    .ceil() as usize,
            )
            .unwrap(),
            additional_fonts: Vec::new(),
        }
    }
}

impl BstOptions {
    /// Configure Basalt to run in app mode. The swapchain will be managed by Basalt and all
    /// renderering to the swapchain will be done by Basalt. Additional rendering to the
    /// swapchain will be unavailable. This is useful for applications that are UI only.
    pub fn app_loop(mut self) -> Self {
        self.app_loop = true;
        self
    }

    /// Enables the device extension required for exclusive fullscreen.
    /// Generally this extension is only present on Windows. Basalt will return an error upon
    /// creation if this feature isn't supported. With this option enabled
    /// ``BasaltWindow::enable_fullscreen()`` will use exclusive fullscreen; otherwise,
    /// borderless window will be used.
    ///
    /// **Default**: `false`
    ///
    /// # Notes
    /// - `Basalt` will return an `Err` if the extension is not present.
    pub fn use_exclusive_fullscreen(mut self, to: bool) -> Self {
        self.exclusive_fullscreen = to;
        self.device_extensions.ext_full_screen_exclusive = true;
        self
    }

    /// Ignore dpi hints provided by the platform.
    ///
    /// **Default**: `false`
    pub fn ignore_dpi(mut self, to: bool) -> Self {
        self.ignore_dpi = to;
        self
    }

    /// Set the initial scale of the UI
    ///
    /// **Default**: `1.0`
    ///
    /// # Notes
    /// - This is independant of DPI Scaling.
    pub fn scale(mut self, to: f32) -> Self {
        self.scale = to;
        self
    }

    /// Set the the amount of MSAA of the UI
    ///
    /// **Default**: `BstMSAALevel::Four`
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

    /// Specifify a custom set of vulkan features. This should be used with
    /// `basalt_required_vk_features()` to ensure Basalt functions correctly. For example:
    /// ```ignore
    /// .with_features(
    ///     Features {
    ///         storage_buffer16_bit_access: true,
    ///         .. basalt_required_vk_features()
    ///     }
    /// )
    /// ```
    ///
    /// **Default**: `basalt_required_vk_features()`
    pub fn with_features(mut self, features: VkFeatures) -> Self {
        self.features = features;
        self
    }

    /// Set the composite alpha mode used when creating the swapchain. Only effective when using
    /// app loop.
    ///
    /// **Default**: `CompositeAlpha::Opaque`
    pub fn composite_alpha(mut self, to: CompositeAlpha) -> Self {
        self.composite_alpha = to;
        self
    }

    /// Setting this to true, will set the environment variable `WINIT_UNIX_BACKEND=x11` forcing
    /// winit to use x11 over wayland. It is recommended to set this to `true` if you intend to
    /// use `Basalt::capture_cursor()`. With winit on wayland, `MouseMotion` will not be emitted.
    ///
    /// **Default**: `false`
    pub fn force_unix_backend_x11(mut self, to: bool) -> Self {
        self.force_unix_backend_x11 = to;
        self
    }

    /// Specify how many threads to use for parallel `Bin` updates.
    ///
    /// **Default**: 1/3 of available threads (rounded up)
    pub fn bin_parallel_threads(mut self, bin_parallel_threads: usize) -> Self {
        self.bin_parallel_threads = NonZeroUsize::new(bin_parallel_threads.max(1)).unwrap();
        self
    }

    /// Only update interface image on UI/Surface change.
    ///
    /// **Default**: false
    ///
    /// # Notes:
    /// - This is for application mode applications only. See `app_loop()`.
    /// - This feature is *EXPERIMENTAL* and may not always work correctly.
    pub fn conservative_draw(mut self, enable: bool) -> Self {
        self.conservative_draw = enable;
        self
    }

    /// Add a font from a binary source that can be used.
    ///
    /// # Notes:
    /// - This is intended to be used with `include_bytes!(...)`.
    pub fn add_binary_font<B: AsRef<[u8]> + Sync + Send + 'static>(mut self, font: B) -> Self {
        self.additional_fonts.push(Arc::new(font));
        self
    }
}

struct Initials {
    instance: Arc<Instance>,
    device: Arc<Device>,
    graphics_queue: Arc<device::Queue>,
    transfer_queue: Arc<device::Queue>,
    compute_queue: Arc<device::Queue>,
    secondary_graphics_queue: Option<Arc<device::Queue>>,
    secondary_transfer_queue: Option<Arc<device::Queue>>,
    secondary_compute_queue: Option<Arc<device::Queue>>,
    bin_stats: bool,
    options: BstOptions,
    window_manager: Arc<WindowManager>,
}

impl Initials {
    pub fn use_first_device(
        mut options: BstOptions,
        result_fn: Box<dyn Fn(Result<Arc<Basalt>, String>) + Send + Sync>,
    ) {
        let mut device_num: Option<usize> = None;
        let mut show_devices = false;
        let mut bin_stats = false;

        for arg in ::std::env::args() {
            if arg.starts_with("--use-device=") {
                let split_by_eq: Vec<_> = arg.split('=').collect();

                if split_by_eq.len() < 2 {
                    println!("Incorrect '--use-device' usage. Example: '--use-device=2'");
                    break;
                } else {
                    device_num = Some(match split_by_eq[1].parse() {
                        Ok(ok) => ok,
                        Err(_) => {
                            println!("Incorrect '--use-device' usage. Example: '--use-device=2'");
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
                let by_equal: Vec<_> = arg.split('=').collect();

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

        let vulkan_library = match VulkanLibrary::new() {
            Ok(ok) => ok,
            Err(e) => return result_fn(Err(format!("Failed to load vulkan library: {}", e))),
        };

        let instance_extensions = vulkan_library
            .supported_extensions()
            .intersection(&InstanceExtensions {
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
                ..InstanceExtensions::empty()
            })
            .union(&options.instance_extensions);

        let instance = match Instance::new(
            vulkan_library,
            InstanceCreateInfo {
                enabled_extensions: instance_extensions,
                enabled_layers: options.instance_layers.clone(),
                engine_name: Some(String::from("Basalt")),
                engine_version: Version {
                    major: 0,
                    minor: 15,
                    patch: 0,
                },
                ..InstanceCreateInfo::default()
            },
        ) {
            Ok(ok) => ok,
            Err(e) => return result_fn(Err(format!("Failed to create instance: {}", e))),
        };

        if instance.api_version() < Version::V1_2 {
            return result_fn(Err(String::from(
                "Basalt requires vulkan version 1.2 or above",
            )));
        }

        WindowManager::new(move |window_manager| {
            let mut physical_devices = match instance.enumerate_physical_devices() {
                Ok(ok) => ok.collect::<Vec<_>>(),
                Err(e) => {
                    return result_fn(Err(format!("Failed to enumerate physical devices: {}", e)))
                },
            };

            if show_devices {
                physical_devices.sort_by_key(|dev| dev.properties().device_id);

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
                Some(device_i) => {
                    if device_i >= physical_devices.len() {
                        return result_fn(Err(format!("No device found at index {}.", device_i)));
                    }

                    physical_devices.sort_by_key(|dev| dev.properties().device_id);
                    physical_devices.swap_remove(device_i)
                },
                None => {
                    if options.prefer_integrated_gpu {
                        physical_devices.sort_by_key(|dev| {
                            match dev.properties().device_type {
                                PhysicalDeviceType::DiscreteGpu => 4,
                                PhysicalDeviceType::IntegratedGpu => 5,
                                PhysicalDeviceType::VirtualGpu => 3,
                                PhysicalDeviceType::Other => 2,
                                PhysicalDeviceType::Cpu => 1,
                                _ => 0,
                            }
                        });
                    } else {
                        physical_devices.sort_by_key(|dev| {
                            match dev.properties().device_type {
                                PhysicalDeviceType::DiscreteGpu => 5,
                                PhysicalDeviceType::IntegratedGpu => 4,
                                PhysicalDeviceType::VirtualGpu => 3,
                                PhysicalDeviceType::Other => 2,
                                PhysicalDeviceType::Cpu => 1,
                                _ => 0,
                            }
                        });
                    }

                    match physical_devices.pop() {
                        Some(some) => some,
                        None => return result_fn(Err(String::from("No suitable device found."))),
                    }
                },
            };

            let mut queue_families: Vec<(u32, QueueFlags)> = physical_device
                .queue_family_properties()
                .iter()
                .enumerate()
                .flat_map(|(index, properties)| {
                    (0..properties.queue_count).map(move |_| (index as u32, properties.queue_flags))
                })
                .collect();

            // TODO: Use https://github.com/rust-lang/rust/issues/43244 when stable

            let mut g_optimal = misc::drain_filter(&mut queue_families, |(_, flags)| {
                flags.contains(QueueFlags::GRAPHICS) && !flags.contains(QueueFlags::COMPUTE)
            });

            let mut c_optimal = misc::drain_filter(&mut queue_families, |(_, flags)| {
                flags.contains(QueueFlags::COMPUTE) && !flags.contains(QueueFlags::GRAPHICS)
            });

            let mut t_optimal = misc::drain_filter(&mut queue_families, |(_, flags)| {
                flags.contains(QueueFlags::TRANSFER)
                    && !flags.intersects(QueueFlags::GRAPHICS | QueueFlags::COMPUTE)
            });

            let (g_primary, mut g_secondary) = match g_optimal.len() {
                0 => {
                    let mut g_suboptimal = misc::drain_filter(&mut queue_families, |(_, flags)| {
                        flags.contains(QueueFlags::GRAPHICS)
                    });

                    match g_suboptimal.len() {
                        0 => {
                            return result_fn(Err(String::from(
                                "Unable to find queue family suitable for graphics.",
                            )))
                        },
                        1 => (Some(g_suboptimal.pop().unwrap()), None),
                        2 => {
                            (
                                Some(g_suboptimal.pop().unwrap()),
                                Some(g_suboptimal.pop().unwrap()),
                            )
                        },
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
                    let mut g_suboptimal = misc::drain_filter(&mut queue_families, |(_, flags)| {
                        flags.contains(QueueFlags::GRAPHICS)
                    });

                    match g_suboptimal.len() {
                        0 => (Some(g_optimal.pop().unwrap()), None),
                        1 => {
                            (
                                Some(g_optimal.pop().unwrap()),
                                Some(g_suboptimal.pop().unwrap()),
                            )
                        },
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
                2 => {
                    (
                        Some(g_optimal.pop().unwrap()),
                        Some(g_optimal.pop().unwrap()),
                    )
                },
                _ => {
                    let ret = (
                        Some(g_optimal.pop().unwrap()),
                        Some(g_optimal.pop().unwrap()),
                    );

                    queue_families.append(&mut g_optimal);
                    ret
                },
            };

            let (c_primary, mut c_secondary) = match c_optimal.len() {
                0 => {
                    let mut c_suboptimal = misc::drain_filter(&mut queue_families, |(_, flags)| {
                        flags.contains(QueueFlags::COMPUTE)
                    });

                    match c_suboptimal.len() {
                        0 => {
                            if g_secondary
                                .as_ref()
                                .map(|(_, flags)| flags.contains(QueueFlags::COMPUTE))
                                .unwrap_or(false)
                            {
                                (Some(g_secondary.take().unwrap()), None)
                            } else {
                                if !g_primary.as_ref().unwrap().1.contains(QueueFlags::COMPUTE) {
                                    return result_fn(Err(String::from(
                                        "Unable to find queue family suitable for compute.",
                                    )));
                                }

                                (None, None)
                            }
                        },
                        1 => (Some(c_suboptimal.pop().unwrap()), None),
                        2 => {
                            (
                                Some(c_suboptimal.pop().unwrap()),
                                Some(c_suboptimal.pop().unwrap()),
                            )
                        },
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
                    let mut c_suboptimal = misc::drain_filter(&mut queue_families, |(_, flags)| {
                        flags.contains(QueueFlags::COMPUTE)
                    });

                    match c_suboptimal.len() {
                        0 => (Some(c_optimal.pop().unwrap()), None),
                        1 => {
                            (
                                Some(c_optimal.pop().unwrap()),
                                Some(c_suboptimal.pop().unwrap()),
                            )
                        },
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
                2 => {
                    (
                        Some(c_optimal.pop().unwrap()),
                        Some(c_optimal.pop().unwrap()),
                    )
                },
                _ => {
                    let ret = (
                        Some(c_optimal.pop().unwrap()),
                        Some(c_optimal.pop().unwrap()),
                    );

                    queue_families.append(&mut c_optimal);
                    ret
                },
            };

            let (t_primary, t_secondary) = match t_optimal.len() {
                0 => {
                    match queue_families.len() {
                        0 => {
                            match c_secondary.take() {
                                Some(some) => (Some(some), None),
                                None => (None, None),
                            }
                        },
                        1 => (Some(queue_families.pop().unwrap()), None),
                        _ => {
                            (
                                Some(queue_families.pop().unwrap()),
                                Some(queue_families.pop().unwrap()),
                            )
                        },
                    }
                },
                1 => {
                    match queue_families.len() {
                        0 => (Some(t_optimal.pop().unwrap()), None),
                        _ => {
                            (
                                Some(t_optimal.pop().unwrap()),
                                Some(queue_families.pop().unwrap()),
                            )
                        },
                    }
                },
                _ => {
                    (
                        Some(t_optimal.pop().unwrap()),
                        Some(t_optimal.pop().unwrap()),
                    )
                },
            };

            let g_count: usize = 1 + g_secondary.as_ref().map(|_| 1).unwrap_or(0);
            let c_count: usize = c_primary.as_ref().map(|_| 1).unwrap_or(0)
                + c_secondary.as_ref().map(|_| 1).unwrap_or(0);
            let t_count: usize = t_primary.as_ref().map(|_| 1).unwrap_or(0)
                + t_secondary.as_ref().map(|_| 1).unwrap_or(0);

            println!("[Basalt]: VK Queues [{}/{}/{}]", g_count, c_count, t_count);

            // Item = (QueueFamilyIndex, [(Binding, Weight)])
            // 0 gp, 1 gs, 2 cp, 3 cs, 4 tp, 5 ts
            let mut family_map: Vec<(u32, Vec<(usize, f32)>)> = Vec::new();

            // discreteQueuePriorities is the number of discrete priorities that can be
            // assigned to a queue based on the value of each member of
            // VkDeviceQueueCreateInfo::pQueuePriorities. This must be at least 2, and
            // levels must be spread evenly over the range, with at least one level at 1.0,
            // and another at 0.0.

            let (high_p, med_p, low_p) = match physical_device
                .properties()
                .discrete_queue_priorities
                .max(2)
            {
                2 => (1.0, 0.0, 0.0),
                _ => (1.0, 0.5, 0.0),
            };

            'iter_queues: for (family_op, binding, priority) in vec![
                (g_primary, 0, high_p),
                (g_secondary, 1, med_p),
                (c_primary, 2, med_p),
                (c_secondary, 3, low_p),
                (t_primary, 4, med_p),
                (t_secondary, 5, low_p),
            ]
            .into_iter()
            {
                if let Some((family_index, _)) = family_op {
                    for family_item in family_map.iter_mut() {
                        if family_item.0 == family_index {
                            family_item.1.push((binding, priority));
                            continue 'iter_queues;
                        }
                    }

                    family_map.push((family_index, vec![(binding, priority)]));
                }
            }

            // Item = (binding, queue_index)
            let mut queue_map: Vec<(usize, usize)> = Vec::new();
            let mut queue_count = 0;

            let queue_request: Vec<QueueCreateInfo> = family_map
                .into_iter()
                .map(|(family_index, members)| {
                    let mut priorites = Vec::with_capacity(members.len());

                    for (binding, priority) in members.into_iter() {
                        queue_map.push((binding, queue_count));
                        queue_count += 1;
                        priorites.push(priority);
                    }

                    QueueCreateInfo {
                        queues: priorites,
                        queue_family_index: family_index,
                        ..Default::default()
                    }
                })
                .collect();

            let (device, queues) = match Device::new(
                physical_device,
                DeviceCreateInfo {
                    enabled_extensions: options.device_extensions,
                    enabled_features: options.features,
                    queue_create_infos: queue_request,
                    ..DeviceCreateInfo::default()
                },
            ) {
                Ok(ok) => ok,
                Err(e) => return result_fn(Err(format!("Failed to create device: {}", e))),
            };

            if queues.len() != queue_map.len() {
                return result_fn(Err(String::from(
                    "Returned queues length != expected length",
                )));
            }

            let mut queues: Vec<Option<Arc<device::Queue>>> =
                queues.into_iter().map(Some).collect();
            let mut graphics_queue = None;
            let mut secondary_graphics_queue = None;
            let mut compute_queue = None;
            let mut secondary_compute_queue = None;
            let mut transfer_queue = None;
            let mut secondary_transfer_queue = None;

            for (binding, queue_index) in queue_map.into_iter() {
                let queue = Some(queues[queue_index].take().unwrap());

                match binding {
                    0 => graphics_queue = queue,
                    1 => secondary_graphics_queue = queue,
                    2 => compute_queue = queue,
                    3 => secondary_compute_queue = queue,
                    4 => transfer_queue = queue,
                    5 => secondary_transfer_queue = queue,
                    _ => unreachable!(),
                }
            }

            let graphics_queue = graphics_queue.unwrap();

            let compute_queue = match compute_queue {
                Some(some) => some,
                None => {
                    println!("[Basalt]: Warning graphics queue and compute queue are the same.");
                    graphics_queue.clone()
                },
            };

            let transfer_queue = match transfer_queue {
                Some(some) => some,
                None => {
                    println!("[Basalt]: Warning compute queue and transfer queue are the same.");
                    compute_queue.clone()
                },
            };

            let basalt = match Basalt::from_initials(Initials {
                device,
                graphics_queue,
                transfer_queue,
                compute_queue,
                secondary_graphics_queue,
                secondary_transfer_queue,
                secondary_compute_queue,
                instance: instance.clone(), // Why?
                bin_stats,
                options: options.clone(),
                window_manager,
            }) {
                Ok(ok) => ok,
                Err(e) => return result_fn(Err(format!("Failed to initialize Basalt: {}", e))),
            };

            if options.app_loop {
                let bst = basalt.clone();
                *basalt.loop_thread.lock() = Some(thread::spawn(move || bst.app_loop()));
            }

            result_fn(Ok(basalt))
        });
    }
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
    instance: Arc<Instance>,
    interface: Arc<Interface>,
    input: Input,
    interval: Arc<Interval>,
    image_cache: Arc<ImageCache>,
    window_manager: Arc<WindowManager>,
    wants_exit: AtomicBool,
    loop_thread: Mutex<Option<JoinHandle<Result<(), String>>>>,
    options: BstOptions,
    bin_stats: bool,
}

#[allow(dead_code)]
impl Basalt {
    /// Begin initializing Basalt, this thread will be taken for window event polling and the
    /// function provided in `result_fn` will be executed after Basalt initialization has
    /// completed or errored.
    pub fn initialize(
        options: BstOptions,
        result_fn: Box<dyn Fn(Result<Arc<Self>, String>) + Send + Sync>,
    ) {
        if options.force_unix_backend_x11 && cfg!(unix) {
            std::env::set_var("WINIT_UNIX_BACKEND", "x11");
        }

        Initials::use_first_device(options, result_fn)
    }

    fn from_initials(initials: Initials) -> Result<Arc<Self>, String> {
        let interface = Interface::new();
        let interval = Arc::new(Interval::new());
        let input = Input::new(interface.clone(), interval.clone());

        let basalt_ret = Arc::new(Basalt {
            device: initials.device,
            graphics_queue: initials.graphics_queue,
            transfer_queue: initials.transfer_queue,
            compute_queue: initials.compute_queue,
            secondary_graphics_queue: initials.secondary_graphics_queue,
            secondary_transfer_queue: initials.secondary_transfer_queue,
            secondary_compute_queue: initials.secondary_compute_queue,
            instance: initials.instance,
            interface,
            input,
            interval,
            image_cache: Arc::new(ImageCache::new()),
            window_manager: initials.window_manager,
            wants_exit: AtomicBool::new(false),
            loop_thread: Mutex::new(None),
            options: initials.options,
            bin_stats: initials.bin_stats,
        });

        basalt_ret.interface.associate_basalt(basalt_ret.clone());
        basalt_ret
            .window_manager
            .associate_basalt(basalt_ret.clone());

        // TODO: Create the initial window.

        Ok(basalt_ret)
    }

    /// Obtain a reference of `Input`
    pub fn input_ref(&self) -> &Input {
        &self.input
    }

    /// Obtain a copy of `Arc<Interval>`
    pub fn interval(&self) -> Arc<Interval> {
        self.interval.clone()
    }

    /// Obtain a reference of `Arc<Interval>`
    pub fn interval_ref(&self) -> &Arc<Interval> {
        &self.interval
    }

    /// Obtain a copy of `Arc<Interface>`
    pub fn interface(&self) -> Arc<Interface> {
        self.interface.clone()
    }

    /// Obtain a reference of `Arc<Interface>`
    pub fn interface_ref(&self) -> &Arc<Interface> {
        &self.interface
    }

    /// Obtain a copy of `Arc<ImageCache>`
    pub fn image_cache(&self) -> Arc<ImageCache> {
        self.image_cache.clone()
    }

    /// Obtain a refernce of `Arc<ImageCache>`
    pub fn image_cache_ref(&self) -> &Arc<ImageCache> {
        &self.image_cache
    }

    /// Obtain a copy of `Arc<WindowManager>`
    pub fn window_manager(&self) -> Arc<WindowManager> {
        self.window_manager.clone()
    }

    /// Obtain a reference of `Arc<WindowManager>`
    pub fn window_manager_ref(&self) -> &Arc<WindowManager> {
        &self.window_manager
    }

    /// Obtain the `BstOptions` that basalt was created with.
    pub fn options(&self) -> BstOptions {
        self.options.clone()
    }

    /// Obtain a reference to the `BstOptions` that basalt was created with.
    pub fn options_ref(&self) -> &BstOptions {
        &self.options
    }

    /// Obtain a copy of `Arc<Instance>`
    pub fn instance(&self) -> Arc<Instance> {
        self.instance.clone()
    }

    /// Obtain a reference of `Arc<Instance>`
    pub fn instance_ref(&self) -> &Arc<Instance> {
        &self.instance
    }

    /// Obtain a copy of `Arc<PhysicalDevice>`
    pub fn physical_device(&self) -> Arc<PhysicalDevice> {
        self.device.physical_device().clone()
    }

    /// Obtain a reference of `Arc<PhysicalDevice>`
    pub fn physical_device_ref(&self) -> &Arc<PhysicalDevice> {
        self.device.physical_device()
    }

    /// Obtain a copy of `Arc<Devcie>`
    pub fn device(&self) -> Arc<Device> {
        self.device.clone()
    }

    /// Obtain a refernce of `Arc<Device>`
    pub fn device_ref(&self) -> &Arc<Device> {
        &self.device
    }

    /// Obtain a copy of the `Arc<Queue>` assigned for graphics operations.
    pub fn graphics_queue(&self) -> Arc<device::Queue> {
        self.graphics_queue.clone()
    }

    /// Obtain a reference of the `Arc<Queue>` assigned for graphics operations.
    pub fn graphics_queue_ref(&self) -> &Arc<device::Queue> {
        &self.graphics_queue
    }

    /// Obtain a copy of the `Arc<Queue>` assigned for secondary graphics operations.
    pub fn secondary_graphics_queue(&self) -> Option<Arc<device::Queue>> {
        self.secondary_graphics_queue.clone()
    }

    /// Obtain a reference of the `Arc<Queue>` assigned for secondary graphics operations.
    pub fn secondary_graphics_queue_ref(&self) -> Option<&Arc<device::Queue>> {
        self.secondary_graphics_queue.as_ref()
    }

    /// Obtain a copy of the `Arc<Queue>` assigned for compute operations.
    ///
    /// # Notes:
    /// - This queue may be the same as the graphics queue in cases where the device only
    /// has a single queue present.
    pub fn compute_queue(&self) -> Arc<device::Queue> {
        self.compute_queue.clone()
    }

    /// Obtain a reference of the `Arc<Queue>` assigned for compute operations.
    ///
    /// # Notes:
    /// - This queue may be the same as the graphics queue in cases where the device only
    /// has a single queue present.
    pub fn compute_queue_ref(&self) -> &Arc<device::Queue> {
        &self.compute_queue
    }

    /// Obtain a copy of the `Arc<Queue>` assigned for secondary compute operations.
    pub fn secondary_compute_queue(&self) -> Option<Arc<device::Queue>> {
        self.secondary_compute_queue.clone()
    }

    /// Obtain a reference of the `Arc<Queue>` assigned for secondary compute operations.
    pub fn secondary_compute_queue_ref(&self) -> Option<&Arc<device::Queue>> {
        self.secondary_compute_queue.as_ref()
    }

    /// Obtain a copy of the `Arc<Queue>` assigned for transfers.
    ///
    /// # Notes:
    /// - This queue may be the same as the compute queue in cases where the device only
    /// has two queues present. In cases where there is only one queue the graphics, compute,
    /// and transfer queues will all be the same queue.
    pub fn transfer_queue(&self) -> Arc<device::Queue> {
        self.transfer_queue.clone()
    }

    /// Obtain a reference of the `Arc<Queue>` assigned for transfers.
    ///
    /// # Notes:
    /// - This queue may be the same as the compute queue in cases where the device only
    /// has two queues present. In cases where there is only one queue the graphics, compute,
    /// and transfer queues will all be the same queue.
    pub fn transfer_queue_ref(&self) -> &Arc<device::Queue> {
        &self.transfer_queue
    }

    /// Obtain a copy of the `Arc<Queue>` assigned for secondary transfers.
    pub fn secondary_transfer_queue(&self) -> Option<Arc<device::Queue>> {
        self.secondary_transfer_queue.clone()
    }

    /// Obtain a reference of the `Arc<Queue>` assigned for secondary transfers.
    pub fn secondary_transfer_queue_ref(&self) -> Option<&Arc<device::Queue>> {
        self.secondary_transfer_queue.as_ref()
    }

    /// Signal the application to exit.
    pub fn exit(&self) {
        self.wants_exit.store(true, atomic::Ordering::Relaxed);
    }

    /// Check if basalt is attempting to exit.
    pub fn wants_exit(&self) -> bool {
        self.wants_exit.load(atomic::Ordering::Relaxed)
    }

    /// Wait for the application to exit.
    ///
    /// # Notes:
    /// - Always returns `Ok` if not configured for app_loop or the application has already closed.
    pub fn wait_for_exit(&self) -> Result<(), String> {
        if let Some(handle) = self.loop_thread.lock().take() {
            match handle.join() {
                Ok(ok) => ok,
                Err(_) => Err(String::from("Failed to join loop thread.")),
            }
        } else {
            Ok(())
        }
    }

    fn app_loop(self: &Arc<Self>) -> Result<(), String> {
        Ok(())
    }
}

impl std::fmt::Debug for Basalt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Basalt").finish()
    }
}
