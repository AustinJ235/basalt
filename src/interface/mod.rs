pub mod bin;
pub mod checkbox;
pub mod hook;
pub mod interface;
mod odb;
pub mod render;
pub mod scroll_bar;
pub mod slider;
pub(crate) mod text;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextWrap {
    None,
    Shift,
    NewLine,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TextAlign {
    Left,
    Right,
    Center,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WrapTy {
    ShiftX(f32),
    ShiftY(f32),
    Normal(f32, f32),
    None,
}
