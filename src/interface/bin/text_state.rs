use std::ops::Range;
use std::sync::Arc;

use cosmic_text as ct;

use crate::image::{
    ImageCache, ImageCacheLifetime, ImageData, ImageFormat, ImageInfo, ImageKey, ImageMap, ImageSet,
};
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
    image_info_cache: ImageMap<Option<ImageInfo>>,

    vertexes_valid: bool,
    vertexes_position: [f32; 2],
}

struct Layout {
    spans: Vec<Span>,
    text_wrap: TextWrap,
    line_limit: LineLimit,
    vert_align: TextVertAlign,
    hori_align: TextHoriAlign,
    lines: Vec<LayoutLine>,
    glyphs: Vec<LayoutGlyph>,
    bounds: [f32; 4],
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

struct LayoutGlyph {
    span_i: usize,
    line_i: usize,
    offset: [f32; 2],
    extent: [f32; 2],
    hitbox: [f32; 4],
    image_key: ImageKey,
    vertex_type: i32,
}

struct LayoutLine {
    height: f32,
    glyphs: Range<usize>,
}

struct GlyphImageData {
    vertex_type: i32,
    placement_top: i32,
    placement_left: i32,
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            buffer_op: None,
            layout_valid: false,
            layout_scale: 1.0,
            layout_size: [0.0; 2],
            layout_op: None,
            image_info_cache: ImageMap::new(),
            vertexes_valid: false,
            vertexes_position: [0.0; 2],
        }
    }
}

