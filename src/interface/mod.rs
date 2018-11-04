pub mod interface;
pub mod bin;
pub mod slider;
pub mod scroll_bar;
pub mod checkbox;
pub(crate) mod text;
mod odb;

#[derive(Clone,Copy)]
pub enum TextWrap {
	None,
	Shift,
	NewLine
}

