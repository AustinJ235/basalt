pub(in crate::window) mod convert;

use std::sync::{Arc, Weak};
use std::thread::spawn;

use foldhash::{HashMap, HashMapExt};
use parking_lot::Mutex;
use raw_window_handle::{
    DisplayHandle, HandleError as RwhHandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

use crate::Basalt;
use crate::input::{InputEvent, MouseButton};
use crate::window::backend::{BackendHandle, BackendWindowHandle, PendingRes};
use crate::window::builder::WindowAttributes;
use crate::window::{
    CreateWindowError, CursorIcon, FullScreenBehavior, Monitor, Window, WindowBackend, WindowError,
    WindowEvent, WindowID, WindowType,
};

mod wnt {
    pub use winit::application::ApplicationHandler;
    pub use winit::dpi::PhysicalSize;
    pub use winit::event::{
        DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent,
    };
    pub use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
    pub use winit::window::{CursorGrabMode, Window, WindowAttributes, WindowId};
}

mod vko {
    pub use vulkano::swapchain::Win32Monitor;
}

pub struct WntBackendHandle {
    event_proxy: wnt::EventLoopProxy<AppEvent>,
}

impl WntBackendHandle {
    pub fn run<F>(_winit_force_x11: bool, exec: F)
    where
        F: FnOnce(Self) + Send + 'static,
    {
        let mut event_loop_builder = wnt::EventLoop::<AppEvent>::with_user_event();

        #[cfg(target_family = "unix")]
        {
            use winit::platform::x11::EventLoopBuilderExtX11;

            if _winit_force_x11 {
                event_loop_builder.with_x11();
            }
        }

        let event_loop = event_loop_builder.build().unwrap();
        let event_proxy_ret = event_loop.create_proxy();
        let event_proxy_app = event_loop.create_proxy();

        spawn(move || {
            exec(Self {
                event_proxy: event_proxy_ret,
            });
        });

        event_loop
            .run_app(&mut AppState::new(event_proxy_app))
            .unwrap();
    }
}

impl BackendHandle for WntBackendHandle {
    fn window_backend(&self) -> WindowBackend {
        WindowBackend::Winit
    }

    fn associate_basalt(&self, basalt: Arc<Basalt>) {
        let _ = self.event_proxy.send_event(AppEvent::AssociateBasalt {
            basalt,
        });
    }

    fn create_window(
        &self,
        window_id: WindowID,
        window_attributes: WindowAttributes,
    ) -> Result<Arc<Window>, WindowError> {
        let pending_res = PendingRes::empty();

        let window_attributes = match window_attributes {
            WindowAttributes::Winit(attrs) => attrs,
            _ => unreachable!(),
        };

        self.event_proxy
            .send_event(AppEvent::CreateWindow {
                window_id,
                window_attributes,
                pending_res: pending_res.clone(),
            })
            .map_err(|_| WindowError::BackendExited)?;

        pending_res.wait()
    }

    fn get_monitors(&self) -> Result<Vec<Monitor>, WindowError> {
        let pending_res = PendingRes::empty();

        self.event_proxy
            .send_event(AppEvent::GetMonitors {
                pending_res: pending_res.clone(),
            })
            .map_err(|_| WindowError::BackendExited)?;

        Ok(pending_res.wait())
    }

    fn get_primary_monitor(&self) -> Result<Monitor, WindowError> {
        let pending_res = PendingRes::empty();

        self.event_proxy
            .send_event(AppEvent::GetPrimaryMonitor {
                pending_res: pending_res.clone(),
            })
            .map_err(|_| WindowError::BackendExited)?;

        pending_res.wait().ok_or(WindowError::NotSupported)
    }

    fn exit(&self) {
        let _ = self.event_proxy.send_event(AppEvent::Exit);
    }
}

struct WntWindowHandle {
    basalt: Arc<Basalt>,
    id: WindowID,
    ty: WindowType,
    inner: wnt::Window,
    proxy: wnt::EventLoopProxy<AppEvent>,
    cached_attributes: Mutex<CachedAttributes>,
}

struct CachedAttributes {
    title: String,
    min_size_op: Option<[u32; 2]>,
    max_size_op: Option<[u32; 2]>,
    cursor_icon: CursorIcon,
    cursor_visible: bool,
    cursor_locked: bool,
    cursor_confined: bool,
}

impl CachedAttributes {
    fn from_attributes(attributes: &wnt::WindowAttributes) -> Self {
        Self {
            title: attributes.title.clone(),
            min_size_op: attributes.min_inner_size.map(|wnt_size| {
                // Note: It is assumed this field is set with PhysicalSize
                let wnt_phy_size = wnt_size.to_physical::<u32>(1.0);
                [wnt_phy_size.width, wnt_phy_size.height]
            }),
            max_size_op: attributes.max_inner_size.map(|wnt_size| {
                // Note: It is assumed this field is set with PhysicalSize
                let wnt_phy_size = wnt_size.to_physical::<u32>(1.0);
                [wnt_phy_size.width, wnt_phy_size.height]
            }),
            cursor_icon: Default::default(),
            cursor_visible: true,
            cursor_locked: false,
            cursor_confined: false,
        }
    }

    fn cursor_captured(&self) -> bool {
        !self.cursor_visible && (self.cursor_locked || self.cursor_confined)
    }
}

impl WntWindowHandle {
    fn get_monitors(&self) -> Vec<Monitor> {
        let current_op = self.inner.current_monitor();
        let primary_op = self.inner.primary_monitor();

        self.inner
            .available_monitors()
            .filter_map(|winit_monitor| {
                let is_current = match current_op.as_ref() {
                    Some(current) => *current == winit_monitor,
                    None => false,
                };

                let is_primary = match primary_op.as_ref() {
                    Some(primary) => *primary == winit_monitor,
                    None => false,
                };

                let mut monitor = convert::monitor_from_wnt_handle(winit_monitor)?;
                monitor.is_current = is_current;
                monitor.is_primary = is_primary;
                Some(monitor)
            })
            .collect()
    }

    fn get_primary_monitor(&self) -> Option<Monitor> {
        self.inner.primary_monitor().and_then(|winit_monitor| {
            let is_current = match self.inner.current_monitor() {
                Some(current) => current == winit_monitor,
                None => false,
            };

            let mut monitor = convert::monitor_from_wnt_handle(winit_monitor)?;
            monitor.is_primary = true;
            monitor.is_current = is_current;
            Some(monitor)
        })
    }
}

impl HasWindowHandle for WntWindowHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, RwhHandleError> {
        self.inner.window_handle()
    }
}

