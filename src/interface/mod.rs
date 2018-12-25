pub mod interface;
pub mod bin;
pub mod slider;
pub mod scroll_bar;
pub mod checkbox;
//pub(crate) mod text;
mod odb;
//pub(crate) mod text2;
pub(crate) mod text3;
pub(crate) use self::text3 as text2;
pub mod scroll_bar2;
pub mod renderer;

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
