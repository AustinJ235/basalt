use crate::Basalt;
use crossbeam::queue::SegQueue;
use std::sync::Arc;
use surface::{
    BackendRequest,
    BstSurface,
    BstSurfaceBuilder,
    DefaultSurfaceBackend,
    SurfaceBackend,
};
use vulkano::{
    device::{self, Device, DeviceExtensions},
    instance::{Instance, InstanceExtensions, PhysicalDevice, PhysicalDeviceType},
    swapchain::Surface,
};

struct DeviceSel {
    device: Arc<Device>,
    graphics_queue: Arc<device::Queue>,
    compute_queue: Arc<device::Queue>,
    transfer_queue: Arc<device::Queue>,
}

/// In order to create basalt you must first start with `BasaltSetup`. This handles the creation
/// of some of its dependencies along with the inital window creation. Some things however need
/// to be created on the main thread and stay there. Therefore this struct requires that it be
/// created on the main thread and that it stays there. `Basalt` however will be free to be used
/// on any thread as it implements Send and Sync.
///
/// # Example Usage
/// ```no_run
/// // Create the setup struct
/// let setup = BasaltSetup::new();
///
/// // The instance needs to be created to proceed further. You can use
/// // `setup.instance_extensions_mut()` to specify any additional instance extensions you may
/// // want for you application. Removal of extensions may hinder Basalt's ability to function.
/// setup.create_instance().unwrap();
///
/// // Now that the instance has been created. A device needs to be selected. This process is
/// // automatic for the time being. Either use `setup.automatic_device()` for the default
/// // preference of devices or `setup.prefer_integrated_device()` if the integrated graphics
/// // of the host is prefered. As with the instance you can modify the device extensions with
/// // `setup.device_extensions_mut()` if you want additional extensions loaded. Removal of
/// // extensions may also hinder Basalt's ability to function.
/// setup.automatic_device().unwrap();
///
/// // A Surface backend needs to be created, so everything is ready for surfaces/windows to be
/// // created and handled.
/// setup.default_surface_backend().unwrap();
///
/// // At least one surface needs to be added for Basalt to function correctly.
/// setup.add_surface_simple("Basalt", 1024, 576).unwrap();
///
/// // All is set setup. You may now begin running.
/// setup.complete(Box::new(move |basalt| {
///     // Do some basalty things.
/// }));
/// ```

pub(crate) struct BstInitials {
    pub instance: Arc<Instance>,
    pub device: Arc<Device>,
    pub graphics_queue: Arc<device::Queue>,
    pub compute_queue: Arc<device::Queue>,
    pub transfer_queue: Arc<device::Queue>,
    pub backend_req_queue: Arc<SegQueue<BackendRequest>>,
    pub surfaces: Vec<Arc<Surface<Arc<dyn BstSurface + Send + Sync>>>>,
}

pub struct BasaltSetup {
    instance_extensions: InstanceExtensions,
    device_extensions: DeviceExtensions,
    instance: Option<Arc<Instance>>,
    device_sel: Option<DeviceSel>,
    surface_backend: Option<Box<dyn SurfaceBackend>>,
    backend_req_queue: Arc<SegQueue<BackendRequest>>,
    surfaces: Vec<Arc<Surface<Arc<dyn BstSurface + Send + Sync>>>>,
}

impl !Send for BasaltSetup {}
impl !Sync for BasaltSetup {}

impl BasaltSetup {
    pub fn new() -> Self {
        BasaltSetup {
            instance_extensions: vulkano_win::required_extensions(),
            device_extensions: DeviceExtensions {
                khr_swapchain: true,
                ext_full_screen_exclusive: true,
                ..DeviceExtensions::none()
            },
            instance: None,
            device_sel: None,
            surface_backend: None,
            surfaces: Vec::new(),
            backend_req_queue: Arc::new(SegQueue::new()),
        }
    }

    /// Returns an mutable reference to `InstanceExtensions`
    ///
    /// # Panics
    ///
    /// Panics if `create_instance` has executed sucessfully.
    pub fn instance_extensions_mut(&mut self) -> &mut InstanceExtensions {
        if self.instance.is_some() {
            panic!("Instance extensions can not be modified after instance creation.");
        }

        &mut self.instance_extensions
    }

    /// Returns an mutable reference to `DeviceExtensions`
    ///
    /// # Panics
    ///
    /// Panics if `automatic_device` or `prefer_integrated_device` has executed sucessfully.
    pub fn device_extensions_mut(&mut self) -> &mut DeviceExtensions {
        if self.device_sel.is_some() {
            panic!("Device extensions can not be modified after device selection.");
        }

        &mut self.device_extensions
    }