impl HasDisplayHandle for WntWindowHandle {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, RwhHandleError> {
        self.inner.display_handle()
    }
}

impl BackendWindowHandle for WntWindowHandle {
    fn backend(&self) -> WindowBackend {
        WindowBackend::Winit
    }

    fn win32_monitor(&self) -> Result<vko::Win32Monitor, WindowError> {
        #[cfg(target_os = "windows")]
        unsafe {
            use wnt::platform::windows::MonitorHandleExtWindows;

            self.inner
                .current_monitor()
                .map(|m| vko::Win32Monitor::new(m.hmonitor()))
                .ok_or(WindowError::NotSupported)
        }

        #[cfg(not(target_os = "windows"))]
        {
            Err(WindowError::NotSupported)
        }
    }

    fn title(&self) -> Result<String, WindowError> {
        match self.ty {
            WindowType::Ios | WindowType::Android => Err(WindowError::NotSupported),
            WindowType::X11 | WindowType::Wayland => {
                Ok(self.cached_attributes.lock().title.clone())
            },
            _ => Ok(self.inner.title()),
        }
    }

    fn set_title(&self, title: String) -> Result<(), WindowError> {
        match self.ty {
            WindowType::Ios | WindowType::Android => Err(WindowError::NotSupported),
            _ => {
                self.inner.set_title(&title);
                self.cached_attributes.lock().title = title;
                Ok(())
            },
        }
    }