impl TextState {
    pub fn update(
        &mut self,
        tlwh: [f32; 4],
        body: &TextBody,
        context: &mut UpdateContext,
        image_cache: &Arc<ImageCache>,
    ) {
        if body.is_empty() {
            self.buffer_op = None;
            self.layout_op = None;
            self.image_info_cache.clear();
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

                    let text_height = match match body_span.attrs.height {
                        UnitValue::Undefined => body.base_attrs.height,
                        span_height => span_height,
                    } {
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

                    let text_height = match match span.attrs.height {
                        UnitValue::Undefined => body.base_attrs.height,
                        span_height => span_height,
                    } {
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
            lines: Vec::new(),
            glyphs: Vec::new(),
            bounds: [0.0; 4],
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
        let layout = self.layout_op.as_mut().unwrap();

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

        let mut layout_glyphs = Vec::new();
        let mut layout_lines: Vec<LayoutLine> = Vec::new();

        for (line_i, run) in buffer.layout_runs().enumerate() {
            for l_glyph in run.glyphs.iter() {
                let p_glyph = l_glyph.physical((0.0, 0.0), 1.0);
                let span_i = l_glyph.metadata;

                layout_glyphs.push(LayoutGlyph {
                    span_i,
                    line_i,
                    offset: [p_glyph.x as f32, p_glyph.y as f32 + run.line_y],
                    extent: [0.0; 2],
                    hitbox: [
                        l_glyph.x,
                        l_glyph.x + l_glyph.w,
                        l_glyph.y
                            + run.line_top
                            + l_glyph
                                .line_height_opt
                                .map(|glyph_lh| run.line_height - glyph_lh)
                                .unwrap_or(0.0),
                        l_glyph.y + run.line_top + run.line_height,
                    ],
                    image_key: ImageKey::glyph(p_glyph.cache_key),
                    vertex_type: 0,
                });

                match layout_lines.get_mut(line_i) {
                    Some(layout_line) => {
                        layout_line.glyphs.end += 1;
                    },
                    None => {
                        layout_lines.push(LayoutLine {
                            height: run.line_height,
                            glyphs: (layout_glyphs.len() - 1)..layout_glyphs.len(),
                        });
                    },
                }
            }
        }

        let mut image_keys = ImageSet::new();

        for glyph in layout_glyphs.iter() {
            image_keys.insert(glyph.image_key.clone());
        }

        self.image_info_cache
            .retain(|image_key, _| image_keys.contains(image_key));

        let mut missing_image_keys = Vec::new();

        for image_key in image_keys.iter() {
            if !self.image_info_cache.contains(image_key) {
                missing_image_keys.push(image_key);
            }
        }

        if !missing_image_keys.is_empty() {
            for (image_key, image_info_op) in missing_image_keys
                .clone()
                .into_iter()
                .zip(image_cache.obtain_image_infos(missing_image_keys))
            {
                if let Some(image_info) = image_info_op {
                    self.image_info_cache
                        .insert(image_key.clone(), Some(image_info));
                    continue;
                }

                if let Some(image) = context
                    .glyph_cache
                    .get_image_uncached(&mut context.font_system, *image_key.as_glyph().unwrap())
                {
                    if image.placement.width == 0
                        || image.placement.height == 0
                        || image.data.is_empty()
                    {
                        self.image_info_cache.insert(image_key.clone(), None);
                        continue;
                    }

                    let (vertex_type, image_format) = match image.content {
                        ct::SwashContent::Mask => (2, ImageFormat::LMono),
                        ct::SwashContent::SubpixelMask => (2, ImageFormat::LRGBA),
                        ct::SwashContent::Color => (100, ImageFormat::LRGBA),
                    };

                    let image_info = image_cache
                        .load_raw_image(
                            image_key.clone(),
                            ImageCacheLifetime::Indefinite,
                            image_format,
                            image.placement.width,
                            image.placement.height,
                            GlyphImageData {
                                vertex_type,
                                placement_top: image.placement.top,
                                placement_left: image.placement.left,
                            },
                            ImageData::D8(image.data.into_iter().collect()),
                        )
                        .unwrap();

                    self.image_info_cache
                        .insert(image_key.clone(), Some(image_info));
                    continue;
                }

                self.image_info_cache.insert(image_key.clone(), None);
            }
        }

        let mut bounds = [
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ];

        let body_height = layout_lines.iter().map(|line| line.height).sum::<f32>();

        let vert_align_offset = match layout.vert_align {
            TextVertAlign::Top => 0.0,
            TextVertAlign::Center => (self.layout_size[1] - body_height) / 2.0,
            TextVertAlign::Bottom => self.layout_size[1] - body_height,
        };

        bounds[2] = bounds[2].min(body_height + vert_align_offset);
        bounds[2] = bounds[2].max(body_height + vert_align_offset);

        for glyph in layout_glyphs.iter_mut() {
            match self.image_info_cache.get(&glyph.image_key).unwrap() {
                Some(image_info) => {
                    let image_data = image_info.associated_data::<GlyphImageData>().unwrap();
                    glyph.offset[0] += image_data.placement_left as f32;
                    glyph.offset[1] -= image_data.placement_top as f32;
                    glyph.extent[0] = image_info.width as f32;
                    glyph.extent[1] = image_info.height as f32;
                    glyph.vertex_type = image_data.vertex_type;
                },
                None => {
                    glyph.image_key = ImageKey::INVALID;
                },
            }

            glyph.offset[1] += vert_align_offset;
            glyph.hitbox[2] += vert_align_offset;
            glyph.hitbox[3] += vert_align_offset;
        }

        for line in layout_lines.iter() {
            let mut line_x_mm = [f32::INFINITY, f32::NEG_INFINITY];

            for glyph_i in line.glyphs.clone() {
                line_x_mm[0] = line_x_mm[0].min(layout_glyphs[glyph_i].offset[0]);
                line_x_mm[1] = line_x_mm[1]
                    .max(layout_glyphs[glyph_i].offset[0] + layout_glyphs[glyph_i].extent[0]);
            }

            let line_width = line_x_mm[1] - line_x_mm[0];

            let hori_align_offset =
                match if layout.text_wrap == TextWrap::Shift && line_width > self.layout_size[0] {
                    TextHoriAlign::Right
                } else {
                    layout.hori_align
                } {
                    TextHoriAlign::Left => 0.0,
                    TextHoriAlign::Center => (self.layout_size[0] - line_width) / 2.0,
                    TextHoriAlign::Right => self.layout_size[0] - line_width,
                };

            bounds[0] = bounds[0].min(line_x_mm[0] + hori_align_offset);
            bounds[1] = bounds[1].max(line_x_mm[1] + hori_align_offset);

            for glyph_i in line.glyphs.clone() {
                layout_glyphs[glyph_i].offset[0] += hori_align_offset;
                layout_glyphs[glyph_i].hitbox[0] += hori_align_offset;
                layout_glyphs[glyph_i].hitbox[1] += hori_align_offset;
            }
        }

        layout.lines = layout_lines;
        layout.glyphs = layout_glyphs;
        layout.bounds = bounds;
        self.layout_valid = true;
    }

    fn invalidate(&mut self) {
        self.layout_valid = false;
        self.vertexes_valid = false;
    }

    pub fn output_reserve(&mut self, output: &mut ImageMap<Vec<ItfVertInfo>>) {
        for image_key in self.image_info_cache.keys() {
            output.try_insert(image_key, Vec::new);
        }
    }

    pub fn output_vertexes(
        &mut self,
        tlwh: [f32; 4],
        z: f32,
        opacity: f32,
        output: &mut ImageMap<Vec<ItfVertInfo>>,
    ) {
        if self.layout_op.is_none() {
            return;
        }

        let layout = self.layout_op.as_ref().unwrap();

        for glyph in layout.glyphs.iter() {
            if !glyph.image_key.is_invalid() {
                let t1 = tlwh[0] + (glyph.offset[1] * self.layout_scale);
                let b1 = t1 + (glyph.extent[1] * self.layout_scale);
                let l1 = tlwh[1] + (glyph.offset[0] * self.layout_scale);
                let r1 = l1 + (glyph.extent[0] * self.layout_scale);
                let t2 = 0.0;
                let b2 = glyph.extent[1];
                let l2 = 0.0;
                let r2 = glyph.extent[0];
                let mut color = layout.spans[glyph.span_i].text_color.rgbaf_array();
                color[3] *= opacity;
                let ty = glyph.vertex_type;

                output.try_insert_then(
                    &glyph.image_key,
                    Vec::new,
                    |vertexes: &mut Vec<ItfVertInfo>| {
                        vertexes.extend(
                            [
                                ItfVertInfo {
                                    position: [r1, t1, z],
                                    coords: [r2, t2],
                                    color,
                                    ty,
                                    tex_i: 0,
                                },
                                ItfVertInfo {
                                    position: [l1, t1, z],
                                    coords: [l2, t2],
                                    color,
                                    ty,
                                    tex_i: 0,
                                },
                                ItfVertInfo {
                                    position: [l1, b1, z],
                                    coords: [l2, b2],
                                    color,
                                    ty,
                                    tex_i: 0,
                                },
                                ItfVertInfo {
                                    position: [r1, t1, z],
                                    coords: [r2, t2],
                                    color,
                                    ty,
                                    tex_i: 0,
                                },
                                ItfVertInfo {
                                    position: [l1, b1, z],
                                    coords: [l2, b2],
                                    color,
                                    ty,
                                    tex_i: 0,
                                },
                                ItfVertInfo {
                                    position: [r1, b1, z],
                                    coords: [r2, b2],
                                    color,
                                    ty,
                                    tex_i: 0,
                                },
                            ]
                            .into_iter(),
                        );

                        // Hitbox Test

                        /*for [x, y] in [
                            [glyph.hitbox[1], glyph.hitbox[2]],
                            [glyph.hitbox[0], glyph.hitbox[2]],
                            [glyph.hitbox[0], glyph.hitbox[3]],
                            [glyph.hitbox[1], glyph.hitbox[2]],
                            [glyph.hitbox[0], glyph.hitbox[3]],
                            [glyph.hitbox[1], glyph.hitbox[3]],
                        ] {
                            let position = [
                                (x * self.layout_scale) + tlwh[1],
                                (y * self.layout_scale) + tlwh[0],
                                z + 0.0001,
                            ];

                            vertexes.push(ItfVertInfo {
                                position,
                                coords: [0.0; 2],
                                color: [0.0, 0.0, 1.0, 0.2],
                                ty: 0,
                                tex_i: 0,
                            });
                        }*/
                    },
                );
            }
        }
    }

    pub fn bounds(&self, tlwh: [f32; 4]) -> Option<[f32; 4]> {
        self.layout_op.as_ref().map(|layout| {
            [
                tlwh[1] + (layout.bounds[0] * self.layout_scale),
                tlwh[1] + (layout.bounds[1] * self.layout_scale),
                tlwh[0] + (layout.bounds[2] * self.layout_scale),
                tlwh[0] + (layout.bounds[3] * self.layout_scale),
            ]
        })
    }
}
