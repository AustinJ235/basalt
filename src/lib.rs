#![allow(clippy::significant_drop_in_scrutinee)]
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::module_inception)]
#![allow(clippy::doc_lazy_continuation)]

pub mod image;
pub mod input;
pub mod interface;
pub mod interval;
pub mod render;
pub mod window;

use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool};
use std::thread::available_parallelism;

mod vko {
    pub use vulkano::device::physical::{PhysicalDevice, PhysicalDeviceType};
    pub use vulkano::device::{
        Device, DeviceCreateInfo, DeviceExtensions, DeviceFeatures, Queue, QueueCreateInfo,
        QueueFlags,
    };
    pub use vulkano::instance::{
        Instance, InstanceCreateFlags, InstanceCreateInfo, InstanceExtensions, Version,
    };
    pub use vulkano::library::LoadingError;
    pub use vulkano::{Validated, VulkanError, VulkanLibrary};
    pub use vulkano_taskgraph::resource::Resources;
}

use crate::image::ImageCache;
use crate::input::Input;
use crate::interface::Interface;
use crate::interval::Interval;
use crate::render::{MSAA, VSync};
use crate::window::WindowManager;

/// Options for Basalt's creation and operation.
pub struct BasaltOptions {
    // Instance Options
    require_instance_extensions: vko::InstanceExtensions,
    prefer_instance_extensions: vko::InstanceExtensions,
    // Physical Device Selection
    portability_subset: bool,
    prefer_integrated_gpu: bool,
    // Device Options
    require_device_extensions: vko::DeviceExtensions,
    prefer_device_extensions: vko::DeviceExtensions,
    require_device_features: vko::DeviceFeatures,
    prefer_device_features: vko::DeviceFeatures,
    // Window Options
    winit_force_x11: bool,
    window_ignore_dpi: bool,
    window_default_scale: f32,
    // Render Options
    render_default_msaa: MSAA,
    render_default_vsync: VSync,
    render_default_consv_draw: bool,
    render_default_worker_threads: NonZeroUsize,
    // Interface Options
    binary_fonts: Vec<Arc<dyn AsRef<[u8]> + Sync + Send>>,
}

impl Default for BasaltOptions {
    fn default() -> Self {
        Self {
            require_instance_extensions: vko::InstanceExtensions {
                khr_surface: true,
                ..vko::InstanceExtensions::empty()
            },
            prefer_instance_extensions: vko::InstanceExtensions {
                khr_xlib_surface: true,
                khr_xcb_surface: true,
                khr_wayland_surface: true,
                khr_android_surface: true,
                khr_win32_surface: true,
                mvk_ios_surface: true,
                mvk_macos_surface: true,
                khr_get_physical_device_properties2: true,
                khr_get_surface_capabilities2: true,
                ext_surface_maintenance1: true,
                ext_swapchain_colorspace: true,
                ..vko::InstanceExtensions::empty()
            },
            portability_subset: false,
            prefer_integrated_gpu: true,
            require_device_extensions: vko::DeviceExtensions::empty(),
            prefer_device_extensions: vko::DeviceExtensions {
                ext_swapchain_maintenance1: true,
                ..vko::DeviceExtensions::empty()
            },
            require_device_features: vko::DeviceFeatures {
                descriptor_indexing: true,
                shader_sampled_image_array_non_uniform_indexing: true,
                runtime_descriptor_array: true,
                descriptor_binding_variable_descriptor_count: true,
                ..vko::DeviceFeatures::empty()
            },
            prefer_device_features: vko::DeviceFeatures::empty(),
            winit_force_x11: false,
            window_ignore_dpi: false,
            window_default_scale: 1.0,
            render_default_msaa: MSAA::X1,
            render_default_vsync: VSync::Enable,
            render_default_consv_draw: false,
            render_default_worker_threads: NonZeroUsize::new(
                (available_parallelism()
                    .unwrap_or(NonZeroUsize::new(4).unwrap())
                    .get() as f64
                    / 3.0)
                    .ceil() as usize,
            )
            .unwrap(),
            binary_fonts: Vec::new(),
        }
    }
}