    fn maximized(&self) -> Result<bool, WindowError> {
        match self.ty {
            WindowType::Ios | WindowType::Android => Err(WindowError::NotSupported),
            _ => Ok(self.inner.is_maximized()),
        }
    }

    fn set_maximized(&self, maximized: bool) -> Result<(), WindowError> {
        match self.ty {
            WindowType::Ios | WindowType::Android => Err(WindowError::NotSupported),
            _ => {
                self.inner.set_maximized(maximized);
                Ok(())
            },
        }
    }

    fn minimized(&self) -> Result<bool, WindowError> {
        match self.ty {
            WindowType::Ios | WindowType::Android => Err(WindowError::NotSupported),
            WindowType::Wayland => {
                // TODO: winit doesn't support this?
                Err(WindowError::NotImplemented)
            },
            _ => {
                match self.inner.is_minimized() {
                    Some(minimized) => Ok(minimized),
                    None => Err(WindowError::NotSupported),
                }
            },
        }
    }

    fn set_minimized(&self, minimized: bool) -> Result<(), WindowError> {
        match self.ty {
            WindowType::Ios | WindowType::Android => Err(WindowError::NotSupported),
            WindowType::Wayland => {
                if !minimized {
                    Err(WindowError::NotSupported)
                } else {
                    self.inner.set_minimized(true);
                    Ok(())
                }
            },
            _ => {
                self.inner.set_minimized(true);
                Ok(())
            },
        }
    }

    fn size(&self) -> Result<[u32; 2], WindowError> {
        Ok(self.inner.inner_size().into())
    }

    fn set_size(&self, size: [u32; 2]) -> Result<(), WindowError> {
        let request_size = wnt::PhysicalSize::from(size);
        let pre_request_size = self.inner.inner_size();

        match self.inner.request_inner_size(request_size) {
            Some(physical_size) => {
                if physical_size == pre_request_size {
                    // Platform doesn't support resize.
                    return Err(WindowError::NotSupported);
                }

                if physical_size == request_size {
                    // If the size is the same as the one that was requested, then the platform
                    // resized the window immediately. In this case, the resize event may not get
                    // sent out per winit docs.

                    self.proxy
                        .send_event(AppEvent::SendWindowEvent {
                            winit_win_id: self.inner.id(),
                            window_event: WindowEvent::Resized {
                                width: size[0],
                                height: size[1],
                            },
                        })
                        .map_err(|_| WindowError::BackendExited)?;
                }

                Ok(())
            },
            None => {
                // The resize request was sent and a subsequent resize event will be sent.
                Ok(())
            },
        }
    }

    fn min_size(&self) -> Result<Option<[u32; 2]>, WindowError> {
        match self.ty {
            WindowType::Ios | WindowType::Android => Err(WindowError::NotSupported),
            _ => Ok(self.cached_attributes.lock().min_size_op),
        }
    }

    fn set_min_size(&self, min_size_op: Option<[u32; 2]>) -> Result<(), WindowError> {
        match self.ty {
            WindowType::Ios | WindowType::Android => Err(WindowError::NotSupported),
            _ => {
                let wnt_phy_size_op = min_size_op.map(|[w, h]| wnt::PhysicalSize::new(w, h));
                self.inner.set_min_inner_size(wnt_phy_size_op);
                self.cached_attributes.lock().min_size_op = min_size_op;
                Ok(())
            },
        }
    }

    fn max_size(&self) -> Result<Option<[u32; 2]>, WindowError> {
        match self.ty {
            WindowType::Ios | WindowType::Android => Err(WindowError::NotSupported),
            _ => Ok(self.cached_attributes.lock().max_size_op),
        }
    }

