#![allow(clippy::significant_drop_in_scrutinee)]
#![allow(clippy::type_complexity)]

#[macro_use]
pub extern crate vulkano;
#[macro_use]
pub extern crate vulkano_shaders;

pub mod atlas;
pub mod image_view;
pub mod input;
pub mod interface;
pub mod interval;
pub mod misc;
pub mod window;

use std::num::NonZeroUsize;
use std::str::FromStr;
use std::sync::atomic::{self, AtomicBool, AtomicUsize};
use std::sync::Arc;
use std::thread;
use std::thread::{available_parallelism, JoinHandle};
use std::time::{Duration, Instant};

use atlas::Atlas;
use crossbeam::channel::{self, Receiver, Sender};
use interface::bin::BinUpdateStats;
use interface::Interface;
use parking_lot::Mutex;
use vulkano::command_buffer::allocator::{
    StandardCommandBufferAllocator, StandardCommandBufferAllocatorCreateInfo,
};
use vulkano::command_buffer::{AutoCommandBufferBuilder, CommandBufferUsage, CopyImageInfo};
use vulkano::device::physical::{PhysicalDevice, PhysicalDeviceType};
use vulkano::device::{
    self, Device, DeviceCreateInfo, DeviceExtensions, Features as VkFeatures, QueueCreateInfo,
    QueueFlags,
};
use vulkano::format::{Format as VkFormat, FormatFeatures};
use vulkano::image::view::ImageView;
use vulkano::image::{ImageAccess, ImageDimensions, ImageUsage};
use vulkano::instance::{Instance, InstanceCreateInfo, InstanceExtensions, Version};
use vulkano::swapchain::{
    self, ColorSpace as VkColorSpace, CompositeAlpha, FullScreenExclusive, PresentMode, Surface,
    SurfaceCapabilities, SurfaceInfo, Swapchain, SwapchainCreateInfo, SwapchainCreationError,
    SwapchainPresentInfo,
};
use vulkano::sync::GpuFuture;
use vulkano::VulkanLibrary;
use window::{BasaltWindow, BstWindowHooks};

use crate::input::{Input, Qwerty};
use crate::interface::{BstMSAALevel, InterfaceInit, ItfDrawTarget};
use crate::interval::Interval;
use crate::window::BstWindowID;

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
    conservative_draw: bool,
    bin_parallel_threads: NonZeroUsize,
    additional_fonts: Vec<Arc<dyn AsRef<[u8]> + Sync + Send>>,
}