impl BasaltOptions {
    /// Add required instance extensions
    ///
    /// ***Note:** This will cause an error if an extension is not supported. If this is not desired
    /// use the `prefer_instance_extensions` method instead.*
    pub fn require_instance_extensions(mut self, extensions: vko::InstanceExtensions) -> Self {
        self.require_instance_extensions |= extensions;
        self
    }

    /// Add preferred instance extensions
    pub fn prefer_instance_extensions(mut self, extensions: vko::InstanceExtensions) -> Self {
        self.prefer_instance_extensions |= extensions;
        self
    }

    /// Allow a portability subset device to be selected when enumerating `PhysicalDevice`'s.
    pub fn allow_portability_subset(mut self) -> Self {
        self.portability_subset = true;
        self
    }

    /// Prefer selecting integrated graphics over dedicated graphics on a hybrid system.
    pub fn prefer_integrated_gpu(mut self) -> Self {
        self.prefer_integrated_gpu = true;
        self
    }

    /// Prefer selecting dedicated graphics over integrated graphics on a hybrid system.
    pub fn prefer_dedicated_gpu(mut self) -> Self {
        self.prefer_integrated_gpu = false;
        self
    }

    /// Add required device extensions
    ///
    /// ***Note:** This will cause an error if an extension is not supported. If this is not desired
    /// use the `prefer_device_extensions` method instead.*
    pub fn require_device_extensions(mut self, extensions: vko::DeviceExtensions) -> Self {
        self.require_device_extensions |= extensions;
        self
    }

    /// Add preferred device extensions
    pub fn prefer_device_extensions(mut self, extensions: vko::DeviceExtensions) -> Self {
        self.prefer_device_extensions |= extensions;
        self
    }

    /// Add required device features
    ///
    /// ***Note:** This will cause an error if an feature is not supported. If this is not desired
    /// use the `prefer_device_features` method instead.*
    pub fn require_device_features(mut self, features: vko::DeviceFeatures) -> Self {
        self.require_device_features |= features;
        self
    }

    /// Add preferred device features
    pub fn prefer_device_features(mut self, features: vko::DeviceFeatures) -> Self {
        self.prefer_device_features |= features;
        self
    }

    /// On systems with wayland use of xwayland instead.
    pub fn winit_force_x11(mut self) -> Self {
        self.winit_force_x11 = true;
        self
    }

    /// Ignore dpi hints provided from windows disabling dpi scaling.
    ///
    /// **Default:** `false`
    pub fn window_ignore_dpi(mut self) -> Self {
        self.window_ignore_dpi = true;
        self
    }

    /// Set the default scale used for the interface when a window is created.
    ///
    /// **Default:** `1.0`
    ///
    /// ***Note:** `1.0` equals 100%*
    pub fn window_default_scale(mut self, scale: f32) -> Self {
        self.window_default_scale = scale;
        self
    }

    /// Set the default `MSAA` used for rendering the interface when a `Renderer` is created.
    ///
    /// **Default:** `MSAA::X1`
    pub fn render_default_msaa(mut self, msaa: MSAA) -> Self {
        self.render_default_msaa = msaa;
        self
    }

    /// Set the default `VSync` used for rendering when a `Renderer` is created.
    ///
    /// **Default:** `Vsync::Enable`
    pub fn render_default_vsync(mut self, vsync: VSync) -> Self {
        self.render_default_vsync = vsync;
        self
    }

    /// Set the default value used when creating a `Renderer` for conservative draw feature.
    ///
    /// **Default:** `false`
    ///
    /// ***Note:** For `Renderer`'s where a user renderer is provided, this is ignored. It is
    /// generally not ideal to use this in those cases.*
    pub fn render_default_consv_draw(mut self, enabled: bool) -> Self {
        self.render_default_consv_draw = enabled;
        self
    }