    /// Creates the instance.
    ///
    /// # Panics
    ///
    /// Panics if `create_instance` has executed sucessfully.
    pub fn create_instance(&mut self) -> Result<(), String> {
        if self.instance.is_some() {
            panic!("Instance is already created!");
        }

        self.instance = Some(
            Instance::new(None, &self.instance_extensions, None)
                .map_err(|e| format!("{}", e))?,
        );
        Ok(())
    }

    /// Obtain the device and queues. In order of preference: Discrete > Integrated >
    /// VirtualGpu > Other > Cpu
    ///
    /// # Panics
    ///
    /// Panics if `create_instance` hasn't executed sucessfully.
    pub fn automatic_device(&mut self) -> Result<(), String> {
        if self.instance.is_none() {
            panic!("Instance has not been created!");
        }

        let physical_devices: Vec<_> =
            PhysicalDevice::enumerate(self.instance.as_ref().unwrap()).collect();

        self.device_sel = Some(Self::create_device(
            physical_devices
                .iter()
                .find(|d| d.ty() == PhysicalDeviceType::DiscreteGpu)
                .unwrap_or(
                    physical_devices
                        .iter()
                        .find(|d| d.ty() == PhysicalDeviceType::IntegratedGpu)
                        .unwrap_or(
                            physical_devices
                                .iter()
                                .find(|d| d.ty() == PhysicalDeviceType::VirtualGpu)
                                .unwrap_or(
                                    physical_devices
                                        .iter()
                                        .find(|d| d.ty() == PhysicalDeviceType::Other)
                                        .unwrap_or(
                                            physical_devices
                                                .iter()
                                                .find(|d| d.ty() == PhysicalDeviceType::Cpu)
                                                .ok_or(format!("No suitable device found."))?,
                                        ),
                                ),
                        ),
                ),
            &self.device_extensions,
        )?);

        Ok(())
    }

    /// Obtain the device and queues. In order of preference: Integrated > Discrete >
    /// VirtualGpu > Other > Cpu
    ///
    /// # Panics
    ///
    /// Panics if `create_instance` hasn't executed sucessfully.
    pub fn prefer_integrated_device(&mut self) -> Result<(), String> {
        if self.instance.is_none() {
            panic!("Instance has not been created!");
        }

        let physical_devices: Vec<_> =
            PhysicalDevice::enumerate(self.instance.as_ref().unwrap()).collect();

        self.device_sel = Some(Self::create_device(
            physical_devices
                .iter()
                .find(|d| d.ty() == PhysicalDeviceType::IntegratedGpu)
                .unwrap_or(
                    physical_devices
                        .iter()
                        .find(|d| d.ty() == PhysicalDeviceType::DiscreteGpu)
                        .unwrap_or(
                            physical_devices
                                .iter()
                                .find(|d| d.ty() == PhysicalDeviceType::VirtualGpu)
                                .unwrap_or(
                                    physical_devices
                                        .iter()
                                        .find(|d| d.ty() == PhysicalDeviceType::Other)
                                        .unwrap_or(
                                            physical_devices
                                                .iter()
                                                .find(|d| d.ty() == PhysicalDeviceType::Cpu)
                                                .ok_or(format!("No suitable device found."))?,
                                        ),
                                ),
                        ),
                ),
            &self.device_extensions,
        )?);

        Ok(())
    }

