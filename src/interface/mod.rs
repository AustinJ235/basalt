pub(crate) mod font;
pub mod interface;
pub mod bin;
pub mod slider;
pub mod scroll_bar;
pub mod checkbox;

#[derive(Clone,Copy)]
pub enum TextWrap {
	None,
	Shift,
	NewLine
}

