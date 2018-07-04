pub(crate) mod font;
pub mod interface;
pub mod bin;
pub mod slider;
pub mod scroll_bar;
pub mod checkbox;
mod itf_dual_buf;

#[derive(Clone,Copy)]
pub enum TextWrap {
	None,
	Shift,
	NewLine
}