    fn set_max_size(&self, max_size_op: Option<[u32; 2]>) -> Result<(), WindowError> {
        match self.ty {
            WindowType::Ios | WindowType::Android => Err(WindowError::NotSupported),
            _ => {
                let wnt_phy_size_op = max_size_op.map(|[w, h]| wnt::PhysicalSize::new(w, h));
                self.inner.set_max_inner_size(wnt_phy_size_op);
                self.cached_attributes.lock().max_size_op = max_size_op;
                Ok(())
            },
        }
    }

    fn cursor_icon(&self) -> Result<CursorIcon, WindowError> {
        match self.ty {
            WindowType::Ios | WindowType::Android => Err(WindowError::NotSupported),
            _ => Ok(self.cached_attributes.lock().cursor_icon),
        }
    }

    fn set_cursor_icon(&self, cursor_icon: CursorIcon) -> Result<(), WindowError> {
        match self.ty {
            WindowType::Ios | WindowType::Android => Err(WindowError::NotSupported),
            _ => {
                self.inner
                    .set_cursor(convert::cursor_icon_to_wnt(cursor_icon)?);
                self.cached_attributes.lock().cursor_icon = cursor_icon;
                Ok(())
            },
        }
    }

    fn cursor_visible(&self) -> Result<bool, WindowError> {
        match self.ty {
            WindowType::Ios | WindowType::Android => Err(WindowError::NotSupported),
            _ => Ok(self.cached_attributes.lock().cursor_visible),
        }
    }

    fn set_cursor_visible(&self, visible: bool) -> Result<(), WindowError> {
        match self.ty {
            WindowType::Ios | WindowType::Android => Err(WindowError::NotSupported),
            _ => {
                self.inner.set_cursor_visible(visible);
                let mut cached_attributes = self.cached_attributes.lock();
                let was_captured = cached_attributes.cursor_captured();
                cached_attributes.cursor_visible = visible;
                let is_captured = cached_attributes.cursor_captured();

                if was_captured != is_captured {
                    self.basalt
                        .input_ref()
                        .send_event(InputEvent::CursorCapture {
                            win: self.id,
                            captured: is_captured,
                        });
                }

                Ok(())
            },
        }
    }

    fn cursor_locked(&self) -> Result<bool, WindowError> {
        match self.ty {
            WindowType::Ios | WindowType::Android => Err(WindowError::NotSupported),
            WindowType::X11 => {
                // TODO: as per winit docs @ version 0.30, this isn't implemented.
                Err(WindowError::NotImplemented)
            },
            _ => Ok(self.cached_attributes.lock().cursor_locked),
        }
    }

    fn set_cursor_locked(&self, locked: bool) -> Result<(), WindowError> {
        match self.ty {
            WindowType::Ios | WindowType::Android => Err(WindowError::NotSupported),
            WindowType::X11 => {
                // TODO: as per winit docs @ version 0.30, this isn't implemented.
                Err(WindowError::NotImplemented)
            },
            _ => {
                let mut cached_attributes = self.cached_attributes.lock();

                if cached_attributes.cursor_locked == locked {
                    return Ok(());
                }

                let was_captured = cached_attributes.cursor_captured();

                if locked {
                    let _ = self.inner.set_cursor_grab(wnt::CursorGrabMode::Locked);
                    cached_attributes.cursor_confined = false;
                } else {
                    let _ = self.inner.set_cursor_grab(wnt::CursorGrabMode::None);
                }

                cached_attributes.cursor_locked = locked;
                let is_captured = cached_attributes.cursor_captured();

                if was_captured != is_captured {
                    self.basalt
                        .input_ref()
                        .send_event(InputEvent::CursorCapture {
                            win: self.id,
                            captured: is_captured,
                        });
                }

                Ok(())
            },
        }
    }