    /// Set the default count of worker threads used for a `Renderer`.
    ///
    /// **Default:** 1/3 of available threads (rounded up)
    pub fn render_default_worker_threads(mut self, threads: usize) -> Self {
        self.render_default_worker_threads = NonZeroUsize::new(threads.max(1)).unwrap();
        self
    }

    /// Add a font from a binary source that can be used by the interface.
    ///
    /// This is intended to be used with `include_bytes!(...)`.
    pub fn add_binary_font<B: AsRef<[u8]> + Sync + Send + 'static>(mut self, font: B) -> Self {
        self.binary_fonts.push(Arc::new(font));
        self
    }
}

/// Used for non-exhaustive structs to retain partial update compatibility.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NonExhaustive(pub(crate) ());

struct BasaltConfig {
    window_ignore_dpi: bool,
    window_default_scale: f32,
    render_default_msaa: MSAA,
    render_default_vsync: VSync,
    render_default_consv_draw: bool,
    render_default_worker_threads: NonZeroUsize,
}

/// The main object of this crate.
///
/// # Notes
/// - This is expected to be kept alive for the lifetime of the application.
/// - There should only ever be one instance of this struct.
pub struct Basalt {
    device: Arc<vko::Device>,
    device_resources: Arc<vko::Resources>,
    graphics_queue: Arc<vko::Queue>,
    transfer_queue: Arc<vko::Queue>,
    compute_queue: Arc<vko::Queue>,
    secondary_graphics_queue: Option<Arc<vko::Queue>>,
    secondary_transfer_queue: Option<Arc<vko::Queue>>,
    secondary_compute_queue: Option<Arc<vko::Queue>>,
    instance: Arc<vko::Instance>,
    interface: Arc<Interface>,
    input: Input,
    interval: Arc<Interval>,
    image_cache: Arc<ImageCache>,
    window_manager: Arc<WindowManager>,
    wants_exit: AtomicBool,
    config: BasaltConfig,
}

