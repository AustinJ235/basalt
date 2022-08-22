pub mod bin_focus;
pub mod press;
pub mod release;

pub(in crate::input_v2) use bin_focus::bin_focus;
pub(in crate::input_v2) use press::press;
pub(in crate::input_v2) use release::release;
