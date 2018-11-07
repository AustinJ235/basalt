pub mod interface;
pub mod bin;
pub mod slider;
pub mod scroll_bar;
pub mod checkbox;
pub(crate) mod text;
mod odb;
#[allow(warnings)]
pub(crate) mod text2;

#[derive(Clone,Copy)]
pub enum TextWrap {
	None,
	Shift,
	NewLine
}