impl Basalt {
    /// Begin initializing Basalt, this thread will be taken for window event polling and the
    /// function provided in `result_fn` will be executed after Basalt initialization has
    /// completed or errored.
    pub fn initialize<F>(options: BasaltOptions, result_fn: F)
    where
        F: FnOnce(Result<Arc<Self>, InitializeError>) + Send + 'static,
    {
        let BasaltOptions {
            portability_subset,
            prefer_integrated_gpu,
            require_instance_extensions,
            prefer_instance_extensions,
            require_device_extensions,
            prefer_device_extensions,
            require_device_features,
            prefer_device_features,
            winit_force_x11,
            window_ignore_dpi,
            window_default_scale,
            render_default_msaa,
            render_default_vsync,
            render_default_consv_draw,
            render_default_worker_threads,
            binary_fonts,
        } = options;

        let vulkan_library = match vko::VulkanLibrary::new() {
            Ok(ok) => ok,
            Err(e) => return result_fn(Err(InitializeError::LoadVulkanLibrary(e))),
        };

        let instance_extensions = vulkan_library
            .supported_extensions()
            .intersection(&prefer_instance_extensions)
            .union(&require_instance_extensions);

        let mut instance_create_flags = vko::InstanceCreateFlags::empty();

        if portability_subset {
            instance_create_flags |= vko::InstanceCreateFlags::ENUMERATE_PORTABILITY;
        }

        let instance = match vko::Instance::new(
            vulkan_library,
            vko::InstanceCreateInfo {
                flags: instance_create_flags,
                enabled_extensions: instance_extensions,
                engine_name: Some(String::from("Basalt")),
                engine_version: vko::Version {
                    major: 0,
                    minor: 21,
                    patch: 0,
                },
                ..Default::default()
            },
        ) {
            Ok(ok) => ok,
            Err(e) => return result_fn(Err(InitializeError::CreateInstance(e))),
        };

        if instance.api_version() < vko::Version::V1_2 {
            return result_fn(Err(InitializeError::IncompatibleVulkan));
        }

        WindowManager::run(winit_force_x11, move |window_manager| {
            let mut physical_devices = match instance.enumerate_physical_devices() {
                Ok(ok) => ok.collect::<Vec<_>>(),
                Err(e) => {
                    return result_fn(Err(InitializeError::EnumerateDevices(e)));
                },
            };

            if prefer_integrated_gpu {
                physical_devices.sort_by_key(|dev| {
                    match dev.properties().device_type {
                        vko::PhysicalDeviceType::DiscreteGpu => 4,
                        vko::PhysicalDeviceType::IntegratedGpu => 5,
                        vko::PhysicalDeviceType::VirtualGpu => 3,
                        vko::PhysicalDeviceType::Other => 2,
                        vko::PhysicalDeviceType::Cpu => 1,
                        _ => 0,
                    }
                });
            } else {
                physical_devices.sort_by_key(|dev| {
                    match dev.properties().device_type {
                        vko::PhysicalDeviceType::DiscreteGpu => 5,
                        vko::PhysicalDeviceType::IntegratedGpu => 4,
                        vko::PhysicalDeviceType::VirtualGpu => 3,
                        vko::PhysicalDeviceType::Other => 2,
                        vko::PhysicalDeviceType::Cpu => 1,
                        _ => 0,
                    }
                });
            }

            physical_devices.retain(|physical_device| {
                if physical_device.api_version() < vko::Version::V1_2 {
                    println!(
                        "[Basalt]: Unable to use physical device, {}: api version < 1.2.",
                        physical_device.properties().device_name
                    );
                    return false;
                }

                if !physical_device
                    .supported_features()
                    .contains(&require_device_features)
                {
                    println!(
                        "[Basalt]: Unable to use physical device, {}: missing required device \
                         features.",
                        physical_device.properties().device_name
                    );
                    return false;
                }

                if !physical_device
                    .supported_extensions()
                    .contains(&require_device_extensions)
                {
                    println!(
                        "[Basalt]: Unable to use physical device, {}: missing required device \
                         extentsions.",
                        physical_device.properties().device_name
                    );
                    return false;
                }

                true
            });

            let physical_device = match physical_devices.pop() {
                Some(some) => some,
                None => return result_fn(Err(InitializeError::NoSuitableDevice)),
            };

            let mut available_queue_families: BTreeMap<u32, (vko::QueueFlags, u32)> =
                BTreeMap::new();
            let mut graphics_queue_families: Vec<u32> = Vec::new();
            let mut compute_queue_families: Vec<u32> = Vec::new();
            let mut transfer_queue_families: Vec<u32> = Vec::new();

            for (i, properties) in physical_device.queue_family_properties().iter().enumerate() {
                if properties.queue_flags.contains(vko::QueueFlags::GRAPHICS) {
                    graphics_queue_families.push(i as u32);
                }

                if properties.queue_flags.contains(vko::QueueFlags::COMPUTE) {
                    compute_queue_families.push(i as u32);
                }

                if properties.queue_flags.contains(vko::QueueFlags::TRANSFER) {
                    transfer_queue_families.push(i as u32);
                }

                available_queue_families
                    .insert(i as u32, (properties.queue_flags, properties.queue_count));
            }

            graphics_queue_families.sort_by_cached_key(|index| {
                let flags = available_queue_families.get(index).unwrap().0;
                let mut weight: u8 = 0;
                weight += flags.contains(vko::QueueFlags::COMPUTE) as u8;
                weight += flags.contains(vko::QueueFlags::PROTECTED) as u8;
                weight += flags.contains(vko::QueueFlags::VIDEO_DECODE) as u8;
                weight += flags.contains(vko::QueueFlags::VIDEO_ENCODE) as u8;
                weight += flags.contains(vko::QueueFlags::OPTICAL_FLOW) as u8;
                weight
            });

            compute_queue_families.sort_by_cached_key(|index| {
                let flags = available_queue_families.get(index).unwrap().0;
                let mut weight: u8 = 0;
                weight += flags.contains(vko::QueueFlags::GRAPHICS) as u8;
                weight += flags.contains(vko::QueueFlags::PROTECTED) as u8;
                weight += flags.contains(vko::QueueFlags::VIDEO_DECODE) as u8;
                weight += flags.contains(vko::QueueFlags::VIDEO_ENCODE) as u8;
                weight += flags.contains(vko::QueueFlags::OPTICAL_FLOW) as u8;
                weight
            });

            transfer_queue_families.sort_by_cached_key(|index| {
                let flags = available_queue_families.get(index).unwrap().0;
                let mut weight: u8 = 0;
                weight += flags.contains(vko::QueueFlags::GRAPHICS) as u8;
                weight += flags.contains(vko::QueueFlags::COMPUTE) as u8;
                weight += flags.contains(vko::QueueFlags::PROTECTED) as u8;
                weight += flags.contains(vko::QueueFlags::VIDEO_DECODE) as u8;
                weight += flags.contains(vko::QueueFlags::VIDEO_ENCODE) as u8;
                weight += flags.contains(vko::QueueFlags::OPTICAL_FLOW) as u8;
                weight
            });

            let select_queue =
                |indexes: &Vec<u32>, queue_families: &mut BTreeMap<u32, (vko::QueueFlags, u32)>| {
                    let mut selected_index = None;

                    for index in indexes.iter() {
                        let count = &mut queue_families.get_mut(index).unwrap().1;

                        if *count > 0 {
                            *count -= 1;
                            selected_index = Some(*index);
                            break;
                        }
                    }

                    selected_index
                };

            let g_primary = select_queue(&graphics_queue_families, &mut available_queue_families);
            let c_primary = select_queue(&compute_queue_families, &mut available_queue_families);
            let t_primary = select_queue(&transfer_queue_families, &mut available_queue_families);
            let g_secondary = select_queue(&graphics_queue_families, &mut available_queue_families);
            let c_secondary = select_queue(&compute_queue_families, &mut available_queue_families);
            let t_secondary = select_queue(&transfer_queue_families, &mut available_queue_families);

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
                if let Some(family_index) = family_op {
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

            let queue_request: Vec<vko::QueueCreateInfo> = family_map
                .into_iter()
                .map(|(family_index, members)| {
                    let mut priorites = Vec::with_capacity(members.len());

                    for (binding, priority) in members.into_iter() {
                        queue_map.push((binding, queue_count));
                        queue_count += 1;
                        priorites.push(priority);
                    }

                    vko::QueueCreateInfo {
                        queues: priorites,
                        queue_family_index: family_index,
                        ..Default::default()
                    }
                })
                .collect();

            let device_extensions = physical_device
                .supported_extensions()
                .intersection(&prefer_device_extensions)
                .union(&require_device_extensions);

            let device_features = physical_device
                .supported_features()
                .intersection(&prefer_device_features)
                .union(&require_device_features);

            let (device, queues) = match vko::Device::new(
                physical_device,
                vko::DeviceCreateInfo {
                    enabled_extensions: device_extensions,
                    enabled_features: device_features,
                    queue_create_infos: queue_request,
                    ..Default::default()
                },
            ) {
                Ok(ok) => ok,
                Err(e) => return result_fn(Err(InitializeError::CreateDevice(e))),
            };

            assert_eq!(queues.len(), queue_map.len());

            let mut queues: Vec<Option<Arc<vko::Queue>>> = queues.into_iter().map(Some).collect();
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

            let device_resources = vko::Resources::new(&device, &Default::default());
            let interface = Interface::new(binary_fonts.clone());
            let interval = Arc::new(Interval::new());
            let input = Input::new(interface.clone(), interval.clone());

            let basalt = Arc::new(Basalt {
                device,
                device_resources,
                graphics_queue,
                transfer_queue,
                compute_queue,
                secondary_graphics_queue,
                secondary_transfer_queue,
                secondary_compute_queue,
                instance: instance.clone(),
                interface,
                input,
                interval,
                image_cache: Arc::new(ImageCache::new()),
                window_manager,
                wants_exit: AtomicBool::new(false),
                config: BasaltConfig {
                    window_ignore_dpi,
                    window_default_scale,
                    render_default_msaa,
                    render_default_vsync,
                    render_default_consv_draw,
                    render_default_worker_threads,
                },
            });

            basalt.interface.associate_basalt(basalt.clone());
            basalt.window_manager.associate_basalt(basalt.clone());
            result_fn(Ok(basalt));
        });
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

    /// Obtain a copy of `Arc<Instance>`
    pub fn instance(&self) -> Arc<vko::Instance> {
        self.instance.clone()
    }

    /// Obtain a reference of `Arc<Instance>`
    pub fn instance_ref(&self) -> &Arc<vko::Instance> {
        &self.instance
    }

    /// Obtain a copy of `Arc<PhysicalDevice>`
    pub fn physical_device(&self) -> Arc<vko::PhysicalDevice> {
        self.device.physical_device().clone()
    }

    /// Obtain a reference of `Arc<PhysicalDevice>`
    pub fn physical_device_ref(&self) -> &Arc<vko::PhysicalDevice> {
        self.device.physical_device()
    }

    /// Obtain a copy of `Arc<Devcie>`
    pub fn device(&self) -> Arc<vko::Device> {
        self.device.clone()
    }

    /// Obtain a refernce of `Arc<Device>`
    pub fn device_ref(&self) -> &Arc<vko::Device> {
        &self.device
    }

    /// Obtain a copy of `Arc<Resources>`.
    pub fn device_resources(&self) -> Arc<vko::Resources> {
        self.device_resources.clone()
    }

    /// Obtain a reference of `Arc<Resources>`.
    pub fn device_resources_ref(&self) -> &Arc<vko::Resources> {
        &self.device_resources
    }

    /// Obtain a copy of the `Arc<Queue>` assigned for graphics operations.
    pub fn graphics_queue(&self) -> Arc<vko::Queue> {
        self.graphics_queue.clone()
    }

    /// Obtain a reference of the `Arc<Queue>` assigned for graphics operations.
    pub fn graphics_queue_ref(&self) -> &Arc<vko::Queue> {
        &self.graphics_queue
    }

    /// Obtain a copy of the `Arc<Queue>` assigned for secondary graphics operations.
    pub fn secondary_graphics_queue(&self) -> Option<Arc<vko::Queue>> {
        self.secondary_graphics_queue.clone()
    }

    /// Obtain a reference of the `Arc<Queue>` assigned for secondary graphics operations.
    pub fn secondary_graphics_queue_ref(&self) -> Option<&Arc<vko::Queue>> {
        self.secondary_graphics_queue.as_ref()
    }

    /// Obtain a copy of the `Arc<Queue>` assigned for compute operations.
    ///
    /// # Notes:
    /// - This queue may be the same as the graphics queue in cases where the device only
    /// has a single queue present.
    pub fn compute_queue(&self) -> Arc<vko::Queue> {
        self.compute_queue.clone()
    }

    /// Obtain a reference of the `Arc<Queue>` assigned for compute operations.
    ///
    /// # Notes:
    /// - This queue may be the same as the graphics queue in cases where the device only
    /// has a single queue present.
    pub fn compute_queue_ref(&self) -> &Arc<vko::Queue> {
        &self.compute_queue
    }

    /// Obtain a copy of the `Arc<Queue>` assigned for secondary compute operations.
    pub fn secondary_compute_queue(&self) -> Option<Arc<vko::Queue>> {
        self.secondary_compute_queue.clone()
    }

    /// Obtain a reference of the `Arc<Queue>` assigned for secondary compute operations.
    pub fn secondary_compute_queue_ref(&self) -> Option<&Arc<vko::Queue>> {
        self.secondary_compute_queue.as_ref()
    }

    /// Obtain a copy of the `Arc<Queue>` assigned for transfers.
    ///
    /// # Notes:
    /// - This queue may be the same as the compute queue in cases where the device only
    /// has two queues present. In cases where there is only one queue the graphics, compute,
    /// and transfer queues will all be the same queue.
    pub fn transfer_queue(&self) -> Arc<vko::Queue> {
        self.transfer_queue.clone()
    }

    /// Obtain a reference of the `Arc<Queue>` assigned for transfers.
    ///
    /// # Notes:
    /// - This queue may be the same as the compute queue in cases where the device only
    /// has two queues present. In cases where there is only one queue the graphics, compute,
    /// and transfer queues will all be the same queue.
    pub fn transfer_queue_ref(&self) -> &Arc<vko::Queue> {
        &self.transfer_queue
    }

    /// Obtain a copy of the `Arc<Queue>` assigned for secondary transfers.
    pub fn secondary_transfer_queue(&self) -> Option<Arc<vko::Queue>> {
        self.secondary_transfer_queue.clone()
    }

    /// Obtain a reference of the `Arc<Queue>` assigned for secondary transfers.
    pub fn secondary_transfer_queue_ref(&self) -> Option<&Arc<vko::Queue>> {
        self.secondary_transfer_queue.as_ref()
    }

    /// Signal the application to exit.
    pub fn exit(&self) {
        self.wants_exit.store(true, atomic::Ordering::Relaxed);
        self.window_manager.exit();
    }

    /// Check if basalt is attempting to exit.
    pub fn wants_exit(&self) -> bool {
        self.wants_exit.load(atomic::Ordering::Relaxed)
    }
}

impl std::fmt::Debug for Basalt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Basalt").finish_non_exhaustive()
    }
}

