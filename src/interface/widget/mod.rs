pub mod button;
pub mod checkbox;
pub mod combo_box;
pub mod context_menu;
pub mod hori_scale;
pub mod hori_scroll_bar;
pub mod image;
pub mod label;
pub mod list_box;
pub mod menu_bar;
pub mod multi_line_entry;
pub mod pager;
pub mod progress_bar;
pub mod radio_button;
pub mod single_line_entry;
pub mod spin_button;
pub mod status;
pub mod switch_button;
pub mod theme;
pub mod toggle_button;
pub mod vert_scale;
pub mod vert_scroll_bar;

pub trait Widget {
    fn set_theme();
    fn current_theme();
}