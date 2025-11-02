//! Builder types

use std::sync::Arc;

use crate::interface::Bin;
pub use crate::interface::widgets::button::ButtonBuilder;
pub use crate::interface::widgets::check_box::CheckBoxBuilder;
pub use crate::interface::widgets::code_editor::CodeEditorBuilder;
pub use crate::interface::widgets::frame::FrameBuilder;
pub use crate::interface::widgets::progress_bar::ProgressBarBuilder;
pub use crate::interface::widgets::radio_button::RadioButtonBuilder;
pub use crate::interface::widgets::scaler::ScalerBuilder;
pub use crate::interface::widgets::scroll_bar::ScrollBarBuilder;
pub use crate::interface::widgets::select::SelectBuilder;
pub use crate::interface::widgets::spin_button::SpinButtonBuilder;
pub use crate::interface::widgets::switch_button::SwitchButtonBuilder;
pub use crate::interface::widgets::text_editor::TextEditorBuilder;
pub use crate::interface::widgets::text_entry::TextEntryBuilder;
pub use crate::interface::widgets::toggle_button::ToggleButtonBuilder;
use crate::interface::widgets::{Container, Theme, WidgetPlacement};

/// General builder for widgets.
pub struct WidgetBuilder<'a, C> {
    pub(crate) container: &'a C,
    pub(crate) theme: Theme,
    pub(crate) placement: Option<WidgetPlacement>,
}

impl<'a, C> From<&'a C> for WidgetBuilder<'a, C>
where
    C: Container,
{
    fn from(container: &'a C) -> Self {
        Self {
            theme: Theme::default(),
            container,
            placement: None,
        }
    }
}

impl<'a, C> WidgetBuilder<'a, C>
where
    C: Container,
{
    /// Specify a theme to be used.
    ///
    /// **Note**: When not used the theme will be Basalt's default light theme.
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Specify how the widget should be placed.
    pub fn with_placement(mut self, placement: WidgetPlacement) -> Self {
        self.placement = Some(placement);
        self
    }

    /// Transition into building a [`Button`](crate::interface::widgets::Button)
    pub fn button(self) -> ButtonBuilder<'a, C> {
        ButtonBuilder::with_builder(self)
    }

    /// Transition into building a [`SpinButton`](crate::interface::widgets::SpinButton)
    pub fn spin_button(self) -> SpinButtonBuilder<'a, C> {
        SpinButtonBuilder::with_builder(self)
    }

    /// Transition into building a [`ToggleButton`](crate::interface::widgets::ToggleButton)
    pub fn toggle_button(self) -> ToggleButtonBuilder<'a, C> {
        ToggleButtonBuilder::with_builder(self)
    }

    /// Transition into building a [`SwitchButton`](crate::interface::widgets::SwitchButton)
    pub fn switch_button(self) -> SwitchButtonBuilder<'a, C> {
        SwitchButtonBuilder::with_builder(self)
    }

    /// Transition into building a [`Scaler`](crate::interface::widgets::Scaler)
    pub fn scaler(self) -> ScalerBuilder<'a, C> {
        ScalerBuilder::with_builder(self)
    }

    /// Transition into building a [`ProgressBar`](crate::interface::widgets::ProgressBar)
    pub fn progress_bar(self) -> ProgressBarBuilder<'a, C> {
        ProgressBarBuilder::with_builder(self)
    }

    /// Transition into building a [`RadioButton`](crate::interface::widgets::RadioButton)
    pub fn radio_button<T>(self, value: T) -> RadioButtonBuilder<'a, C, T>
    where
        T: Send + Sync + 'static,
    {
        RadioButtonBuilder::with_builder(self, value)
    }

    /// Transition into building a [`CheckBox`](crate::interface::widgets::CheckBox)
    pub fn check_box<T>(self, value: T) -> CheckBoxBuilder<'a, C, T>
    where
        T: Send + Sync + 'static,
    {
        CheckBoxBuilder::with_builder(self, value)
    }

    /// Transition into building a [`ScrollBar`](crate::interface::widgets::ScrollBar)
    pub fn scroll_bar(self, target: Arc<Bin>) -> ScrollBarBuilder<'a, C> {
        ScrollBarBuilder::with_builder(self, target)
    }

    /// Transition into building a [`Select`](crate::interface::widgets::Select)
    pub fn select<I>(self) -> SelectBuilder<'a, C, I>
    where
        I: Ord + Copy + Send + 'static,
    {
        SelectBuilder::with_builder(self)
    }

    /// Transition into building a [`TextEditor`](crate::interface::widgets::TextEditor)
    pub fn text_editor(self) -> TextEditorBuilder<'a, C> {
        TextEditorBuilder::with_builder(self)
    }

    /// Transition into building a [`TextEntry`](crate::interface::widgets::TextEntry)
    pub fn text_entry(self) -> TextEntryBuilder<'a, C> {
        TextEntryBuilder::with_builder(self)
    }

    /// Transition into building a [`CodeEditor`](crate::interface::widgets::CodeEditor)
    pub fn code_editor(self) -> CodeEditorBuilder<'a, C> {
        CodeEditorBuilder::with_builder(self)
    }

    /// Transition into builder a [`Frame`](crate::interface::widgets::Frame)
    pub fn frame(self) -> FrameBuilder<'a, C> {
        FrameBuilder::with_builder(self)
    }
}