/// An error occurred during the `Basalt`'s initialization.
#[derive(Debug)]
pub enum InitializeError {
    /// Failed to load the `VulkanLibrary`.
    LoadVulkanLibrary(vko::LoadingError),
    /// Failed to create the `Instance`.
    CreateInstance(vko::Validated<vko::VulkanError>),
    /// The `Instance`'s vulkan version is less than  1.2.
    IncompatibleVulkan,
    /// Failed to enumerate the physical devices.
    EnumerateDevices(vko::VulkanError),
    /// There are no suitable devices.
    NoSuitableDevice,
    /// Failed to create the `Device`
    CreateDevice(vko::Validated<vko::VulkanError>),
}

impl Display for InitializeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            Self::LoadVulkanLibrary(e) => {
                write!(f, "Failed to load the vulkan library: {}", e)
            },
            Self::CreateInstance(e) => {
                write!(f, "Failed to create the instance: {}", e)
            },
            Self::IncompatibleVulkan => {
                f.write_str("The vulkan instance's version is less than 1.2.")
            },
            Self::EnumerateDevices(e) => {
                write!(f, "Failed to enumerate the physical devices: {}", e)
            },
            Self::NoSuitableDevice => f.write_str("No suitable device was found."),
            Self::CreateDevice(e) => {
                write!(f, "Failed to create the device: {}", e)
            },
        }
    }
}

fn ulps_eq(a: f32, b: f32, tol: u32) -> bool {
    if a.is_nan() || b.is_nan() {
        false
    } else if a.is_sign_positive() != b.is_sign_positive() {
        a == b
    } else {
        let a_bits = a.to_bits();
        let b_bits = b.to_bits();
        let max = a_bits.max(b_bits);
        let min = a_bits.min(b_bits);
        (max - min) <= tol
    }
}
