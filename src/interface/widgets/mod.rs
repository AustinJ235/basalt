#![allow(clippy::significant_drop_in_scrutinee)]
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::module_inception)]
#![allow(clippy::doc_lazy_continuation)]

pub mod builder;
pub mod error;
mod placement;

mod button;
mod check_box;
mod code_editor;
mod frame;
mod notebook;
mod progress_bar;
mod radio_button;
mod scaler;
mod scroll_bar;
mod select;
mod spin_button;
mod switch_button;
mod text_editor;
mod text_entry;
mod text_hooks;
mod theme;
mod toggle_button;

use std::sync::Arc;

use self::builder::WidgetBuilder;
pub use self::button::Button;
pub use self::check_box::CheckBox;
pub use self::code_editor::CodeEditor;
pub use self::frame::{Frame, ScrollBarVisibility};
pub use self::notebook::Notebook;
pub use self::placement::{WidgetPlacement, WidgetPlcmtError, WidgetPlcmtErrorKind};
pub use self::progress_bar::ProgressBar;
pub use self::radio_button::{RadioButton, RadioButtonGroup};
pub use self::scaler::{Scaler, ScalerOrientation, ScalerRound};
pub use self::scroll_bar::{ScrollAxis, ScrollBar};
pub use self::select::Select;
pub use self::spin_button::SpinButton;
pub use self::switch_button::SwitchButton;
pub use self::text_editor::TextEditor;
pub use self::text_entry::TextEntry;
pub use self::theme::{Theme, ThemeColors};
pub use self::toggle_button::ToggleButton;
use crate::interface::Bin;
use crate::window::Window;

/// Trait used by containers that support containing widgets or `Bin`'s.
pub trait Container: Sized {
    /// Create a child widget.
    fn create_widget(&self) -> WidgetBuilder<'_, Self> {
        WidgetBuilder::from(self)
    }

    /// Create a child [`Arc<Bin>`](`Bin`).
    fn create_bin(&self) -> Arc<Bin> {
        self.create_bins(1).next().unwrap()
    }

    /// Create many child [`Arc<Bin>`](`Bin`)'s.
    fn create_bins(&self, count: usize) -> impl Iterator<Item = Arc<Bin>>;
}

impl Container for Arc<Bin> {
    fn create_bins(&self, count: usize) -> impl Iterator<Item = Arc<Bin>> {
        self.window()
            .unwrap()
            .new_bins(count)
            .into_iter()
            .map(|child| {
                self.add_child(child.clone());
                child
            })
    }
}

impl Container for Arc<Window> {
    fn create_bins(&self, count: usize) -> impl Iterator<Item = Arc<Bin>> {
        self.new_bins(count).into_iter()
    }
}

impl<'a, T> Container for &'a T
where
    T: Container,
{
    fn create_bins(&self, count: usize) -> impl Iterator<Item = Arc<Bin>> {
        (*self).create_bins(count)
    }
}
