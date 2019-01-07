pub mod interface;
pub mod bin;
pub mod slider;
pub mod checkbox;
mod odb;
pub(crate) mod text;
pub mod scroll_bar;
pub mod render;
pub mod hook;

#[derive(Clone,Copy,PartialEq)]
pub enum TextWrap {
	None,
	Shift,
	NewLine
}

#[derive(Clone,Debug,PartialEq)]
pub enum TextAlign {
	Left,
	Right,
	Center
}

#[derive(Clone,Debug,PartialEq)]
pub enum WrapTy {
	ShiftX(f32),
	ShiftY(f32),
	Normal(f32, f32),
	None
}

