use cosmic_text as ct;

use crate::image::ImageMap;
use crate::interface::{
    Color, FontFamily, FontStretch, FontStyle, FontWeight, ItfVertInfo, LineLimit, LineSpacing,
    TextBody, TextHoriAlign, TextVertAlign, TextWrap, UnitValue, UpdateContext,
};

pub struct TextState {
    buffer_op: Option<ct::Buffer>,

    layout_valid: bool,
    layout_scale: f32,
    layout_size: [f32; 2],
    layout_op: Option<Layout>,

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

impl Layout {
    fn cosmic_attrs(&self) -> ct::Attrs {
        ct::Attrs {
            color_opt: None,
            family: ct::Family::Serif,
            stretch: ct::Stretch::Normal,
            style: ct::Style::Normal,
            weight: ct::Weight(400),
            metadata: 0,
            cache_key_flags: ct::CacheKeyFlags::empty(),
            metrics_opt: None,
            letter_spacing_opt: None,
            font_features: Default::default(),
        }
    }
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

impl Span {
    fn cosmic_attrs(&self, metadata: usize) -> ct::Attrs {
        ct::Attrs {
            color_opt: None,
            family: self.font_family.into_cosmic().unwrap(),
            stretch: self.font_stretch.into_cosmic().unwrap(),
            style: self.font_style.into_cosmic().unwrap(),
            weight: self.font_weight.into_cosmic().unwrap(),
            metadata,
            cache_key_flags: ct::CacheKeyFlags::empty(),
            metrics_opt: Some(
                ct::Metrics {
                    font_size: self.text_height,
                    line_height: self.line_height,
                }
                .into(),
            ),
            letter_spacing_opt: None,
            font_features: Default::default(),
        }
    }
}

impl Default for TextState {
    fn default() -> Self {
        todo!()
    }
}

impl TextState {
    pub fn update(&mut self, tlwh: [f32; 4], body: &TextBody, context: &mut UpdateContext) {
        if body.is_empty() {
            self.buffer_op = None;
            self.layout_op = None;
            return;
        }

        let new_layout_size = [tlwh[2] / context.scale, tlwh[3] / context.scale];

        if new_layout_size != self.layout_size || context.scale != self.layout_scale {
            self.layout_size = new_layout_size;
            self.layout_scale = context.scale;
            self.invalidate();
        }

        if self.layout_valid {
            'validity: {
                if self.layout_op.is_none() {
                    break 'validity self.invalidate();
                }

                let layout = self.layout_op.as_ref().unwrap();

                if body.line_limit != layout.line_limit
                    || body.text_wrap != layout.text_wrap
                    || body.vert_align != layout.vert_align
                    || body.hori_align != layout.hori_align
                {
                    break 'validity self.invalidate();
                }

                if body.spans.len() != layout.spans.len() {
                    break 'validity self.invalidate();
                }

                for (body_span, layout_span) in body.spans.iter().zip(layout.spans.iter()) {
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
                        // TODO: This should probably be apart of Default Font?
                        UnitValue::Undefined => 12.0 / self.layout_scale,
                        UnitValue::Pixels(px) => px / self.layout_scale,
                        UnitValue::Percent(pct) => self.layout_size[1] * (pct / 100.0),
                        UnitValue::PctOffsetPx(pct, off_px) => {
                            (self.layout_size[1] * (pct / 100.0)) + off_px
                        },
                    };

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

        if self.layout_valid {
            return;
        }

        self.layout_op = Some(Layout {
            spans: body
                .spans
                .iter()
                .map(|span| {
                    let text = if span.attrs.secret {
                        (0..span.text.len()).map(|_| '*').collect::<String>()
                    } else {
                        span.text.clone()
                    };

                    let text_height = match span.attrs.height {
                        // TODO: This should probably be apart of Default Font?
                        UnitValue::Undefined => 12.0 / self.layout_scale,
                        UnitValue::Pixels(px) => px / self.layout_scale,
                        UnitValue::Percent(pct) => self.layout_size[1] * (pct / 100.0),
                        UnitValue::PctOffsetPx(pct, off_px) => {
                            (self.layout_size[1] * (pct / 100.0)) + off_px
                        },
                    };

                    let line_height = match body.line_spacing {
                        LineSpacing::HeightMult(mult) => text_height * mult,
                        LineSpacing::HeightMultAdd(mult, add) => (text_height * mult) + add,
                    };

                    let font_family = match span.attrs.font_family.clone() {
                        FontFamily::Inheirt => {
                            match body.base_attrs.font_family.clone() {
                                FontFamily::Inheirt => context.default_font.family.clone(),
                                base_family => base_family,
                            }
                        },
                        span_family => span_family,
                    };

                    let font_weight = match span.attrs.font_weight {
                        FontWeight::Inheirt => {
                            match body.base_attrs.font_weight {
                                FontWeight::Inheirt => context.default_font.weight,
                                base_weight => base_weight,
                            }
                        },
                        span_weight => span_weight,
                    };

                    let font_stretch = match span.attrs.font_stretch {
                        FontStretch::Inheirt => {
                            match body.base_attrs.font_stretch {
                                FontStretch::Inheirt => context.default_font.stretch,
                                base_stretch => base_stretch,
                            }
                        },
                        span_stretch => span_stretch,
                    };

                    let font_style = match span.attrs.font_style {
                        FontStyle::Inheirt => {
                            match body.base_attrs.font_style {
                                FontStyle::Inheirt => context.default_font.style,
                                base_style => base_style,
                            }
                        },
                        span_style => span_style,
                    };

                    Span {
                        text,
                        text_color: span.attrs.color,
                        text_height,
                        text_secret: span.attrs.secret,
                        line_height,
                        font_family,
                        font_weight,
                        font_stretch,
                        font_style,
                    }
                })
                .collect(),
            text_wrap: body.text_wrap,
            line_limit: body.line_limit,
            vert_align: body.vert_align,
            hori_align: body.hori_align,
        });

        if self.buffer_op.is_none() {
            self.buffer_op = Some(ct::Buffer::new(
                &mut context.font_system,
                ct::Metrics {
                    font_size: 12.0,
                    line_height: 14.0,
                },
            ));
        }

        let buffer = self.buffer_op.as_mut().unwrap();
        let layout = self.layout_op.as_ref().unwrap();

        let buffer_width_op = if matches!(layout.text_wrap, TextWrap::None | TextWrap::Shift) {
            None
        } else {
            Some(self.layout_size[0])
        };

        buffer.set_size(&mut context.font_system, buffer_width_op, None);

        buffer.set_rich_text(
            &mut context.font_system,
            layout
                .spans
                .iter()
                .enumerate()
                .map(|(i, span)| (span.text.as_str(), span.cosmic_attrs(i))),
            &layout.cosmic_attrs(),
            ct::Shaping::Advanced,
            None,
        );

        todo!()
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
