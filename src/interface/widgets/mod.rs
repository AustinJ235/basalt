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

/// Trait used by containers that support containing widgets.
pub trait WidgetContainer: Sized {
    fn container_bin(&self) -> &Arc<Bin>;

    fn create_widget(&self) -> WidgetBuilder<'_, Self> {
        WidgetBuilder::from(self)
    }

    fn default_theme(&self) -> Theme {
        Theme::default()
    }
}

impl WidgetContainer for Arc<Bin> {
    fn container_bin(&self) -> &Arc<Bin> {
        self
    }
}

// TODO: More Generic
impl WidgetContainer for &Arc<Bin> {
    fn container_bin(&self) -> &Arc<Bin> {
        *self
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
