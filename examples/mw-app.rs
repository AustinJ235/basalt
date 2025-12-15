use std::sync::Arc;
use std::sync::atomic::{self, AtomicUsize};
use std::thread;

use basalt::input::Qwerty;
use basalt::interface::UnitValue::Pixels;
use basalt::interface::{BinStyle, Color, TextAttrs, TextBody};
use basalt::render::{Renderer, RendererError};
use basalt::window::Window;
use basalt::{Basalt, BasaltOptions};

fn main() {
    Basalt::initialize(BasaltOptions::default(), move |basalt_res| {
        let basalt = basalt_res.unwrap();
        let windows_opened = Arc::new(AtomicUsize::new(0));
        let thrd_windows_opened = windows_opened.clone();

        basalt.window_manager_ref().on_open(move |_window| {
            thrd_windows_opened.fetch_add(1, atomic::Ordering::SeqCst);
        });

        let thrd_basalt = basalt.clone();
        let thrd_windows_opened = windows_opened.clone();
        let main_thread = thread::current();

        basalt.window_manager_ref().on_close(move |_window_id| {
            if thrd_windows_opened.fetch_sub(1, atomic::Ordering::SeqCst) == 1 {
                main_thread.unpark();
                thrd_basalt.exit();
            }
        });

        open_window(&basalt);

        while windows_opened.load(atomic::Ordering::SeqCst) > 0 {
            thread::park();
        }

        basalt.exit();
    });
}

fn open_window(basalt: &Arc<Basalt>) {
    let window = basalt
        .window_manager_ref()
        .create()
        .title("mw-app")
        .size([400; 2])
        .build()
        .unwrap();

    window.on_press(Qwerty::F8, move |target, _, _| {
        let window = target.into_window().unwrap();
        println!("VSync: {:?}", window.toggle_renderer_vsync());
        Default::default()
    });

    window.on_press(Qwerty::F9, move |target, _, _| {
        let window = target.into_window().unwrap();
        println!("MSAA: {:?}", window.decr_renderer_msaa());
        Default::default()
    });

    window.on_press(Qwerty::F10, move |target, _, _| {
        let window = target.into_window().unwrap();
        println!("MSAA: {:?}", window.incr_renderer_msaa());
        Default::default()
    });

    window.on_press(Qwerty::F11, move |target, _, _| {
        let window = target.into_window().unwrap();
        println!("Fullscreen: {:?}", window.toggle_fullscreen());
        Default::default()
    });

    window.on_press(Qwerty::O, move |target, _, _| {
        open_window(target.into_window().unwrap().basalt_ref());
        Default::default()
    });

    add_bins_to_window(window.clone());

    thread::spawn(move || {
        let mut renderer = Renderer::new(window).unwrap();
        renderer.interface_only();

        match renderer.run() {
            Ok(_) | Err(RendererError::Closed) => (),
            Err(e) => {
                println!("{:?}", e);
            },
        }
    });
}

fn add_bins_to_window(window: Arc<Window>) {
    let background = window.new_bin();

    background
        .style_update(BinStyle {
            pos_from_t: Pixels(0.0),
            pos_from_b: Pixels(0.0),
            pos_from_l: Pixels(0.0),
            pos_from_r: Pixels(0.0),
            back_color: Color::shex("f0f0f0"),
            ..Default::default()
        })
        .expect_valid();

    let window_id_disp = window.new_bin();
    background.add_child(window_id_disp.clone());

    window_id_disp
        .style_update(BinStyle {
            pos_from_t: Pixels(0.0),
            pos_from_l: Pixels(0.0),
            pos_from_r: Pixels(0.0),
            height: Pixels(32.0),
            padding_t: Pixels(7.0),
            padding_l: Pixels(8.0),
            back_color: Color::shex("c0c0c0"),
            border_size_b: Pixels(1.0),
            border_color_b: Color::shex("707070"),
            text_body: TextBody {
                base_attrs: TextAttrs {
                    height: Pixels(16.0),
                    color: Color::shex("303030"),
                    ..Default::default()
                },
                ..TextBody::from(format!("{:?}", window.id()))
            },
            ..Default::default()
        })
        .expect_valid();

    window.keep_alive([background, window_id_disp]);
}
