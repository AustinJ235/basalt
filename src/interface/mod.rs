pub(crate) mod font;
pub mod interface;
pub mod bin;
pub mod slider;
pub mod scroll_bar;
pub mod checkbox;
mod itf_dual_buf;
pub(crate) mod text;

#[derive(Clone,Copy)]
pub enum TextWrap {
	None,
	Shift,
	NewLine
}