    fn cursor_confined(&self) -> Result<bool, WindowError> {
        match self.ty {
            WindowType::Ios | WindowType::Android => Err(WindowError::NotSupported),
            WindowType::Macos => {
                // TODO: as per winit docs @ version 0.30, this isn't implemented.
                Err(WindowError::NotImplemented)
            },
            _ => Ok(self.cached_attributes.lock().cursor_confined),
        }
    }

    fn set_cursor_confined(&self, confined: bool) -> Result<(), WindowError> {
        match self.ty {
            WindowType::Ios | WindowType::Android => Err(WindowError::NotSupported),
            WindowType::Macos => {
                // TODO: as per winit docs @ version 0.30, this isn't implemented.
                Err(WindowError::NotImplemented)
            },
            _ => {
                let mut cached_attributes = self.cached_attributes.lock();

                if cached_attributes.cursor_confined == confined {
                    return Ok(());
                }

                let was_captured = cached_attributes.cursor_captured();

                if confined {
                    let _ = self.inner.set_cursor_grab(wnt::CursorGrabMode::Confined);
                    cached_attributes.cursor_locked = false;
                } else {
                    let _ = self.inner.set_cursor_grab(wnt::CursorGrabMode::None);
                }

                cached_attributes.cursor_confined = confined;
                let is_captured = cached_attributes.cursor_captured();

                if was_captured != is_captured {
                    self.basalt
                        .input_ref()
                        .send_event(InputEvent::CursorCapture {
                            win: self.id,
                            captured: is_captured,
                        });
                }

                Ok(())
            },
        }
    }

    fn cursor_captured(&self) -> Result<bool, WindowError> {
        Ok(self.cached_attributes.lock().cursor_captured())
    }

    fn set_cursor_captured(&self, captured: bool) -> Result<(), WindowError> {
        if matches!(self.ty, WindowType::Ios | WindowType::Android) {
            return Err(WindowError::NotSupported);
        }

        let mut cached_attributes = self.cached_attributes.lock();
        let was_captured = cached_attributes.cursor_captured();

        if captured {
            if cached_attributes.cursor_captured() {
                return Ok(());
            }

            if cached_attributes.cursor_visible {
                self.inner.set_cursor_visible(false);
                cached_attributes.cursor_visible = false;
            }

            if !cached_attributes.cursor_locked && !cached_attributes.cursor_confined {
                let wnt_grab_mode = match self.ty {
                    WindowType::Ios | WindowType::Android => unreachable!(), // Checked above
                    WindowType::Macos => {
                        cached_attributes.cursor_locked = true;
                        wnt::CursorGrabMode::Locked
                    },
                    WindowType::X11 => {
                        cached_attributes.cursor_confined = true;
                        wnt::CursorGrabMode::Confined
                    },
                    _ => {
                        cached_attributes.cursor_locked = true;
                        wnt::CursorGrabMode::Locked
                    },
                };

                let _ = self.inner.set_cursor_grab(wnt_grab_mode);
            }
        } else {
            if !cached_attributes.cursor_visible {
                self.inner.set_cursor_visible(true);
                cached_attributes.cursor_visible = true;
            }

            if cached_attributes.cursor_locked || cached_attributes.cursor_confined {
                let _ = self.inner.set_cursor_grab(wnt::CursorGrabMode::None);
                cached_attributes.cursor_locked = false;
                cached_attributes.cursor_confined = false;
            }
        }

        let is_captured = cached_attributes.cursor_captured();

        if was_captured != is_captured {
            self.basalt
                .input_ref()
                .send_event(InputEvent::CursorCapture {
                    win: self.id,
                    captured: is_captured,
                });
        }

        Ok(())
    }

