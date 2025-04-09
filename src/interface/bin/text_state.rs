use cosmic_text as ct;

use crate::image::ImageMap;
use crate::interface::{
    Color, FontFamily, FontStretch, FontStyle, FontWeight, ItfVertInfo, LineLimit, LineSpacing,
    TextBody, TextHoriAlign, TextVertAlign, TextWrap, UnitValue, UpdateContext,
};

pub struct TextState {
    layout_valid: bool,
    layout_scale: f32,
    layout_size: [f32; 2],
    layout: Layout,

    vertexes_valid: bool,
    vertexes_position: [f32; 2],
}

struct Layout {
    spans: Vec<Span>,
    text_wrap: TextWrap,
    line_limit: LineLimit,
    vert_align: TextVertAlign,
    hori_align: TextHoriAlign,
}

struct Span {
    text: String,
    text_color: Color,
    text_height: f32,
    text_secret: bool,
    line_height: f32,
    font_family: FontFamily,
    font_weight: FontWeight,
    font_stretch: FontStretch,
    font_style: FontStyle,
}

impl Default for TextState {
    fn default() -> Self {
        todo!()
    }
}

impl TextState {
    pub fn update(&mut self, tlwh: [f32; 4], body: &TextBody, context: &mut UpdateContext) {
        let new_layout_size = [tlwh[2] / context.scale, tlwh[3] / context.scale];

        if new_layout_size != self.layout_size || context.scale != self.layout_scale {
            self.layout_size = new_layout_size;
            self.layout_scale = context.scale;
            self.invalidate();
        }

        if self.layout_valid {
            'validity: {
                if body.line_limit != self.layout.line_limit
                    || body.text_wrap != self.layout.text_wrap
                    || body.vert_align != self.layout.vert_align
                    || body.hori_align != self.layout.hori_align
                {
                    break 'validity self.invalidate();
                }

                if body.spans.len() != self.layout.spans.len() {
                    break 'validity self.invalidate();
                }

                for (body_span, layout_span) in body.spans.iter().zip(self.layout.spans.iter()) {
                    if body_span.attrs.secret != layout_span.text_secret {
                        break 'validity self.invalidate();
                    }

                    if body_span.attrs.secret {
                        if body_span.text.len() != layout_span.text.len() {
                            break 'validity self.invalidate();
                        }
                    } else {
                        if body_span.text != layout_span.text {
                            break 'validity self.invalidate();
                        }
                    }

                    if body_span.attrs.color != layout_span.text_color {
                        break 'validity self.invalidate();
                    }

                    let text_height = match body_span.attrs.height {
                        UnitValue::Undefined => 12.0, // TODO: This should probably be apart of Default Font?
                        UnitValue::Pixels(px) => px,
                        UnitValue::Percent(pct) => self.layout_size[1] * (pct / 100.0),
                        UnitValue::PctOffsetPx(pct, off_px) => {
                            (self.layout_size[1] * (pct / 100.0)) + off_px
                        },
                    } / self.layout_scale;

                    if text_height != layout_span.text_height {
                        break 'validity self.invalidate();
                    }

                    let line_height = match body.line_spacing {
                        LineSpacing::HeightMult(mult) => text_height * mult,
                        LineSpacing::HeightMultAdd(mult, add) => (text_height * mult) + add,
                    };

                    if line_height != layout_span.line_height {
                        break 'validity self.invalidate();
                    }

                    let font_family = match &body_span.attrs.font_family {
                        FontFamily::Inheirt => {
                            match &body.base_attrs.font_family {
                                FontFamily::Inheirt => &context.default_font.family,
                                base_family => &base_family,
                            }
                        },
                        span_family => &span_family,
                    };

                    if *font_family != layout_span.font_family {
                        break 'validity self.invalidate();
                    }

                    let font_weight = match body_span.attrs.font_weight {
                        FontWeight::Inheirt => {
                            match body.base_attrs.font_weight {
                                FontWeight::Inheirt => context.default_font.weight,
                                base_weight => base_weight,
                            }
                        },
                        span_weight => span_weight,
                    };

                    if font_weight != layout_span.font_weight {
                        break 'validity self.invalidate();
                    }

                    let font_stretch = match body_span.attrs.font_stretch {
                        FontStretch::Inheirt => {
                            match body.base_attrs.font_stretch {
                                FontStretch::Inheirt => context.default_font.stretch,
                                base_stretch => base_stretch,
                            }
                        },
                        span_stretch => span_stretch,
                    };

                    if font_stretch != layout_span.font_stretch {
                        break 'validity self.invalidate();
                    }

                    let font_style = match body_span.attrs.font_style {
                        FontStyle::Inheirt => {
                            match body.base_attrs.font_style {
                                FontStyle::Inheirt => context.default_font.style,
                                base_style => base_style,
                            }
                        },
                        span_style => span_style,
                    };

                    if font_style != layout_span.font_style {
                        break 'validity self.invalidate();
                    }
                }
            }
        }

        if !self.layout_valid {
            todo!()
        }
    }

    fn invalidate(&mut self) {
        self.layout_valid = false;
        self.vertexes_valid = false;
    }

    pub fn output_reserve(
        &mut self,
        _tlwh: [f32; 4],
        _z: f32,
        _opacity: f32,
        _output: &mut ImageMap<Vec<ItfVertInfo>>,
    ) {
        todo!()
    }

    pub fn output_vertexes(
        &mut self,
        _tlwh: [f32; 4],
        _z: f32,
        _opacity: f32,
        _output: &mut ImageMap<Vec<ItfVertInfo>>,
    ) {
        todo!()
    }

    pub fn bounds(&self) -> Option<[f32; 4]> {
        todo!()
    }
}
