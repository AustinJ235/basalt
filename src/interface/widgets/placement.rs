use crate::interface::UnitValue::Undefined;
use crate::interface::{BinStyle, FloatWeight, Position, UnitValue, ZIndex};

#[derive(Default, Debug, Clone, PartialEq)]
pub struct WidgetPlacement {
    pub position: Position,
    pub z_index: ZIndex,
    pub float_weight: FloatWeight,
    pub pos_from_t: UnitValue,
    pub pos_from_b: UnitValue,
    pub pos_from_l: UnitValue,
    pub pos_from_r: UnitValue,
    pub width: UnitValue,
    pub height: UnitValue,
    pub margin_t: UnitValue,
    pub margin_b: UnitValue,
    pub margin_l: UnitValue,
    pub margin_r: UnitValue,
}

pub struct WidgetPlcmtError {
    pub kind: WidgetPlcmtErrorKind,
    pub desc: &'static str,
}

pub enum WidgetPlcmtErrorKind {
    NotConstrained,
    TooConstrained,
}

impl WidgetPlacement {
    #[allow(dead_code)]
    pub(crate) fn validate(&self) -> Result<(), WidgetPlcmtError> {
        match self.position {
            Position::Relative | Position::Anchor => {
                match [
                    self.pos_from_t.is_defined(),
                    self.pos_from_b.is_defined(),
                    self.height.is_defined(),
                ] {
                    [false, false, false] => {
                        return Err(WidgetPlcmtError {
                            kind: WidgetPlcmtErrorKind::NotConstrained,
                            desc: "Two of 'pos_from_t`, 'pos_from_b` and 'height' must be defined.",
                        });
                    },
                    [true, true, true] => {
                        return Err(WidgetPlcmtError {
                            kind: WidgetPlcmtErrorKind::TooConstrained,
                            desc: "Only two of 'pos_from_t`, 'pos_from_b` and 'height' must be \
                                   defined.",
                        });
                    },
                    [true, false, false] => {
                        return Err(WidgetPlcmtError {
                            kind: WidgetPlcmtErrorKind::NotConstrained,
                            desc: "'pos_from_t' is defined, but either 'pos_from_b' or 'height' \
                                   must also be defined.",
                        });
                    },
                    [false, true, false] => {
                        return Err(WidgetPlcmtError {
                            kind: WidgetPlcmtErrorKind::NotConstrained,
                            desc: "'pos_from_b' is defined, but either 'pos_from_t' or 'height' \
                                   must also be defined.",
                        });
                    },
                    [false, false, true] => {
                        return Err(WidgetPlcmtError {
                            kind: WidgetPlcmtErrorKind::NotConstrained,
                            desc: "'height' is defined, but either 'pos_from_t' or 'pos_from_b' \
                                   must also be defined.",
                        });
                    },
                    _ => (),
                }

                match [
                    self.pos_from_l.is_defined(),
                    self.pos_from_r.is_defined(),
                    self.width.is_defined(),
                ] {
                    [false, false, false] => {
                        return Err(WidgetPlcmtError {
                            kind: WidgetPlcmtErrorKind::NotConstrained,
                            desc: "Two of 'pos_from_l`, 'pos_from_r` and 'width' must be defined.",
                        });
                    },
                    [true, true, true] => {
                        return Err(WidgetPlcmtError {
                            kind: WidgetPlcmtErrorKind::TooConstrained,
                            desc: "Only two of 'pos_from_l`, 'pos_from_r` and 'width' must be \
                                   defined.",
                        });
                    },
                    [true, false, false] => {
                        return Err(WidgetPlcmtError {
                            kind: WidgetPlcmtErrorKind::NotConstrained,
                            desc: "'left' is defined, but either 'pos_from_r' or 'width' must \
                                   also be defined.",
                        });
                    },
                    [false, true, false] => {
                        return Err(WidgetPlcmtError {
                            kind: WidgetPlcmtErrorKind::NotConstrained,
                            desc: "'right' is defined, but either 'pos_from_l' or 'width' must \
                                   also be defined.",
                        });
                    },
                    [false, false, true] => {
                        return Err(WidgetPlcmtError {
                            kind: WidgetPlcmtErrorKind::NotConstrained,
                            desc: "'width' is defined, but either 'pos_from_l' or 'right' must \
                                   also be defined.",
                        });
                    },
                    _ => (),
                }
            },
            Position::Floating => {
                if self.width == Undefined {
                    return Err(WidgetPlcmtError {
                        kind: WidgetPlcmtErrorKind::NotConstrained,
                        desc: "'width' must be defined.",
                    });
                }

                if self.height == Undefined {
                    return Err(WidgetPlcmtError {
                        kind: WidgetPlcmtErrorKind::NotConstrained,
                        desc: "'height' must be defined.",
                    });
                }
            },
        }

        Ok(())
    }

    pub(crate) fn into_style(self) -> BinStyle {
        BinStyle {
            position: self.position,
            z_index: self.z_index,
            float_weight: self.float_weight,
            pos_from_t: self.pos_from_t,
            pos_from_b: self.pos_from_b,
            pos_from_l: self.pos_from_l,
            pos_from_r: self.pos_from_r,
            width: self.width,
            height: self.height,
            margin_t: self.margin_t,
            margin_b: self.margin_b,
            margin_l: self.margin_l,
            margin_r: self.margin_r,
            ..Default::default()
        }
    }
}