    fn monitor(&self) -> Result<Monitor, WindowError> {
        let wnt_cur_mon = self
            .inner
            .current_monitor()
            .ok_or(WindowError::NotSupported)?;

        let is_primary = match self.inner.primary_monitor() {
            Some(wnt_prm_mon) => wnt_prm_mon == wnt_cur_mon,
            None => false,
        };

        let mut cur_mon = convert::monitor_from_wnt_handle(wnt_cur_mon).ok_or(
            WindowError::Other(String::from("failed to translate monitor.")),
        )?;

        cur_mon.is_current = true;
        cur_mon.is_primary = is_primary;
        Ok(cur_mon)
    }

    fn full_screen(&self) -> Result<bool, WindowError> {
        Ok(self.inner.fullscreen().is_some())
    }

    fn enable_full_screen(
        &self,
        borderless_fallback: bool,
        full_screen_behavior: FullScreenBehavior,
    ) -> Result<(), WindowError> {
        let winit_fullscreen = convert::fsb_to_wnt(
            full_screen_behavior,
            borderless_fallback,
            self.basalt
                .device_ref()
                .enabled_extensions()
                .ext_full_screen_exclusive,
            self.monitor().ok(),
            self.get_primary_monitor(),
            self.get_monitors(),
        )?;

        self.inner.set_fullscreen(Some(winit_fullscreen));

        self.proxy
            .send_event(AppEvent::SendWindowEvent {
                winit_win_id: self.inner.id(),
                window_event: WindowEvent::EnabledFullscreen,
            })
            .map_err(|_| WindowError::BackendExited)?;

        Ok(())
    }

    fn disable_full_screen(&self) -> Result<(), WindowError> {
        if self.inner.fullscreen().is_some() {
            self.inner.set_fullscreen(None);

            self.proxy
                .send_event(AppEvent::SendWindowEvent {
                    winit_win_id: self.inner.id(),
                    window_event: WindowEvent::DisabledFullscreen,
                })
                .map_err(|_| WindowError::BackendExited)?;
        }

        Ok(())
    }
}

impl Drop for WntWindowHandle {
    fn drop(&mut self) {
        let _ = self.proxy.send_event(AppEvent::CloseWindow {
            window_id: self.id,
        });
    }
}

#[derive(Debug)]
enum AppEvent {
    AssociateBasalt {
        basalt: Arc<Basalt>,
    },
    CreateWindow {
        window_id: WindowID,
        window_attributes: Box<wnt::WindowAttributes>,
        pending_res: PendingRes<Result<Arc<Window>, WindowError>>,
    },
    CloseWindow {
        window_id: WindowID,
    },
    GetPrimaryMonitor {
        pending_res: PendingRes<Option<Monitor>>,
    },
    GetMonitors {
        pending_res: PendingRes<Vec<Monitor>>,
    },
    SendWindowEvent {
        winit_win_id: wnt::WindowId,
        window_event: WindowEvent,
    },
    Exit,
}

struct AppState {
    proxy: wnt::EventLoopProxy<AppEvent>,
    basalt_op: Option<Arc<Basalt>>,
    windows_wk: HashMap<wnt::WindowId, Weak<Window>>,
    bst_to_winit_id: HashMap<WindowID, wnt::WindowId>,
}

impl AppState {
    fn new(proxy: wnt::EventLoopProxy<AppEvent>) -> Self {
        Self {
            proxy,
            basalt_op: None,
            windows_wk: HashMap::new(),
            bst_to_winit_id: HashMap::new(),
        }
    }
}

impl wnt::ApplicationHandler<AppEvent> for AppState {
    fn resumed(&mut self, _: &wnt::ActiveEventLoop) {
        // Required, but basalt only supports Desktop platforms.
    }