    fn create_device(
        physical_device: &PhysicalDevice,
        device_extensions: &DeviceExtensions,
    ) -> Result<DeviceSel, String> {
        let mut families: Vec<_> = physical_device.queue_families().collect();

        // Find a graphics family. This always needs to exist as Basalt is after all a UI lib.
        let graphics_family = {
            let (family_i, family) = families
                .iter()
                .cloned()
                .enumerate()
                .find(|(_, f)| f.supports_graphics())
                .ok_or(format!("No graphics family available."))?;

            families.swap_remove(family_i);
            family
        };

        // Try to find a compute family. Try to find a separate family otherwise if the graphics
        // family also supports compute and can have multiple queues use the graphics family for
        // compute also.
        let compute_family_op = {
            match families.iter().cloned().enumerate().find(|(_, f)| f.supports_compute()) {
                Some((family_i, family)) => {
                    families.swap_remove(family_i);
                    Some(family)
                },
                None => {
                    if graphics_family.queues_count() >= 2 {
                        Some(graphics_family)
                    } else {
                        None
                    }
                },
            }
        };

        // Try to find a transfer family. Check if there is any families that only support
        // transfers as those may have special relations with the gpu for better performance.
        // If there is none of those see if the compute family has multiple queues. If the
        // compute family doesn't have multiple queues then check if the graphics queue has
        // three or more queues available.
        let transfer_family_op = {
            match families.iter().cloned().find(|f| {
                f.explicitly_supports_transfers()
                    && !f.supports_graphics()
                    && !f.supports_compute()
            }) {
                Some(some) => Some(some),
                None => {
                    match families.iter().cloned().find(|f| f.explicitly_supports_transfers()) {
                        Some(some) => Some(some),
                        None => {
                            match compute_family_op.as_ref() {
                                Some(compute_family) => {
                                    if *compute_family == graphics_family {
                                        if graphics_family.queues_count() >= 3 {
                                            Some(graphics_family)
                                        } else {
                                            None
                                        }
                                    } else {
                                        if compute_family.queues_count() >= 2 {
                                            Some(*compute_family)
                                        } else {
                                            None
                                        }
                                    }
                                },
                                None => None,
                            }
                        },
                    }
                },
            }
        };

        let compute_family_requested = compute_family_op.is_some();
        let transfer_family_requested = transfer_family_op.is_some();
        let mut queue_request = vec![(graphics_family, 1.0)];

        if let Some(family) = compute_family_op {
            queue_request.push((family, 0.2));
        }

        if let Some(family) = transfer_family_op {
            queue_request.push((family, 0.2));
        }

        let (device, mut queues) = Device::new(
            *physical_device,
            physical_device.supported_features(),
            device_extensions,
            queue_request.into_iter(),
        )
        .map_err(|e| format!("Failed to create device: {}", e))?;

        let graphics_queue =
            queues.next().ok_or(format!("Expected graphics queue to be present."))?;

        let compute_queue = match compute_family_requested {
            true => queues.next().ok_or(format!("Expected compute queue to be present."))?,
            false => graphics_queue.clone(),
        };

        let transfer_queue = match transfer_family_requested {
            true => queues.next().ok_or(format!("Expected transfer queue to be present."))?,
            false => compute_queue.clone(),
        };

        Ok(DeviceSel {
            device,
            graphics_queue,
            compute_queue,
            transfer_queue,
        })
    }

    /// Add a surface/window with just a title and size.
    ///
    /// # Panics
    ///
    /// Panics if a surface backend hasn't been selected. See `default_surface_backend`.
    pub fn create_surface_simple<T: Into<String>>(
        &mut self,
        title: T,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        self.surfaces.push(self.surface_backend.as_mut().unwrap().create_surface(
            BstSurfaceBuilder::new().with_size(width, height).with_title(title),
        )?);

        Ok(())
    }

    /// Setup the default surface backend.
    ///
    /// # Panics
    ///
    /// - Instance hasn't not been created.
    /// - Surface backend is already present.
    pub fn default_surface_backend(&mut self) -> Result<(), String> {
        if self.instance.is_none() {
            panic!("Instance hasn't been created!");
        }

        if self.surface_backend.is_some() {
            panic!("Surface backend is already present!");
        }

        self.surface_backend = Some(DefaultSurfaceBackend::new(
            self.instance.clone().unwrap(),
            self.backend_req_queue.clone(),
        ));

        Ok(())
    }

    /// Complete the setup and start running the event loops. This takes control of the main
    /// thread. Pass a closure which will be ran after setup is complete that'll have access to
    /// the newly created Basalt instance. Generally you start your initial creation of bins and
    /// run off the bin hooks and input hooks. This thread will not be expected to return.
    pub fn complete(self, hook: Box<dyn FnMut(Arc<Basalt>) + Send>) {
        if self.instance.is_none()
            || self.device_sel.is_none()
            || self.surface_backend.is_none()
            || self.surfaces.is_empty()
        {
            panic!(
                "Setup is incomplete! Instance: {}, Device: {}, Surface: {}",
                self.instance.is_some(),
                self.device_sel.is_some(),
                !self.surfaces.is_empty()
            );
        }

        let DeviceSel {
            device,
            graphics_queue,
            transfer_queue,
            compute_queue,
        } = self.device_sel.unwrap();

        let initials = BstInitials {
            instance: self.instance.unwrap(),
            device,
            graphics_queue,
            transfer_queue,
            compute_queue,
            backend_req_queue: self.backend_req_queue,
            surfaces: self.surfaces,
        };

        let basalt: Arc<Basalt> = unimplemented!();

        self.surface_backend.take().unwrap().run(basalt);
    }
}
