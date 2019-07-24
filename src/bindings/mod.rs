// bindgen --no-layout-tests  -o ./src/bindings/xinput2.rs /usr/include/X11/extensions/XInput2.h --raw-line="#[link(name = \"x11\")] extern {}"

pub mod harfbuzz;
#[cfg(target_os = "linux")]
pub mod xinput2;