    fn user_event(&mut self, ael: &wnt::ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::AssociateBasalt {
                basalt,
            } => {
                self.basalt_op = Some(basalt);
            },
            AppEvent::CreateWindow {
                window_id,
                window_attributes,
                pending_res,
            } => {
                let basalt = self.basalt_op.as_ref().unwrap();
                let cached_attributes = CachedAttributes::from_attributes(&window_attributes);

                let inner = match ael.create_window(*window_attributes) {
                    Ok(ok) => ok,
                    Err(e) => {
                        pending_res.set(Err(CreateWindowError::Os(format!("{}", e)).into()));
                        return;
                    },
                };

                let window_ty = match WindowType::from_window_handle(&inner) {
                    Ok(ok) => ok,
                    Err(e) => {
                        pending_res.set(Err(e));
                        return;
                    },
                };

                let winit_window = WntWindowHandle {
                    basalt: basalt.clone(),
                    id: window_id,
                    ty: window_ty,
                    inner,
                    proxy: self.proxy.clone(),
                    cached_attributes: Mutex::new(cached_attributes),
                };

                let scale_factor = winit_window.inner.scale_factor() as f32;
                let winit_win_id = winit_window.inner.id();

                let window = match Window::new(basalt.clone(), window_id, winit_window) {
                    Ok(ok) => ok,
                    Err(e) => {
                        pending_res.set(Err(e));
                        return;
                    },
                };

                window.set_dpi_scale(scale_factor);
                self.bst_to_winit_id.insert(window_id, winit_win_id);
                self.windows_wk
                    .insert(winit_win_id, Arc::downgrade(&window));
                basalt.window_manager_ref().window_created(window.clone());
                pending_res.set(Ok(window));
            },
            AppEvent::CloseWindow {
                window_id,
            } => {
                if let Some(winit_win_id) = self.bst_to_winit_id.remove(&window_id) {
                    self.windows_wk.remove(&winit_win_id);
                }
            },
            AppEvent::GetPrimaryMonitor {
                pending_res,
            } => {
                let monitor_op = ael.primary_monitor().and_then(|winit_monitor| {
                    let mut monitor = convert::monitor_from_wnt_handle(winit_monitor)?;
                    monitor.is_primary = true;
                    Some(monitor)
                });

                pending_res.set(monitor_op);
            },
            AppEvent::GetMonitors {
                pending_res,
            } => {
                let primary_op = ael.primary_monitor();

                let monitors = ael
                    .available_monitors()
                    .filter_map(|winit_monitor| {
                        let is_primary = match primary_op.as_ref() {
                            Some(primary) => *primary == winit_monitor,
                            None => false,
                        };

                        let mut monitor = convert::monitor_from_wnt_handle(winit_monitor)?;
                        monitor.is_primary = is_primary;
                        Some(monitor)
                    })
                    .collect::<Vec<_>>();

                pending_res.set(monitors);
            },
            AppEvent::SendWindowEvent {
                winit_win_id,
                window_event,
            } => {
                if let Some(window_wk) = self.windows_wk.get(&winit_win_id)
                    && let Some(window) = window_wk.upgrade()
                {
                    window.send_event(window_event);
                }
            },
            AppEvent::Exit => {
                ael.exit();
            },
        }
    }

    fn window_event(
        &mut self,
        _: &wnt::ActiveEventLoop,
        winit_win_id: wnt::WindowId,
        event: wnt::WindowEvent,
    ) {
        let basalt = match self.basalt_op.as_ref() {
            Some(some) => some,
            None => return,
        };

        let window_wk = match self.windows_wk.get(&winit_win_id) {
            Some(some) => some,
            None => return,
        };

        let window = match window_wk.upgrade() {
            Some(some) => some,
            None => return,
        };

        match event {
            wnt::WindowEvent::Resized(physical_size) => {
                window.send_event(WindowEvent::Resized {
                    width: physical_size.width,
                    height: physical_size.height,
                });
            },
            wnt::WindowEvent::CloseRequested => {
                window.close_requested();
            },
            wnt::WindowEvent::Destroyed => {
                window.close();
            },
            wnt::WindowEvent::Focused(focused) => {
                basalt.input_ref().send_event(match focused {
                    true => {
                        InputEvent::Focus {
                            win: window.id(),
                        }
                    },
                    false => {
                        InputEvent::FocusLost {
                            win: window.id(),
                        }
                    },
                });
            },
            wnt::WindowEvent::KeyboardInput {
                event, ..
            } => {
                match event.state {
                    wnt::ElementState::Pressed => {
                        if let Some(qwerty) = convert::key_ev_to_qwerty(&event) {
                            basalt.input_ref().send_event(InputEvent::Press {
                                win: window.id(),
                                key: qwerty.into(),
                            });
                        }

                        if let Some(text) = event.text {
                            for c in text.as_str().chars() {
                                basalt.input_ref().send_event(InputEvent::Character {
                                    win: window.id(),
                                    c,
                                });
                            }
                        }
                    },
                    wnt::ElementState::Released => {
                        if let Some(qwerty) = convert::key_ev_to_qwerty(&event) {
                            basalt.input_ref().send_event(InputEvent::Release {
                                win: window.id(),
                                key: qwerty.into(),
                            });
                        }
                    },
                }
            },
            wnt::WindowEvent::CursorMoved {
                position, ..
            } => {
                basalt.input_ref().send_event(InputEvent::Cursor {
                    win: window.id(),
                    x: position.x as f32,
                    y: position.y as f32,
                });
            },
            wnt::WindowEvent::CursorEntered {
                ..
            } => {
                basalt.input_ref().send_event(InputEvent::Enter {
                    win: window.id(),
                });
            },
            wnt::WindowEvent::CursorLeft {
                ..
            } => {
                basalt.input_ref().send_event(InputEvent::Leave {
                    win: window.id(),
                });
            },
            wnt::WindowEvent::MouseWheel {
                delta, ..
            } => {
                let [v, h] = match delta {
                    wnt::MouseScrollDelta::LineDelta(x, y) => [-y, x],
                    wnt::MouseScrollDelta::PixelDelta(position) => {
                        [-position.y as f32, position.x as f32]
                    },
                };

                basalt.input_ref().send_event(InputEvent::Scroll {
                    win: window.id(),
                    v: v.clamp(-1.0, 1.0),
                    h: h.clamp(-1.0, 1.0),
                });
            },
            wnt::WindowEvent::MouseInput {
                state,
                button,
                ..
            } => {
                let button = match button {
                    wnt::MouseButton::Left => MouseButton::Left,
                    wnt::MouseButton::Right => MouseButton::Right,
                    wnt::MouseButton::Middle => MouseButton::Middle,
                    _ => return,
                };

                basalt.input_ref().send_event(match state {
                    wnt::ElementState::Pressed => {
                        InputEvent::Press {
                            win: window.id(),
                            key: button.into(),
                        }
                    },
                    wnt::ElementState::Released => {
                        InputEvent::Release {
                            win: window.id(),
                            key: button.into(),
                        }
                    },
                });
            },
            wnt::WindowEvent::ScaleFactorChanged {
                scale_factor,
                mut inner_size_writer,
            } => {
                if window.ignoring_dpi() {
                    let _ = inner_size_writer.request_inner_size(window.size().unwrap().into());
                }

                window.set_dpi_scale(scale_factor as f32);
            },
            wnt::WindowEvent::RedrawRequested => {
                window.send_event(WindowEvent::RedrawRequested);
            },
            _ => (),
        }
    }

    fn device_event(
        &mut self,
        _: &wnt::ActiveEventLoop,
        _: wnt::DeviceId,
        event: wnt::DeviceEvent,
    ) {
        let basalt = match self.basalt_op.as_ref() {
            Some(some) => some,
            None => return,
        };

        if let wnt::DeviceEvent::Motion {
            axis,
            value,
        } = event
        {
            basalt.input_ref().send_event(match axis {
                0 => {
                    InputEvent::Motion {
                        x: -value as f32,
                        y: 0.0,
                    }
                },
                1 => {
                    InputEvent::Motion {
                        x: 0.0,
                        y: -value as f32,
                    }
                },
                _ => return,
            });
        }
    }
}
