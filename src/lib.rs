#![feature(arbitrary_self_types)]
#![feature(integer_atomics)]

extern crate winit;
#[macro_use]
pub extern crate vulkano;
extern crate vulkano_win;
#[macro_use]
extern crate vulkano_shaders;
extern crate arc_swap;
extern crate crossbeam;
extern crate ilmenite;
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
use parking_lot::{Mutex, RwLock};
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
    instance::{Instance, PhysicalDevice},
    swapchain::{self, ColorSpace, Surface, Swapchain, SwapchainCreationError},
    sync::GpuFuture,
};
use window::BasaltWindow;

const SHOW_SWAPCHAIN_WARNINGS: bool = true;

#[derive(Debug)]
pub struct Limits {
    pub max_image_dimension_2d: u32,
    pub max_image_dimension_3d: u32,
}

struct Initials {
    device: Arc<Device>,
    graphics_queue: Arc<device::Queue>,
    transfer_queue: Arc<device::Queue>,
    surface: Arc<Surface<Arc<dyn BasaltWindow + Send + Sync>>>,
    swap_caps: swapchain::Capabilities,
    limits: Arc<Limits>,
    pdevi: usize,
    window_size: [u32; 2],
    bin_stats: bool,
}

impl Initials {
    pub fn use_first_device(options: Options) -> Result<Self, String> {
        let mut device_num = 0;
        let mut show_devices = false;
        let mut bin_stats = false;

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
                            println!(
                                "Incorrect '--use-device' usage. Example: '--use-device=2'"
                            );
                            continue;
                        },
                    };

                    println!("Using device: {}", device_num);
                }
            } else if arg.starts_with("--show-devices") {
                show_devices = true;
            } else if arg.starts_with("--binstats") {
                bin_stats = true;
            }
        }

        let device_ext = DeviceExtensions {
            khr_swapchain: true,
            ext_full_screen_exclusive: true,
            ..DeviceExtensions::none()
        };

        let extensions = vulkano_win::required_extensions();

        let instance = match Instance::new(None, &extensions, None) {
            Ok(ok) => ok,
            Err(e) => return Err(format!("Failed to create instance: {}", e)),
        };

        let surface = match window::open_surface(options.clone(), instance.clone()) {
            Ok(ok) => ok,
            Err(e) => return Err(e),
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
            None => {
                if device_num == 0 {
                    return Err(format!("No physical devices available."));
                } else {
                    return Err(format!("Phyiscal device not found."));
                }
            },
        };

        let physical = physical_devs.remove(device_num);
        let mut queue_family_opts = Vec::new();

        for family in physical.queue_families() {
            for _ in 0..family.queues_count() {
                queue_family_opts.push(family);
            }
        }

        let mut graphics_queue_ = None;
        let mut transfer_queue_ = None;

        for i in 0..queue_family_opts.len() {
            if queue_family_opts[i].supports_graphics()
                && surface.is_supported(queue_family_opts[i]).unwrap_or(false)
            {
                graphics_queue_ = Some((queue_family_opts[i], 0.8));
                queue_family_opts.remove(i);
                break;
            }
        }
        if graphics_queue_.is_none() {
            return Err(format!("Couldn't find a suitable queue for graphics."));
        }

        for i in 0..queue_family_opts.len() {
            transfer_queue_ = Some((queue_family_opts[i], 0.2));
            queue_family_opts.remove(i);
            break;
        }
        if transfer_queue_.is_none() {
            println!(
                "Couldn't find a suitable queue for transfers.\nUsing graphics queue for \
                 transfers also."
            );
        }

        let mut req_queues = Vec::new();
        req_queues.push(graphics_queue_.unwrap());

        if let Some(transfer_queue) = transfer_queue_ {
            req_queues.push(transfer_queue);
        }

        let (device, mut queues) =
            match Device::new(physical, physical.supported_features(), &device_ext, req_queues)
            {
                Ok(ok) => ok,
                Err(e) => return Err(format!("Failed to create device: {}", e)),
            };
        let graphics_queue = match queues.next() {
            Some(some) => some,
            None => return Err(format!("Device didn't have any queues")),
        };
        let transfer_queue = match queues.next() {
            Some(some) => some,
            None => graphics_queue.clone(),
        };
        let swap_caps = match surface.capabilities(physical) {
            Ok(ok) => ok,
            Err(e) => return Err(format!("Failed to get surface capabilities: {}", e)),
        };

        let phy_limits = physical.limits();

        let limits = Arc::new(Limits {
            max_image_dimension_2d: phy_limits.max_image_dimension_2d(),
            max_image_dimension_3d: phy_limits.max_image_dimension_3d(),
        });

        Ok(Initials {
            device,
            graphics_queue,
            transfer_queue,
            surface,
            swap_caps,
            limits,
            pdevi: device_num,
            window_size: options.window_size,
            bin_stats,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Options {
    ignore_dpi: bool,
    window_size: [u32; 2],
    title: String,
    scale: f32,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            ignore_dpi: false,
            window_size: [1920, 1080],
            title: "vk-basalt".to_string(),
            scale: 1.0,
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SwapchainRecreateReason {
    Scale,
    Resize(u32, u32),
    Redraw,
    Properties,
    External,
    Exclusive(bool),
}

#[allow(dead_code)]
pub struct Basalt {
    device: Arc<Device>,
    graphics_queue: Arc<device::Queue>,
    transfer_queue: Arc<device::Queue>,
    surface: Arc<Surface<Arc<dyn BasaltWindow + Send + Sync>>>,
    swap_caps: swapchain::Capabilities,
    do_every: RwLock<Vec<Arc<dyn Fn() + Send + Sync>>>,
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
}

#[allow(dead_code)]
impl Basalt {
    pub fn new(options: Options) -> Result<Arc<Self>, String> {
        unsafe {
            let initials = match Initials::use_first_device(options.clone()) {
                Ok(ok) => ok,
                Err(e) => return Err(e),
            };

            let mut basalt_ret = Arc::new(Basalt {
                device: initials.device,
                graphics_queue: initials.graphics_queue,
                transfer_queue: initials.transfer_queue,
                surface: initials.surface,
                swap_caps: initials.swap_caps,
                do_every: RwLock::new(Vec::new()),
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
                custom_scale: Mutex::new(options.scale),
                options,
                ignore_dpi_data: Mutex::new(None),
                bin_stats: initials.bin_stats,
                swapchain_recreate: Mutex::new(Vec::new()),
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
                    output.push_str(" F2: Prints fps while held\r\n");
                    output.push_str(" F3: Prints bin update stats\r\n");
                    output.push_str(" F7: Decreases msaa level\r\n");
                    output.push_str(" F8: Increases msaa level\r\n");
                    output.push_str(" F10: Toggles vsync\r\n");
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

            Ok(basalt_ret)
        }
    }

    pub(crate) fn recreate_swapchain(&self, reason: SwapchainRecreateReason) {
        self.swapchain_recreate.lock().push(reason);
    }

    pub fn force_recreate_swapchain(&self) {
        self.swapchain_recreate.lock().push(SwapchainRecreateReason::External);
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

    pub fn transfer_queue(&self) -> Arc<device::Queue> {
        self.transfer_queue.clone()
    }

    pub fn transfer_queue_ref(&self) -> &Arc<device::Queue> {
        &self.transfer_queue
    }

    pub fn graphics_queue(&self) -> Arc<device::Queue> {
        self.graphics_queue.clone()
    }

    pub fn graphics_queue_ref(&self) -> &Arc<device::Queue> {
        &self.graphics_queue
    }

    pub fn physical_device_index(&self) -> usize {
        self.pdevi
    }

    pub fn surface(&self) -> Arc<Surface<Arc<dyn BasaltWindow + Send + Sync>>> {
        self.surface.clone()
    }

    pub fn surface_ref(&self) -> &Arc<Surface<Arc<dyn BasaltWindow + Send + Sync>>> {
        &self.surface
    }

    pub fn swap_caps(&self) -> &swapchain::Capabilities {
        &self.swap_caps
    }

    pub fn wants_exit(&self) -> bool {
        self.wants_exit.load(atomic::Ordering::Relaxed)
    }

    pub fn window(&self) -> Arc<dyn BasaltWindow + Send + Sync> {
        self.surface().window().clone()
    }

    /// This will only work if the basalt is handling the loop thread. This
    /// is done via the method ``spawn_app_loop()``
    pub fn wait_for_exit(&self) -> Result<(), String> {
        match self.loop_thread.lock().take() {
            Some(handle) => {
                match handle.join() {
                    Ok(ok) => ok,
                    Err(_) => Err(format!("Failed to join loop thread.")),
                }
            },
            None => Ok(()),
        }
    }

    pub fn spawn_app_loop(self: &Arc<Self>) {
        let basalt = self.clone();

        *self.loop_thread.lock() = Some(thread::spawn(move || basalt.app_loop()));
    }

    /// only works with app loop
    pub fn resize(&self, w: u32, h: u32) {
        self.surface.window().request_resize(w, h);
    }

    /// only works with app loop
    pub fn enable_fullscreen(&self) {
        self.surface.window().enable_fullscreen();
    }

    /// only works with app loop
    pub fn disable_fullscreen(&self) {
        self.surface.window().disable_fullscreen();
    }

    /// only works with app loop
    pub fn toggle_fullscreen(&self) {
        self.surface.window().toggle_fullscreen();
    }

    /// only works with app loop
    pub fn exit(&self) {
        self.wants_exit.store(true, atomic::Ordering::Relaxed);
    }

    /// only works with app loop
    pub fn do_every(&self, func: Arc<dyn Fn() + Send + Sync>) {
        self.do_every.write().push(func);
    }

    /// only works with app loop
    pub fn fps(&self) -> usize {
        self.fps.load(atomic::Ordering::Relaxed)
    }

    pub fn app_loop(self: &Arc<Self>) -> Result<(), String> {
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
            None => {
                return Err(format!(
                    "Failed to find capatible format for swapchain. Avaible formats: {:?}",
                    self.swap_caps.supported_formats
                ))
            },
        };

        let mut itf_renderer = interface::render::ItfRenderer::new(self.clone());
        let mut previous_frame_future: Option<Box<dyn GpuFuture>> = None;
        let mut fullscreen_exclusive = false;

        'resize: loop {
            self.swapchain_recreate.lock().clear();
            let [x, y] = self
                .surface
                .capabilities(
                    PhysicalDevice::from_index(self.surface.instance(), self.pdevi).unwrap(),
                )
                .unwrap()
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

            dbg!(&fullscreen_exclusive);

            swapchain_ = match match swapchain_
                .as_ref()
                .map(|v: &(Arc<Swapchain<_>>, _)| v.0.clone())
            {
                Some(old_swapchain) => {
                    Swapchain::with_old_swapchain(
                        self.device.clone(),
                        self.surface.clone(),
                        self.swap_caps.min_image_count,
                        swapchain_format,
                        [x, y],
                        1,
                        self.swap_caps.supported_usage_flags,
                        &self.graphics_queue,
                        swapchain::SurfaceTransform::Identity,
                        swapchain::CompositeAlpha::Opaque,
                        present_mode,
                        fullscreen_exclusive,
                        true,
                        ColorSpace::SrgbNonLinear,
                        old_swapchain,
                    )
                },
                None => {
                    Swapchain::new(
                        self.device.clone(),
                        self.surface.clone(),
                        self.swap_caps.min_image_count,
                        swapchain_format,
                        [x, y],
                        1,
                        self.swap_caps.supported_usage_flags,
                        &self.graphics_queue,
                        swapchain::SurfaceTransform::Identity,
                        swapchain::CompositeAlpha::Opaque,
                        present_mode,
                        fullscreen_exclusive,
                        true,
                        ColorSpace::SrgbNonLinear,
                    )
                },
            } {
                Ok(ok) => Some(ok),
                Err(e) => {
                    match e {
                        SwapchainCreationError::UnsupportedDimensions => continue,
                        e => return Err(format!("Basalt failed to recreate swapchain: {}", e)),
                    }
                },
            };

            let (swapchain, images) =
                (&swapchain_.as_ref().unwrap().0, &swapchain_.as_ref().unwrap().1);
            let mut fps_avg = VecDeque::new();

            loop {
                previous_frame_future.as_mut().map(|future| future.cleanup_finished());

                for reason in self.swapchain_recreate.lock().split_off(0) {
                    match reason {
                        SwapchainRecreateReason::Scale => {
                            itf_resize = true;
                        },
                        SwapchainRecreateReason::Resize(w, h) => {
                            if w != win_size_x || h != win_size_y {
                                itf_resize = true;
                                continue 'resize;
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

                            if w != win_size_x || h != win_size_y {
                                itf_resize = true;
                                continue 'resize;
                            }
                        },
                        SwapchainRecreateReason::Properties
                        | SwapchainRecreateReason::External => {
                            itf_resize = true;
                            continue 'resize;
                        },
                        SwapchainRecreateReason::Exclusive(ex) => {
                            fullscreen_exclusive = ex;
                            dbg!(ex);
                            continue 'resize;
                        },
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

                for func in &*self.do_every.read() {
                    func()
                }

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
                    Err(e) => {
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
                        }
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