impl Default for BstOptions {
    fn default() -> Self {
        Self {
            ignore_dpi: false,
            window_size: [1920, 1080],
            title: "Basalt".to_string(),
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

    /// Set the inner size of the window to be created
    ///
    /// **Default**: `1920`, `1080`
    pub fn window_size(mut self, width: u32, height: u32) -> Self {
        self.window_size = [width, height];
        self
    }

    /// Set the title of the window to be created
    ///
    /// **Default**: `"Basalt"`
    pub fn title<T: AsRef<str>>(mut self, title: T) -> Self {
        self.title = String::from(title.as_ref());
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BstFormatsInUse {
    pub atlas: VkFormat,
    pub interface: VkFormat,
    pub swapchain: VkFormat,
    pub swapchain_colorspace: VkColorSpace,
}

struct Initials {
    device: Arc<Device>,
    graphics_queue: Arc<device::Queue>,
    transfer_queue: Arc<device::Queue>,
    compute_queue: Arc<device::Queue>,
    secondary_graphics_queue: Option<Arc<device::Queue>>,
    secondary_transfer_queue: Option<Arc<device::Queue>>,
    secondary_compute_queue: Option<Arc<device::Queue>>,
    surface: Arc<Surface>,
    window: Arc<dyn BasaltWindow>,
    window_size: [u32; 2],
    bin_stats: bool,
    options: BstOptions,
    formats_in_use: BstFormatsInUse,
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

        window::open_surface(
            options.clone(),
            BstWindowID(0),
            instance.clone(),
            Box::new(move |surface_result| {
                let (surface, window) = match surface_result {
                    Ok(ok) => ok,
                    Err(e) => return result_fn(Err(format!("Failed to create surface: {}", e))),
                };

                let mut physical_devices = match instance.enumerate_physical_devices() {
                    Ok(ok) => ok.collect::<Vec<_>>(),
                    Err(e) => {
                        return result_fn(Err(format!(
                            "Failed to enumerate physical devices: {}",
                            e
                        )))
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
                            return result_fn(Err(format!(
                                "No device found at index {}.",
                                device_i
                            )));
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
                            None => {
                                return result_fn(Err(String::from("No suitable device found.")))
                            },
                        }
                    },
                };

                let mut queue_families: Vec<(u32, QueueFlags)> = physical_device
                    .queue_family_properties()
                    .iter()
                    .enumerate()
                    .flat_map(|(index, properties)| {
                        (0..properties.queue_count)
                            .map(move |_| (index as u32, properties.queue_flags))
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
                        let mut g_suboptimal =
                            misc::drain_filter(&mut queue_families, |(_, flags)| {
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
                        let mut g_suboptimal =
                            misc::drain_filter(&mut queue_families, |(_, flags)| {
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
                        let mut c_suboptimal =
                            misc::drain_filter(&mut queue_families, |(_, flags)| {
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
                                    if !g_primary.as_ref().unwrap().1.contains(QueueFlags::COMPUTE)
                                    {
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
                        let mut c_suboptimal =
                            misc::drain_filter(&mut queue_families, |(_, flags)| {
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
                        println!(
                            "[Basalt]: Warning graphics queue and compute queue are the same."
                        );
                        graphics_queue.clone()
                    },
                };

                let transfer_queue = match transfer_queue {
                    Some(some) => some,
                    None => {
                        println!(
                            "[Basalt]: Warning compute queue and transfer queue are the same."
                        );
                        compute_queue.clone()
                    },
                };

                let pref_format_colorspace = vec![
                    (VkFormat::B8G8R8A8_SRGB, VkColorSpace::SrgbNonLinear),
                    (VkFormat::B8G8R8A8_SRGB, VkColorSpace::SrgbNonLinear),
                ];

                let mut swapchain_format_op = None;
                let surface_formats = device
                    .physical_device()
                    .surface_formats(&surface, SurfaceInfo::default())
                    .unwrap();

                for (a, b) in &pref_format_colorspace {
                    for (c, d) in surface_formats.iter() {
                        if a == c && b == d {
                            swapchain_format_op = Some((*a, *b));
                            break;
                        }
                    }
                    if swapchain_format_op.is_some() {
                        break;
                    }
                }

                if swapchain_format_op.is_none() {
                    return result_fn(Err(String::from(
                        "Unable to find a suitable format for the swapchain.",
                    )));
                }

                let (swapchain_format, swapchain_colorspace) = swapchain_format_op.unwrap();

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
                    let properties = match device.physical_device().format_properties(*f) {
                        Ok(ok) => ok,
                        Err(e) => {
                            println!(
                                "[Basalt][Warning]: failed to get format properties for {:?}: {}",
                                f, e
                            );
                            return false;
                        },
                    };

                    properties.optimal_tiling_features.contains(
                        FormatFeatures::SAMPLED_IMAGE
                            | FormatFeatures::STORAGE_IMAGE
                            | FormatFeatures::BLIT_DST
                            | FormatFeatures::TRANSFER_DST
                            | FormatFeatures::TRANSFER_SRC,
                    )
                });

                interface_formats.retain(|f| {
                    let properties = match device.physical_device().format_properties(*f) {
                        Ok(ok) => ok,
                        Err(e) => {
                            println!(
                                "[Basalt][Warning]: failed to get format properties for {:?}: {}",
                                f, e
                            );
                            return false;
                        },
                    };

                    properties
                        .optimal_tiling_features
                        .contains(FormatFeatures::SAMPLED_IMAGE | FormatFeatures::COLOR_ATTACHMENT)
                });

                if atlas_formats.is_empty() {
                    return result_fn(Err(String::from(
                        "Unable to find a suitable format for the atlas.",
                    )));
                }

                let interface_format = if options.app_loop && options.conservative_draw {
                    swapchain_format
                } else if interface_formats.is_empty() {
                    return result_fn(Err(String::from(
                        "Unable to find a suitable format for the interface.",
                    )));
                } else {
                    interface_formats.remove(0)
                };

                let formats_in_use = BstFormatsInUse {
                    atlas: atlas_formats.remove(0),
                    interface: interface_format,
                    swapchain: swapchain_format,
                    swapchain_colorspace,
                };

                let mut present_queue_family_indexes = Vec::with_capacity(2);
                present_queue_family_indexes.push(graphics_queue.queue_family_index());

                if let Some(queue) = secondary_graphics_queue.as_ref() {
                    present_queue_family_indexes.push(queue.queue_family_index());
                }

                present_queue_family_indexes.dedup();

                for index in present_queue_family_indexes {
                    match device.physical_device().surface_support(index, &surface) {
                        Ok(supported) if !supported => {
                            return result_fn(Err(String::from(
                                "Queue family doesn't support presentation on surface.",
                            )))
                        },
                        Err(e) => {
                            return result_fn(Err(format!(
                                "Failed to check presentation support for queue family: {:?}",
                                e
                            )))
                        },
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
                    window,
                    window_size: options.window_size,
                    bin_stats,
                    options: options.clone(),
                    formats_in_use,
                }) {
                    Ok(ok) => ok,
                    Err(e) => return result_fn(Err(format!("Failed to initialize Basalt: {}", e))),
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BstWinEv {
    Resized(u32, u32),
    ScaleChanged,
    RedrawRequest,
    FullScreenExclusive(bool),
}

impl BstWinEv {
    pub fn requires_swapchain_recreate(&self) -> bool {
        match self {
            Self::Resized(..) => true,
            Self::ScaleChanged => true,
            Self::RedrawRequest => true, // TODO: Is swapchain recreate required or just a
            // new frame?
            Self::FullScreenExclusive(_) => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum BstAppEvent {
    Normal(BstEvent),
    SwapchainPropertiesChanged,
    ExternalForceUpdate,
    DumpAtlasImages,
}

#[derive(Clone)]
enum BstEventSend {
    App(Sender<BstAppEvent>),
    Normal(Sender<BstEvent>),
}

impl BstEventSend {
    fn send(&self, event: BstEvent) {
        match self {
            Self::App(s) => s.send(BstAppEvent::Normal(event)).unwrap(),
            Self::Normal(s) => s.send(event).unwrap(),
        }
    }
}

#[derive(Clone)]
enum BstEventRecv {
    App(Receiver<BstAppEvent>),
    Normal(Receiver<BstEvent>),
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
    surface: Arc<Surface>,
    window: Arc<dyn BasaltWindow>,
    fps: AtomicUsize,
    gpu_time: AtomicUsize,
    cpu_time: AtomicUsize,
    bin_time: AtomicUsize,
    interface: Arc<Interface>,
    atlas: Arc<Atlas>,
    input: Input,
    interval: Arc<Interval>,
    wants_exit: AtomicBool,
    loop_thread: Mutex<Option<JoinHandle<Result<(), String>>>>,
    vsync: Mutex<bool>,
    window_size: Mutex<[u32; 2]>,
    options: BstOptions,
    ignore_dpi_data: Mutex<Option<(usize, Instant, u32, u32)>>,
    bin_stats: bool,
    event_recv: BstEventRecv,
    event_send: BstEventSend,
    formats_in_use: BstFormatsInUse,
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
        let (event_send, event_recv) = if initials.options.app_loop {
            let (s, r) = channel::unbounded();
            (BstEventSend::App(s), BstEventRecv::App(r))
        } else {
            let (s, r) = channel::unbounded();
            (BstEventSend::Normal(s), BstEventRecv::Normal(r))
        };

        let atlas = Atlas::new(
            initials
                .secondary_graphics_queue
                .clone()
                .unwrap_or_else(|| initials.graphics_queue.clone()),
            initials.formats_in_use.atlas,
            initials
                .device
                .physical_device()
                .properties()
                .max_image_dimension2_d,
        );

        let interface = Interface::new(InterfaceInit {
            options: initials.options.clone(),
            device: initials.device.clone(),
            transfer_queue: initials.transfer_queue.clone(),
            compute_queue: initials.compute_queue.clone(),
            itf_format: initials.formats_in_use.interface,
            atlas: atlas.clone(),
            window: initials.window.clone(),
        });

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
            surface: initials.surface,
            window: initials.window,
            fps: AtomicUsize::new(0),
            cpu_time: AtomicUsize::new(0),
            gpu_time: AtomicUsize::new(0),
            bin_time: AtomicUsize::new(0),
            interface,
            atlas,
            input,
            interval,
            wants_exit: AtomicBool::new(false),
            loop_thread: Mutex::new(None),
            vsync: Mutex::new(true),
            window_size: Mutex::new(initials.window_size),
            options: initials.options,
            ignore_dpi_data: Mutex::new(None),
            bin_stats: initials.bin_stats,
            event_recv,
            event_send: event_send.clone(),
            formats_in_use: initials.formats_in_use,
        });

        unsafe {
            basalt_ret.window_ref().attach_basalt(basalt_ret.clone());
        }

        basalt_ret.interface.attach_basalt(basalt_ret.clone());
        let is_app_loop = basalt_ret.options.app_loop;

        basalt_ret
            .window_ref()
            .on_press(Qwerty::F1, move |_, _, _| {
                if is_app_loop {
                    let mut output = String::new();
                    output.push_str("\r\n[Basalt]: Built-In Bindings:");
                    output.push_str("  F1: Prints keys used by basalt\r\n");
                    output.push_str("  F2: Prints fps while held\r\n");
                    output.push_str("  F3: Prints bin update stats\r\n");
                    output.push_str("  F7: Decreases msaa level\r\n");
                    output.push_str("  F8: Increases msaa level\r\n");
                    output.push_str("  F10: Toggles vsync\r\n");
                    output.push_str("  F11: Toggles fullscreen\r\n");
                    output.push_str("  LCtrl + Dash: Decreases ui scale\r\n");
                    output.push_str("  LCtrl + Equal: Increaes ui scale\r\n\r\n");
                    println!("{}", output);
                } else {
                    let mut output = String::new();
                    output.push_str("\r\n[Basalt]: Built-In Bindings:");
                    output.push_str("  F1: Prints keys used by basalt\r\n");
                    output.push_str("  F3: Prints bin update stats\r\n");
                    output.push_str("  F7: Decreases msaa level\r\n");
                    output.push_str("  F8: Increases msaa level\r\n");
                    output.push_str("  F11: Toggles fullscreen\r\n");
                    output.push_str("  LCtrl + Dash: Decreases ui scale\r\n");
                    output.push_str("  LCtrl + Equal: Increaes ui scale\r\n\r\n");
                }

                Default::default()
            });

        let basalt = basalt_ret.clone();

        basalt_ret
            .input_ref()
            .hook()
            .window(basalt_ret.window_ref())
            .on_hold()
            .keys(Qwerty::F2)
            .interval(Duration::from_millis(100))
            .call(move |_, _, _| {
                if is_app_loop {
                    println!(
                        "[Basalt]: FPS: {}, GPU Time: {:.2} ms, CPU Time: {:.2} ms, BIN Time: \
                         {:.2} ms",
                        basalt.fps(),
                        basalt.gpu_time.load(atomic::Ordering::Relaxed) as f32 / 1000.0,
                        basalt.cpu_time.load(atomic::Ordering::Relaxed) as f32 / 1000.0,
                        basalt.interface_ref().composer_ref().bin_time() as f32 / 1000.0,
                    );
                } else {
                    println!(
                        "[Basalt]: CPU Time: {:.2} ms, BIN Time: {:.2} ms",
                        basalt.cpu_time.load(atomic::Ordering::Relaxed) as f32 / 1000.0,
                        basalt.interface_ref().composer_ref().bin_time() as f32 / 1000.0,
                    );
                }

                Default::default()
            })
            .finish()
            .unwrap();

        if is_app_loop {
            let s = event_send.clone();

            basalt_ret
                .window_ref()
                .on_press(Qwerty::F9, move |_, _, _| {
                    if let BstEventSend::App(s) = &s {
                        s.send(BstAppEvent::DumpAtlasImages).unwrap();
                    } else {
                        unreachable!()
                    }

                    Default::default()
                });
        }

        let window = basalt_ret.window();

        basalt_ret
            .window_ref()
            .on_press(Qwerty::F11, move |_, _, _| {
                window.toggle_fullscreen();
                Default::default()
            });

        let interface = basalt_ret.interface.clone();
        let bin_stats = basalt_ret.bin_stats;

        basalt_ret
            .window_ref()
            .on_press(Qwerty::F3, move |_, _, _| {
                if bin_stats {
                    let bins = interface.bins();
                    let count = bins.len();
                    let sum = BinUpdateStats::sum(&bins.iter().map(|v| v.update_stats()).collect());
                    let avg = sum.divide(count as f32);
                    println!("[Basalt]: Total Bins: {}", count);
                    println!("[Basalt]: Bin Update Time Sum: {:?}\r\n", sum);
                    println!("[Basalt]: Bin Update Time Average: {:?}\r\n", avg);
                } else {
                    println!("[Basalt]: Bin Stats Disabled. Launch with --binstats");
                }

                Default::default()
            });

        let interface = basalt_ret.interface.clone();

        basalt_ret
            .window_ref()
            .on_press(Qwerty::F7, move |_, _, _| {
                let msaa = interface.decrease_msaa();
                println!("[Basalt]: MSAA set to {}X", msaa.as_u32());
                Default::default()
            });

        let interface = basalt_ret.interface.clone();

        basalt_ret
            .window_ref()
            .on_press(Qwerty::F8, move |_, _, _| {
                let msaa = interface.increase_msaa();
                println!("[Basalt]: MSAA set to {}X", msaa.as_u32());
                Default::default()
            });

        if is_app_loop {
            let s = event_send;
            let basalt = basalt_ret.clone();

            basalt_ret
                .window_ref()
                .on_press(Qwerty::F10, move |_, _, _| {
                    if let BstEventSend::App(s) = &s {
                        let mut vsync = basalt.vsync.lock();
                        *vsync = !*vsync;
                        s.send(BstAppEvent::SwapchainPropertiesChanged).unwrap();

                        if *vsync {
                            println!("[Basalt]: VSync Enabled");
                        } else {
                            println!("[Basalt]: VSync Disabled");
                        }
                    } else {
                        unreachable!()
                    }

                    Default::default()
                });
        }

        let interface = basalt_ret.interface.clone();

        basalt_ret
            .window_ref()
            .on_press([Qwerty::LCtrl, Qwerty::Dash], move |_, _, _| {
                let mut scale = interface.current_scale();
                scale -= 0.05;

                if scale < 0.05 {
                    scale = 0.05;
                }

                interface.set_scale(scale);
                println!("[Basalt]: Current Inteface Scale: {:.1} %", scale * 100.0);
                Default::default()
            });

        let interface = basalt_ret.interface.clone();

        basalt_ret
            .window_ref()
            .on_press([Qwerty::LCtrl, Qwerty::Equal], move |_, _, _| {
                let mut scale = interface.current_scale();
                scale += 0.05;

                if scale > 4.0 {
                    scale = 4.0;
                }

                interface.set_scale(scale);
                println!("[Basalt]: Current Inteface Scale: {:.1} %", scale * 100.0);
                Default::default()
            });

        Ok(basalt_ret)
    }

    /// # Panics:
    /// - Panics if the current cofiguration is an app_loop.
    pub fn poll_events(&self) -> Vec<BstEvent> {
        match &self.event_recv {
            BstEventRecv::App(_) => {
                panic!("Basalt::poll_events() only allowed in non-app_loop aapplications.")
            },
            BstEventRecv::Normal(r) => r.try_iter().collect(),
        }
    }

    pub(crate) fn send_event(&self, event: BstEvent) {
        self.event_send.send(event);
    }

    fn show_bin_stats(&self) -> bool {
        self.bin_stats
    }

    pub fn input_ref(&self) -> &Input {
        &self.input
    }

    pub fn interval(&self) -> Arc<Interval> {
        self.interval.clone()
    }

    pub fn interval_ref(&self) -> &Arc<Interval> {
        &self.interval
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

    /// # Notes:
    /// - This queue may be the same as the graphics queue in cases where the device only
    /// has a single queue present.
    pub fn compute_queue(&self) -> Arc<device::Queue> {
        self.compute_queue.clone()
    }

    /// # Notes:
    /// - This queue may be the same as the graphics queue in cases where the device only
    /// has a single queue present.
    pub fn compute_queue_ref(&self) -> &Arc<device::Queue> {
        &self.compute_queue
    }

    /// # Notes:
    /// - This queue may be the same as the compute queue in cases where the device only
    /// has two queues present. In cases where there is only one queue the graphics, compute,
    /// and transfer queues will all be the same queue.
    pub fn transfer_queue(&self) -> Arc<device::Queue> {
        self.transfer_queue.clone()
    }

    /// # Notes:
    /// - This queue may be the same as the compute queue in cases where the device only
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

    pub fn physical_device_ref(&self) -> &Arc<PhysicalDevice> {
        self.device.physical_device()
    }

    pub fn physical_device(&self) -> Arc<PhysicalDevice> {
        self.device.physical_device().clone()
    }

    fn fullscreen_exclusive_mode(&self) -> FullScreenExclusive {
        if self.options_ref().exclusive_fullscreen {
            FullScreenExclusive::ApplicationControlled
        } else {
            FullScreenExclusive::Default
        }
    }

    pub fn surface_capabilities(&self, fse: FullScreenExclusive) -> SurfaceCapabilities {
        self.physical_device()
            .surface_capabilities(
                &self.surface,
                match fse {
                    FullScreenExclusive::ApplicationControlled => {
                        SurfaceInfo {
                            full_screen_exclusive: FullScreenExclusive::ApplicationControlled,
                            win32_monitor: self.window_ref().win32_monitor(),
                            ..SurfaceInfo::default()
                        }
                    },
                    fse => {
                        SurfaceInfo {
                            full_screen_exclusive: fse,
                            ..SurfaceInfo::default()
                        }
                    },
                },
            )
            .unwrap()
    }

    pub fn surface_formats(&self, fse: FullScreenExclusive) -> Vec<(VkFormat, VkColorSpace)> {
        self.physical_device()
            .surface_formats(
                &self.surface,
                match fse {
                    FullScreenExclusive::ApplicationControlled => {
                        SurfaceInfo {
                            full_screen_exclusive: FullScreenExclusive::ApplicationControlled,
                            win32_monitor: self.window_ref().win32_monitor(),
                            ..SurfaceInfo::default()
                        }
                    },
                    fse => {
                        SurfaceInfo {
                            full_screen_exclusive: fse,
                            ..SurfaceInfo::default()
                        }
                    },
                },
            )
            .unwrap()
    }

    pub fn surface_present_modes(&self) -> Vec<PresentMode> {
        self.physical_device()
            .surface_present_modes(&self.surface)
            .unwrap()
            .collect()
    }

    pub fn instance(&self) -> Arc<Instance> {
        self.surface.instance().clone()
    }

    pub fn instance_ref(&self) -> &Arc<Instance> {
        self.surface.instance()
    }

    pub fn surface(&self) -> Arc<Surface> {
        self.surface.clone()
    }

    pub fn surface_ref(&self) -> &Arc<Surface> {
        &self.surface
    }

    /// Returns list of `Format`'s used by `Basalt`.
    pub fn formats_in_use(&self) -> BstFormatsInUse {
        self.formats_in_use.clone()
    }

    /// Get the current extent of the surface. In the case current extent is none, the window's
    /// inner dimensions will be used instead.
    pub fn current_extent(&self, fse: FullScreenExclusive) -> [u32; 2] {
        self.surface_capabilities(fse)
            .current_extent
            .unwrap_or_else(|| self.window_ref().inner_dimensions())
    }

    pub fn wants_exit(&self) -> bool {
        self.wants_exit.load(atomic::Ordering::Relaxed)
    }

    pub fn window(&self) -> Arc<dyn BasaltWindow> {
        self.window.clone()
    }

    pub fn window_ref(&self) -> &Arc<dyn BasaltWindow> {
        &self.window
    }

    pub fn options(&self) -> BstOptions {
        self.options.clone()
    }

    pub fn options_ref(&self) -> &BstOptions {
        &self.options
    }

    /// Signal the application to exit.
    pub fn exit(&self) {
        self.wants_exit.store(true, atomic::Ordering::Relaxed);
    }

    /// Retrieve the current FPS.
    ///
    /// # Notes:
    /// - Returns zero if not configured for app_loop.
    pub fn fps(&self) -> usize {
        self.fps.load(atomic::Ordering::Relaxed)
    }

    /// Trigger the Swapchain to be recreated.
    ///
    /// # Notes:
    /// - Does nothing if not configured for app_loop.
    pub fn force_recreate_swapchain(&self) {
        match &self.event_send {
            BstEventSend::App(s) => s.send(BstAppEvent::ExternalForceUpdate).unwrap(),
            BstEventSend::Normal(_) => {
                panic!("force_recreate_swapchain() can not be called on a normal application.")
            },
        }
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
        let mut win_size_x;
        let mut win_size_y;
        let mut swapchain_op: Option<Arc<Swapchain>> = None;
        let swapchain_format = self.formats_in_use.swapchain;
        let swapchain_colorspace = self.formats_in_use.swapchain_colorspace;

        println!(
            "[Basalt]: Swapchain {:?}/{:?}",
            swapchain_format, swapchain_colorspace
        );

        let mut previous_frame_future: Option<Box<dyn GpuFuture>> = None;
        let mut acquire_fullscreen_exclusive = false;

        let mut swapchain_usage = ImageUsage::COLOR_ATTACHMENT;

        if self.options_ref().conservative_draw {
            swapchain_usage |= ImageUsage::TRANSFER_DST;
        }

        let cmd_alloc = StandardCommandBufferAllocator::new(
            self.device(),
            StandardCommandBufferAllocatorCreateInfo {
                primary_buffer_count: 16,
                secondary_buffer_count: 1,
                ..Default::default()
            },
        );

        'resize: loop {
            let _: Vec<_> = match &self.event_recv {
                BstEventRecv::App(r) => r.try_iter().collect(),
                BstEventRecv::Normal(_) => unreachable!(),
            };

            let surface_capabilities = self.surface_capabilities(self.fullscreen_exclusive_mode());
            let surface_present_modes = self.surface_present_modes();

            let [x, y] = surface_capabilities
                .current_extent
                .unwrap_or_else(|| self.window_ref().inner_dimensions());

            win_size_x = x;
            win_size_y = y;
            *self.window_size.lock() = [x, y];

            if win_size_x == 0 || win_size_y == 0 {
                thread::sleep(Duration::from_millis(30));
                continue;
            }

            let present_mode = if *self.vsync.lock() {
                if surface_present_modes.contains(&PresentMode::FifoRelaxed) {
                    PresentMode::FifoRelaxed
                } else {
                    PresentMode::Fifo
                }
            } else if surface_present_modes.contains(&PresentMode::Mailbox) {
                PresentMode::Mailbox
            } else if surface_present_modes.contains(&PresentMode::Immediate) {
                PresentMode::Immediate
            } else {
                PresentMode::Fifo
            };

            let mut min_image_count = surface_capabilities.min_image_count;
            let max_image_count = surface_capabilities.max_image_count.unwrap_or(0);

            if max_image_count == 0 || min_image_count < max_image_count {
                min_image_count += 1;
            }

            let (swapchain, images) = match match swapchain_op.as_ref() {
                Some(old_swapchain) => {
                    old_swapchain.recreate(SwapchainCreateInfo {
                        min_image_count,
                        image_format: Some(swapchain_format),
                        image_extent: [x, y],
                        image_usage: swapchain_usage,
                        present_mode,
                        full_screen_exclusive: self.fullscreen_exclusive_mode(),
                        composite_alpha: self.options.composite_alpha,
                        ..SwapchainCreateInfo::default()
                    })
                },
                None => {
                    Swapchain::new(
                        self.device.clone(),
                        self.surface.clone(),
                        SwapchainCreateInfo {
                            min_image_count,
                            image_format: Some(swapchain_format),
                            image_extent: [x, y],
                            image_usage: swapchain_usage,
                            present_mode,
                            full_screen_exclusive: self.fullscreen_exclusive_mode(),
                            composite_alpha: self.options.composite_alpha,
                            ..SwapchainCreateInfo::default()
                        },
                    )
                },
            } {
                Ok(ok) => ok,
                Err(SwapchainCreationError::ImageExtentNotSupported {
                    ..
                }) => continue,
                Err(e) => return Err(format!("Basalt failed to recreate swapchain: {}", e)),
            };

            swapchain_op = Some(swapchain.clone());

            let images: Vec<_> = images
                .into_iter()
                .map(|image| ImageView::new_default(image).unwrap())
                .collect();

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

            let calc_time =
                |arr: &[u128; 10]| -> usize { (arr.iter().copied().sum::<u128>() / 10) as usize };

            let mut last_present = Instant::now();

            loop {
                if let Some(future) = previous_frame_future.as_mut() {
                    future.cleanup_finished();
                }

                let mut cpu_time_start = Instant::now();
                let mut recreate_swapchain_now = false;
                let mut dump_atlas_images = false;

                if let BstEventRecv::App(recv) = &self.event_recv {
                    for ev in recv.try_iter() {
                        match ev {
                            BstAppEvent::Normal(ev) => {
                                match ev {
                                    BstEvent::BstWinEv(win_ev) => {
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
                                                let [w, h] = self.current_extent(
                                                    self.fullscreen_exclusive_mode(),
                                                );

                                                if w != win_size_x || h != win_size_y {
                                                    recreate_swapchain_now = true;
                                                }
                                            },
                                            BstWinEv::FullScreenExclusive(exclusive) => {
                                                if exclusive {
                                                    acquire_fullscreen_exclusive = true;
                                                } else {
                                                    swapchain
                                                        .release_full_screen_exclusive()
                                                        .unwrap();
                                                }
                                            },
                                        }
                                    },
                                }
                            },
                            BstAppEvent::SwapchainPropertiesChanged => {
                                recreate_swapchain_now = true;
                            },
                            BstAppEvent::ExternalForceUpdate => {
                                recreate_swapchain_now = true;
                            },
                            BstAppEvent::DumpAtlasImages => {
                                dump_atlas_images = true;
                            },
                        }
                    }
                }

                if recreate_swapchain_now {
                    continue 'resize;
                }

                if acquire_fullscreen_exclusive && swapchain.acquire_full_screen_exclusive().is_ok()
                {
                    acquire_fullscreen_exclusive = false;
                    println!("Exclusive fullscreen acquired!");
                }

                cpu_times[cpu_times_i] = cpu_time_start.elapsed().as_micros();

                let (image_num, suboptimal, acquire_future) = match swapchain::acquire_next_image(
                    swapchain.clone(),
                    Some(::std::time::Duration::new(1, 0)),
                ) {
                    Ok(ok) => ok,
                    Err(_) => continue 'resize,
                };

                cpu_time_start = Instant::now();

                let cmd_buf = AutoCommandBufferBuilder::primary(
                    &cmd_alloc,
                    self.graphics_queue.queue_family_index(),
                    CommandBufferUsage::OneTimeSubmit,
                )
                .unwrap();

                if self.options_ref().conservative_draw {
                    let extent = match images[image_num as usize].image().dimensions() {
                        ImageDimensions::Dim2d {
                            width,
                            height,
                            ..
                        } => [width, height],
                        _ => unreachable!(),
                    };

                    let (mut cmd_buf, itf_image) = self.interface.draw(
                        cmd_buf,
                        ItfDrawTarget::Image {
                            extent,
                        },
                    );

                    cmd_buf
                        .copy_image(CopyImageInfo::images(
                            itf_image.unwrap(),
                            images[image_num as usize].image().clone(),
                        ))
                        .unwrap();
                    let cmd_buf = cmd_buf.build().unwrap();
                    cpu_times[cpu_times_i] += cpu_time_start.elapsed().as_micros();

                    match acquire_future
                        .then_execute(self.graphics_queue.clone(), cmd_buf)
                        .unwrap()
                        .then_swapchain_present(
                            self.graphics_queue.clone(),
                            SwapchainPresentInfo::swapchain_image_index(
                                swapchain.clone(),
                                image_num,
                            ),
                        )
                        .then_signal_fence_and_flush()
                    {
                        Ok(future) => {
                            future.wait(None).unwrap();

                            if dump_atlas_images {
                                self.atlas.dump();
                            }

                            previous_frame_future = None;
                        },
                        Err(vulkano::sync::FlushError::OutOfDate) => continue 'resize,
                        Err(e) => panic!("then_signal_fence_and_flush() {:?}", e),
                    }
                } else {
                    let (cmd_buf, _) = self.interface.draw(
                        cmd_buf,
                        ItfDrawTarget::Swapchain {
                            images: images.clone(),
                            image_num: image_num as _,
                        },
                    );

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
                        SwapchainPresentInfo::swapchain_image_index(swapchain.clone(), image_num),
                    )
                    .then_signal_fence_and_flush()
                    {
                        Ok(ok) => {
                            if dump_atlas_images {
                                ok.wait(None).unwrap();
                                self.atlas.dump();
                                None
                            } else {
                                Some(Box::new(ok))
                            }
                        },
                        Err(e) => {
                            match e {
                                vulkano::sync::FlushError::OutOfDate => continue 'resize,
                                _ => panic!("then_signal_fence_and_flush() {:?}", e),
                            }
                        },
                    };
                }

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
                self.fps
                    .store(calc_hertz(&gpu_times), atomic::Ordering::Relaxed);
                self.gpu_time
                    .store(calc_time(&gpu_times), atomic::Ordering::Relaxed);
                self.cpu_time
                    .store(calc_time(&cpu_times), atomic::Ordering::Relaxed);
            }
        }

        Ok(())
    }
}
