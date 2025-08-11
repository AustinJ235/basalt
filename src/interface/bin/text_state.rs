use std::cmp::Ordering;
use std::collections::BTreeMap;
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PosTextCursor {
    pub span: usize,
    pub byte_s: usize,
    pub byte_e: usize,
    pub affinity: TextCursorAffinity,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextCursor {
    #[default]
    None,
    Empty,
    Position(PosTextCursor),
}

impl TextCursor {
    pub fn into_position(self) -> Option<PosTextCursor> {
        match self {
            Self::Position(cursor) => Some(cursor),
            _ => None
        }
    }
}

impl From<PosTextCursor> for TextCursor {
    fn from(cursor: PosTextCursor) -> TextCursor {
        TextCursor::Position(cursor)
    }
}

impl PartialOrd for PosTextCursor {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PosTextCursor {
    fn cmp(&self, other: &Self) -> Ordering {
        self.span.cmp(&other.span).then(
            self.byte_s
                .cmp(&other.byte_s)
                .then(self.affinity.cmp(&other.affinity)),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextCursorAffinity {
    Before,
    After,
}

impl PartialOrd for TextCursorAffinity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TextCursorAffinity {
    fn cmp(&self, other: &Self) -> Ordering {
        match self {
            Self::Before => {
                match other {
                    Self::Before => Ordering::Equal,
                    Self::After => Ordering::Less,
                }
            },
            Self::After => {
                match other {
                    Self::Before => Ordering::Greater,
                    Self::After => Ordering::Equal,
                }
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSelection {
    pub start: PosTextCursor,
    pub end: PosTextCursor,
}

impl TextSelection {
    pub fn extend<C>(self, cursor: C) -> Self
    where
        C: Into<TextCursor>
    {
        match cursor.into() {
            TextCursor::None | TextCursor::Empty => self,
            TextCursor::Position(cursor) => {
                if cursor < self.start {
                    Self {
                        start: cursor,
                        ..self
                    }
                } else if cursor > self.end {
                    Self {
                        end: cursor,
                        ..self
                    }
                } else {
                    self
                }
            }
        }
    }
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
    fn cosmic_attrs(&self) -> ct::Attrs<'_> {
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
    fn cosmic_attrs(&self, metadata: usize) -> ct::Attrs<'_> {
        let mut font_features = ct::FontFeatures::default();

        // TODO: Ligatures are disabled as they break selection.
        font_features.disable(ct::FeatureTag::STANDARD_LIGATURES);
        font_features.disable(ct::FeatureTag::CONTEXTUAL_LIGATURES);

        ct::Attrs {
            color_opt: None,
            family: self.font_family.as_cosmic().unwrap(),
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
            font_features,
        }
    }
}

#[derive(Debug)]
struct LayoutGlyph {
    span_i: usize,
    byte_s: usize,
    byte_e: usize,
    offset: [f32; 2],
    extent: [f32; 2],
    hitbox: [f32; 4],
    image_extent: [f32; 2],
    image_key: ImageKey,
    vertex_type: i32,
}

#[derive(Debug)]
struct LayoutLine {
    bounds: [f32; 4],
    hitbox: [f32; 4],
    height: f32,
    glyphs: Range<usize>,
    s_cursor: PosTextCursor,
    e_cursor: PosTextCursor,
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
        }
    }
}

impl TextState {
    pub fn get_cursor(&self, cursor_position: [f32; 2]) -> TextCursor {
        let layout = match self.layout_op.as_ref() {
            Some(layout) => layout,
            None => return TextCursor::Empty,
        };

        if layout.lines.is_empty() {
            return TextCursor::Empty;
        }

        let f_line = layout.lines.first().unwrap();
        let l_line = layout.lines.last().unwrap();

        if cursor_position[1] < f_line.hitbox[2] {
            // Cursor is above the first line, use start of the first line
            return TextCursor::Position(f_line.s_cursor);
        }

        if cursor_position[1] > layout.lines.last().unwrap().hitbox[3] {
            // Cursor is below the last line, use end of the last line
            return TextCursor::Position(l_line.e_cursor);
        }

        // Find the closest line to the cursor.

        let mut line_i_op = None;
        let mut dist = 0.0;

        for (line_i, line) in layout.lines.iter().enumerate() {
            // TODO: Use baseline instead of center?
            let c = line.hitbox[2] + ((line.hitbox[3] - line.hitbox[2]) / 2.0);
            let d = (cursor_position[1] - c).abs();

            if line_i_op.is_none() {
                line_i_op = Some(line_i);
                dist = d;
                continue;
            }

            if d < dist {
                line_i_op = Some(line_i);
                dist = d;
            }
        }

        Self::get_cursor_on_line(layout, line_i_op.unwrap(), cursor_position[0])
    }

    fn get_cursor_on_line(layout: &Layout, line_i: usize, cursor_x: f32) -> TextCursor {
        if line_i >= layout.lines.len() {
            return TextCursor::None;
        }

        let line = &layout.lines[line_i];
        let glyphs = &layout.glyphs[line.glyphs.clone()];

        if glyphs.is_empty() {
            return line.e_cursor.into();
        }

        if cursor_x < glyphs.first().unwrap().hitbox[0] {
            // Cursor is to the left of the first glyph, use start of line
            return line.s_cursor.into();
        }

        if cursor_x > glyphs.last().unwrap().hitbox[1] {
            // Cursor is to the right of the last glyph, use end of line
            return line.e_cursor.into();
        }

        let mut glyph_i_op = None;
        let mut dist = 0.0;
        let mut affinity = TextCursorAffinity::Before;

        for (i, glyph) in glyphs.iter().enumerate() {
            let c = glyph.hitbox[0] + ((glyph.hitbox[1] - glyph.hitbox[0]) / 2.0);
            let d = (cursor_x - c).abs();

            let a = if cursor_x < c {
                TextCursorAffinity::Before
            } else {
                TextCursorAffinity::After
            };

            if glyph_i_op.is_none() {
                glyph_i_op = Some(i);
                dist = d;
                affinity = a;
                continue;
            }

            if d < dist {
                glyph_i_op = Some(i);
                dist = d;
                affinity = a;
            }
        }

        let glyph = &glyphs[glyph_i_op.unwrap()];

        PosTextCursor {
            span: glyph.span_i,
            byte_s: glyph.byte_s,
            byte_e: glyph.byte_e,
            affinity,
        }
        .into()
    }

    pub fn cursor_up(&self, cursor: TextCursor, text_body: &TextBody) -> TextCursor {
        self.cursor_line_offset(cursor, text_body, -1)
    }

    pub fn cursor_down(&self, cursor: TextCursor, text_body: &TextBody) -> TextCursor {
        self.cursor_line_offset(cursor, text_body, 1)
    }

    fn cursor_line_offset(
        &self,
        cursor: TextCursor,
        text_body: &TextBody,
        line_offset: isize,
    ) -> TextCursor {
        if self.layout_op.is_none() || matches!(cursor, TextCursor::Empty | TextCursor::None) {
            return TextCursor::None;
        }

        // Note: Since it is known that TextCursor isn't Empty.
        //       - default_font_height doesn't need to be valid.
        //       - tlwh can be all zeros.
        let ([min_x, max_x, _, _], line_i) =
            match self.get_cursor_bounds(cursor, [0.0; 4], text_body, UnitValue::Pixels(0.0)) {
                Some(some) => some,
                None => return TextCursor::None,
            };

        let cursor_x = ((max_x - min_x) / 2.0) + min_x;

        let line_i: usize = match (line_i as isize + line_offset).try_into() {
            Ok(ok) => ok,
            Err(_) => return TextCursor::None,
        };

        let layout = self.layout_op.as_ref().unwrap();
        Self::get_cursor_on_line(layout, line_i, cursor_x)
    }

    pub fn get_cursor_bounds(
        &self,
        cursor: TextCursor,
        tlwh: [f32; 4],
        text_body: &TextBody,
        default_font_height: UnitValue,
    ) -> Option<([f32; 4], usize)> {
        if cursor == TextCursor::None {
            return None;
        }

        if self.layout_op.is_none() {
            let text_height = match text_body.base_attrs.height {
                UnitValue::Undefined => default_font_height,
                body_height => body_height,
            }
            .px_height([tlwh[2], tlwh[3]])
            .unwrap();

            let line_height = match text_body.line_spacing {
                LineSpacing::HeightMult(mult) => text_height * mult,
                LineSpacing::HeightMultAdd(mult, add) => (text_height * mult) + add,
            };

            let [t, b] = match text_body.vert_align {
                TextVertAlign::Top => [0.0, line_height],
                TextVertAlign::Center => {
                    let center = tlwh[3] / 2.0;
                    let half_height = line_height / 2.0;
                    [center - half_height, center + half_height]
                },
                TextVertAlign::Bottom => [tlwh[3] - line_height, tlwh[3]],
            };

            let [l, r] = match text_body.hori_align {
                TextHoriAlign::Left => [0.0, 1.0],
                TextHoriAlign::Center => {
                    let center = tlwh[2] / 2.0;
                    [center - 0.5, center + 0.5]
                },
                TextHoriAlign::Right => [tlwh[2] - 1.0, tlwh[2]],
            };

            return Some(([l + tlwh[1], r + tlwh[1], t + tlwh[0], b + tlwh[0]], 0));
        }

        let layout = match self.layout_op.as_ref() {
            Some(layout) => layout,
            None => return None,
        };

        let cursor = match cursor {
            TextCursor::None => unreachable!(),
            TextCursor::Empty => {
                match text_body.cursor_next(TextCursor::Empty) {
                    TextCursor::None | TextCursor::Empty => return None,
                    TextCursor::Position(cursor) => cursor,
                }
            },
            TextCursor::Position(cursor) => cursor,
        };

        for (line_i, line) in layout.lines.iter().enumerate() {
            if cursor > line.e_cursor {
                // The cursor is past this line.
                continue;
            }

            if cursor < line.s_cursor {
                // The cursor is before this line?
                break;
            }

            if line.glyphs.is_empty() {
                // This line has no glyphs, so use the line's hitbox.

                let t = tlwh[0] + line.hitbox[2];
                let b = tlwh[0] + line.hitbox[3];
                let l = tlwh[1] + line.hitbox[0];
                let r = l + 1.0;
                return Some(([l, r, t, b], line_i));
            }

            let first_glyph = &layout.glyphs[line.glyphs.start];

            if match first_glyph.span_i.cmp(&cursor.span) {
                Ordering::Less => false,
                Ordering::Equal => {
                    match first_glyph.byte_s.cmp(&cursor.byte_s) {
                        Ordering::Less => false,
                        Ordering::Equal => false,
                        Ordering::Greater => true,
                    }
                },
                Ordering::Greater => true,
            } {
                // Cursor is before the first glyph, use the left side of the line's bounding box.

                let t = tlwh[0] + line.hitbox[2];
                let b = tlwh[0] + line.hitbox[3];
                let l = tlwh[1] + line.hitbox[0];
                let r = l + 1.0;
                return Some(([l, r, t, b], line_i));
            }

            let last_glyph = &layout.glyphs[line.glyphs.end - 1];

            if match last_glyph.span_i.cmp(&cursor.span) {
                Ordering::Less => true,
                Ordering::Equal => {
                    match last_glyph.byte_s.cmp(&cursor.byte_s) {
                        Ordering::Less => true,
                        Ordering::Equal => false,
                        Ordering::Greater => false,
                    }
                },
                Ordering::Greater => false,
            } {
                // Cursor is after the last glyph, use the right side of the line's bounding box.

                let t = tlwh[0] + line.hitbox[2];
                let b = tlwh[0] + line.hitbox[3];
                let r = tlwh[1] + line.hitbox[1];
                let l = r - 1.0;
                return Some(([l, r, t, b], line_i));
            }

            // The cursor *should* have an associated glyph.

            for glyph in layout.glyphs[line.glyphs.clone()].iter() {
                if glyph.span_i == cursor.span && glyph.byte_s == cursor.byte_s {
                    match cursor.affinity {
                        TextCursorAffinity::Before => {
                            let t = tlwh[0] + glyph.hitbox[2];
                            let b = tlwh[0] + glyph.hitbox[3];
                            let r = tlwh[1] + glyph.hitbox[0];
                            let l = r - 1.0;
                            return Some(([l, r, t, b], line_i));
                        },
                        TextCursorAffinity::After => {
                            let t = tlwh[0] + glyph.hitbox[2];
                            let b = tlwh[0] + glyph.hitbox[3];
                            let l = tlwh[1] + glyph.hitbox[1];
                            let r = l - 1.0;
                            return Some(([l, r, t, b], line_i));
                        },
                    };
                }
            }
        }

        // The cursor is probably invalid.

        None
    }

    pub fn select_line(&self, cursor: TextCursor) -> Option<TextSelection> {
        let cursor = match cursor {
            TextCursor::None | TextCursor::Empty => return None,
            TextCursor::Position(cursor) => cursor,
        };

        let layout = match self.layout_op.as_ref() {
            Some(layout) => layout,
            None => return None,
        };

        for line in layout.lines.iter() {
            if cursor > line.e_cursor {
                continue;
            }

            if cursor < line.s_cursor {
                break;
            }

            return Some(TextSelection {
                start: line.s_cursor,
                end: line.e_cursor,
            });
        }

        None
    }

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

        let current_size = [tlwh[2], tlwh[3]];

        if current_size != self.layout_size || context.scale != self.layout_scale {
            self.layout_size = current_size;
            self.layout_scale = context.scale;
            self.layout_valid = false;
        }

        if self.layout_valid {
            'validity: {
                if self.layout_op.is_none() {
                    self.layout_valid = false;
                    break 'validity;
                }

                let layout = self.layout_op.as_ref().unwrap();

                if body.line_limit != layout.line_limit
                    || body.text_wrap != layout.text_wrap
                    || body.vert_align != layout.vert_align
                    || body.hori_align != layout.hori_align
                {
                    self.layout_valid = false;
                    break 'validity;
                }

                if body.spans.len() != layout.spans.len() {
                    self.layout_valid = false;
                    break 'validity;
                }

                for (body_span, layout_span) in body.spans.iter().zip(layout.spans.iter()) {
                    if body_span.attrs.secret != layout_span.text_secret {
                        self.layout_valid = false;
                        break 'validity;
                    }

                    if body_span.attrs.secret {
                        if body_span.text.len() != layout_span.text.len() {
                            self.layout_valid = false;
                            break 'validity;
                        }
                    } else {
                        if body_span.text != layout_span.text {
                            self.layout_valid = false;
                            break 'validity;
                        }
                    }

                    let text_color = if body_span.attrs.color.a == 0.0 {
                        body.base_attrs.color
                    } else {
                        body_span.attrs.color
                    };

                    if text_color != layout_span.text_color {
                        self.layout_valid = false;
                        break 'validity;
                    }

                    let text_height = match body_span.attrs.height {
                        UnitValue::Undefined => {
                            match body.base_attrs.height {
                                UnitValue::Undefined => context.default_font.height,
                                body_height => body_height,
                            }
                        },
                        span_height => span_height,
                    }
                    .px_height(self.layout_size)
                    .unwrap();

                    if text_height != layout_span.text_height {
                        self.layout_valid = false;
                        break 'validity;
                    }

                    let text_secret = if body.base_attrs.secret {
                        true
                    } else {
                        body_span.attrs.secret
                    };

                    if text_secret != layout_span.text_secret {
                        self.layout_valid = false;
                        break 'validity;
                    }

                    let line_height = match body.line_spacing {
                        LineSpacing::HeightMult(mult) => text_height * mult,
                        LineSpacing::HeightMultAdd(mult, add) => (text_height * mult) + add,
                    };

                    if line_height != layout_span.line_height {
                        self.layout_valid = false;
                        break 'validity;
                    }

                    let font_family = match &body_span.attrs.font_family {
                        FontFamily::Inheirt => {
                            match &body.base_attrs.font_family {
                                FontFamily::Inheirt => &context.default_font.family,
                                base_family => base_family,
                            }
                        },
                        span_family => span_family,
                    };

                    if *font_family != layout_span.font_family {
                        self.layout_valid = false;
                        break 'validity;
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
                        self.layout_valid = false;
                        break 'validity;
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
                        self.layout_valid = false;
                        break 'validity;
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
                        self.layout_valid = false;
                        break 'validity;
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
                        UnitValue::Undefined => {
                            match body.base_attrs.height {
                                UnitValue::Undefined => context.default_font.height,
                                body_height => body_height,
                            }
                        },
                        span_height => span_height,
                    }
                    .px_height(self.layout_size)
                    .unwrap();

                    let text_color = if span.attrs.color.a == 0.0 {
                        body.base_attrs.color
                    } else {
                        span.attrs.color
                    };

                    let text_secret = if body.base_attrs.secret {
                        true
                    } else {
                        span.attrs.secret
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
                        text_color,
                        text_height,
                        text_secret,
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

        buffer.shape_until_scroll(&mut context.font_system, false);
        let mut layout_glyphs = Vec::new();
        let mut layout_lines: Vec<LayoutLine> = Vec::new();
        let mut line_byte_mapping: Vec<BTreeMap<usize, [usize; 3]>> = Vec::new();
        let mut line_byte = 0;

        for (span_i, span) in layout.spans.iter().enumerate() {
            for (byte_i, c) in span.text.char_indices() {
                if line_byte_mapping.is_empty() {
                    line_byte_mapping.push(BTreeMap::new());
                }

                line_byte_mapping
                    .last_mut()
                    .unwrap()
                    .insert(line_byte, [span_i, byte_i, byte_i + c.len_utf8()]);

                if c == '\n' {
                    line_byte_mapping.push(BTreeMap::new());
                    line_byte = 0;
                } else {
                    line_byte += c.len_utf8();
                }
            }
        }

        // FIXME: There seems to be a bug with cosmic-text where an empty line will not output
        //        following a '\n' if it is the last line.
        assert!(line_byte_mapping.len() >= buffer.lines.len());

        let mut line_top = 0.0;

        for (buffer_i, buffer_line) in buffer.lines.iter().enumerate() {
            let ct_layout_lines = buffer_line.layout_opt().unwrap();

            for (layout_line_i, layout_line) in ct_layout_lines.iter().enumerate() {
                if layout_line.glyphs.is_empty() {
                    // If the layout line is empty then there should only be one layout line in
                    // this buffer line.
                    debug_assert_eq!(ct_layout_lines.len(), 1);

                    if line_byte_mapping[buffer_i].is_empty() {
                        // This is the last line in the body and it is empty and there should be
                        // at least one buffer line before this one.
                        debug_assert_eq!(buffer_i, buffer.lines.len() - 1);

                        let [span, byte_s, byte_e] =
                            *line_byte_mapping[buffer_i - 1].last_entry().unwrap().get();

                        let se_cursor = PosTextCursor {
                            span,
                            byte_s,
                            byte_e,
                            affinity: TextCursorAffinity::After,
                        };

                        layout_lines.push(LayoutLine {
                            height: layout.spans[se_cursor.span].line_height,
                            glyphs: 0..0,
                            bounds: [0.0; 4],
                            hitbox: [0.0; 4],
                            s_cursor: se_cursor,
                            e_cursor: se_cursor,
                        });
                    } else {
                        // This line only has a '\n'
                        debug_assert_eq!(line_byte_mapping[buffer_i].len(), 1);

                        let [span, byte_s, byte_e] =
                            *line_byte_mapping[buffer_i].last_entry().unwrap().get();

                        let e_cursor = PosTextCursor {
                            span,
                            byte_s,
                            byte_e,
                            affinity: TextCursorAffinity::Before,
                        };

                        let s_cursor = if buffer_i > 0 {
                            // There is a buffer before this line use that as the start cursor.

                            let [span, byte_s, byte_e] =
                                *line_byte_mapping[buffer_i - 1].last_entry().unwrap().get();

                            PosTextCursor {
                                span,
                                byte_s,
                                byte_e,
                                affinity: TextCursorAffinity::After,
                            }
                        } else {
                            e_cursor
                        };

                        layout_lines.push(LayoutLine {
                            height: layout.spans[e_cursor.span].line_height,
                            glyphs: 0..0,
                            bounds: [0.0; 4],
                            hitbox: [0.0; 4],
                            s_cursor,
                            e_cursor,
                        });
                    }

                    line_top += layout_lines.last().unwrap().height;
                    continue;
                }

                debug_assert!(!layout_line.glyphs.is_empty());

                let first_glyph_i = layout_glyphs.len();

                for l_glyph in layout_line.glyphs.iter() {
                    let g_span_i = l_glyph.metadata;
                    let p_glyph = l_glyph.physical((0.0, 0.0), self.layout_scale);
                    let g_byte_s = line_byte_mapping[buffer_i][&l_glyph.start][1];
                    let g_byte_e = g_byte_s + (l_glyph.end - l_glyph.start);

                    layout_glyphs.push(LayoutGlyph {
                        span_i: g_span_i,
                        byte_s: g_byte_s,
                        byte_e: g_byte_e,
                        offset: [
                            p_glyph.x as f32 / self.layout_scale,
                            p_glyph.y as f32 / self.layout_scale,
                        ],
                        extent: [0.0; 2],
                        image_extent: [0.0; 2],
                        hitbox: [
                            l_glyph.x,
                            l_glyph.x + l_glyph.w,
                            l_glyph.y + line_top,
                            l_glyph.y + line_top,
                        ],
                        image_key: ImageKey::glyph(p_glyph.cache_key),
                        vertex_type: 0,
                    });
                }

                let last_glyph_i = layout_glyphs.len() - 1;

                let s_cursor = if layout_line_i == 0 {
                    // This is the first layout line in this buffer line, so include
                    // the previous '\n' as the start cursor.

                    if buffer_i == 0 {
                        // Use the start of this line as the start.

                        let [span, byte_s, byte_e] =
                            *line_byte_mapping[buffer_i].first_entry().unwrap().get();

                        PosTextCursor {
                            span,
                            byte_s,
                            byte_e,
                            affinity: TextCursorAffinity::Before,
                        }
                    } else {
                        // There is one buffer line before this one use that one's '\n' as
                        // the start of this line.
                        let [span, byte_s, byte_e] =
                            *line_byte_mapping[buffer_i - 1].last_entry().unwrap().get();

                        PosTextCursor {
                            span,
                            byte_s,
                            byte_e,
                            affinity: TextCursorAffinity::After,
                        }
                    }
                } else {
                    // This is not the first layout line in this buffer line, so it wrapped
                    // on a character or whitespace.

                    let last_line = layout_lines.last_mut().unwrap();
                    let first_glyph = &layout_glyphs[first_glyph_i];

                    if last_line.e_cursor.span == first_glyph.span_i {
                        // The two points are within the same span, this is easier...

                        if last_line.e_cursor.byte_e == first_glyph.byte_s {
                            // Wrapped on a character, use this line's first glyph

                            PosTextCursor {
                                span: first_glyph.span_i,
                                byte_s: first_glyph.byte_s,
                                byte_e: first_glyph.byte_e,
                                affinity: TextCursorAffinity::Before,
                            }
                        } else {
                            // Wrapped on whitespace, modify last line's cursor to include this
                            // white space and use that as the line ends.

                            // TODO: This assumes the byte range between the two points is a single
                            //       whitespace character. Could it be possible that it is multiple?

                            last_line.e_cursor = PosTextCursor {
                                span: first_glyph.span_i,
                                byte_s: last_line.e_cursor.byte_e,
                                byte_e: first_glyph.byte_s,
                                affinity: TextCursorAffinity::Before,
                            };

                            PosTextCursor {
                                affinity: TextCursorAffinity::After,
                                ..last_line.e_cursor
                            }
                        }
                    } else {
                        // The two points go across span boundries, this is trickier...

                        if last_line.e_cursor.byte_e
                            == body.spans[last_line.e_cursor.span].text.len()
                        {
                            // The last line included the entirety of the last span.

                            if first_glyph.byte_s == 0 {
                                // The first glyph starts at zero, so we wrapped on a character.
                                // Use this line's first glyph.

                                PosTextCursor {
                                    span: first_glyph.span_i,
                                    byte_s: first_glyph.byte_s,
                                    byte_e: first_glyph.byte_e,
                                    affinity: TextCursorAffinity::Before,
                                }
                            } else {
                                // The first glyph starts doesn't start at zero, so the span that
                                // the first glyph resides in contains the whitespace. Modify the
                                // last line's cursor to include the whitespace and use it as the
                                // the line ends.

                                // TODO: See above TODO about the whitespace's validity.

                                last_line.e_cursor = PosTextCursor {
                                    span: first_glyph.span_i,
                                    byte_s: 0,
                                    byte_e: first_glyph.byte_s,
                                    affinity: TextCursorAffinity::Before,
                                };

                                PosTextCursor {
                                    affinity: TextCursorAffinity::After,
                                    ..last_line.e_cursor
                                }
                            }
                        } else {
                            // The last line doesn't include the entirety of the last span.

                            if first_glyph.byte_s == 0 {
                                // This line starts at the start of first glyph's span, the
                                // whitespace entirely resides in the previous span. Modify the
                                // last line's e_cursor to include the whitespace and use it as the
                                // start of this line.

                                last_line.e_cursor = PosTextCursor {
                                    byte_s: last_line.e_cursor.byte_e,
                                    byte_e: body.spans[last_line.e_cursor.span].text.len(),
                                    affinity: TextCursorAffinity::Before,
                                    ..last_line.e_cursor
                                };

                                PosTextCursor {
                                    affinity: TextCursorAffinity::After,
                                    ..last_line.e_cursor
                                }
                            } else {
                                // TODO: The whitespace is multiple characters? Is this possible?

                                // Modify the last line's e_cursor to include the start of this
                                // span's whitespace and use it as the start of the line.

                                last_line.e_cursor = PosTextCursor {
                                    span: first_glyph.span_i,
                                    byte_s: 0,
                                    byte_e: first_glyph.byte_s,
                                    affinity: TextCursorAffinity::Before,
                                };

                                PosTextCursor {
                                    affinity: TextCursorAffinity::After,
                                    ..last_line.e_cursor
                                }
                            }
                        }
                    }
                };

                let e_cursor = if layout_line_i + 1 == ct_layout_lines.len() {
                    // This is the last layout line in this buffer line.

                    let [span, byte_s, byte_e] =
                        *line_byte_mapping[buffer_i].last_entry().unwrap().get();
                    let last_glyph = &layout_glyphs[last_glyph_i];

                    if last_glyph.span_i == span && last_glyph.byte_s == byte_s {
                        // The last glyph is at the end of this buffer line's bytes. Use the last
                        // glyph as the end.

                        PosTextCursor {
                            span: last_glyph.span_i,
                            byte_s: last_glyph.byte_s,
                            byte_e: last_glyph.byte_e,
                            affinity: TextCursorAffinity::After,
                        }
                    } else {
                        // The last glyph isn't at the end of this buffer line's bytes. Use the end
                        // of the line's bytes.

                        PosTextCursor {
                            span,
                            byte_s,
                            byte_e,
                            affinity: TextCursorAffinity::Before,
                        }
                    }
                } else {
                    // This isn't the last layout line use the last glyph as the end for now.
                    // NOTE: the above s_cursor code modifies the e_cursor if need be.

                    let last_glyph = &layout_glyphs[last_glyph_i];

                    PosTextCursor {
                        span: last_glyph.span_i,
                        byte_s: last_glyph.byte_s,
                        byte_e: last_glyph.byte_e,
                        affinity: TextCursorAffinity::After,
                    }
                };

                let mut line_height: f32 = 0.0;

                for span_i in
                    layout_glyphs[first_glyph_i].span_i..=layout_glyphs[last_glyph_i].span_i
                {
                    line_height = line_height.max(layout.spans[span_i].line_height);
                }

                let line_offset = line_top
                    + ((line_height - (layout_line.max_ascent + layout_line.max_descent)) / 2.0)
                    + layout_line.max_ascent;

                for glyph_i in first_glyph_i..=last_glyph_i {
                    let glyph = &mut layout_glyphs[glyph_i];
                    glyph.offset[1] += line_offset;
                    glyph.hitbox[3] += line_height;
                }

                layout_lines.push(LayoutLine {
                    height: line_height,
                    glyphs: first_glyph_i..(last_glyph_i + 1),
                    bounds: [0.0; 4],
                    hitbox: [0.0; 4],
                    s_cursor,
                    e_cursor,
                });

                line_top += line_height;
            }
        }

        // FIXME: See above assert
        if line_byte_mapping.len() > buffer.lines.len() {
            let line_i = line_byte_mapping.len() - 2;
            let [span, byte_s, byte_e] = *line_byte_mapping[line_i].last_entry().unwrap().get();

            let cursor = PosTextCursor {
                span,
                byte_s,
                byte_e,
                affinity: TextCursorAffinity::After,
            };

            layout_lines.push(LayoutLine {
                height: layout.spans[span].line_height,
                glyphs: 0..0,
                bounds: [0.0; 4],
                hitbox: [0.0; 4],
                s_cursor: cursor,
                e_cursor: cursor,
            });
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

        let body_height = layout_lines.iter().map(|line| line.height).sum::<f32>();

        let vert_align_offset = match layout.vert_align {
            TextVertAlign::Top => 0.0,
            TextVertAlign::Center => (self.layout_size[1] - body_height) / 2.0,
            TextVertAlign::Bottom => self.layout_size[1] - body_height,
        };

        let mut bounds = [
            f32::INFINITY,
            f32::NEG_INFINITY,
            vert_align_offset,
            body_height + vert_align_offset,
        ];

        for glyph in layout_glyphs.iter_mut() {
            match self.image_info_cache.get(&glyph.image_key).unwrap() {
                Some(image_info) => {
                    let image_data = image_info.associated_data::<GlyphImageData>().unwrap();
                    glyph.offset[0] += image_data.placement_left as f32 / self.layout_scale;
                    glyph.offset[1] -= image_data.placement_top as f32 / self.layout_scale;
                    glyph.extent[0] = image_info.width as f32 / self.layout_scale;
                    glyph.extent[1] = image_info.height as f32 / self.layout_scale;
                    glyph.image_extent[0] = image_info.width as f32;
                    glyph.image_extent[1] = image_info.height as f32;
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

        let mut line_y_min = vert_align_offset;

        for line in layout_lines.iter_mut() {
            let mut line_x_mm = [f32::INFINITY, f32::NEG_INFINITY];
            let mut line_x_hb = [f32::INFINITY, f32::NEG_INFINITY];

            for glyph_i in line.glyphs.clone() {
                line_x_mm[0] = line_x_mm[0].min(layout_glyphs[glyph_i].offset[0]);
                line_x_mm[1] = line_x_mm[1]
                    .max(layout_glyphs[glyph_i].offset[0] + layout_glyphs[glyph_i].extent[0]);
                line_x_hb[0] = line_x_hb[0].min(layout_glyphs[glyph_i].hitbox[0]);
                line_x_hb[1] = line_x_hb[1].max(layout_glyphs[glyph_i].hitbox[1]);
            }

            if line.glyphs.is_empty() {
                line_x_mm = [0.0; 2];
                line_x_hb = [0.0; 2];
            }

            let line_width = line_x_mm[1] - line_x_mm[0];

            let hori_align_offset =
                match if layout.text_wrap == TextWrap::Shift && line_width > self.layout_size[0] {
                    TextHoriAlign::Right
                } else {
                    layout.hori_align
                } {
                    /*TextHoriAlign::Left => -line_x_mm[0],
                    TextHoriAlign::Center => -line_x_mm[0] + ((self.layout_size[0] - line_width) / 2.0),
                    TextHoriAlign::Right => line_x_mm[0] + self.layout_size[0] - line_width,*/
                    TextHoriAlign::Left => 0.0,
                    TextHoriAlign::Center => (self.layout_size[0] - line_width) / 2.0,
                    TextHoriAlign::Right => self.layout_size[0] - line_width,
                };

            line_x_mm[0] += hori_align_offset;
            line_x_mm[1] += hori_align_offset;
            line_x_hb[0] += hori_align_offset;
            line_x_hb[1] += hori_align_offset;

            bounds[0] = bounds[0].min(line_x_mm[0]);
            bounds[1] = bounds[1].max(line_x_mm[1]);

            for glyph_i in line.glyphs.clone() {
                layout_glyphs[glyph_i].offset[0] += hori_align_offset;
                layout_glyphs[glyph_i].hitbox[0] += hori_align_offset;
                layout_glyphs[glyph_i].hitbox[1] += hori_align_offset;
            }

            line.bounds = [
                line_x_mm[0],
                line_x_mm[1],
                line_y_min,
                line_y_min + line.height,
            ];

            line.hitbox = [
                line_x_hb[0],
                line_x_hb[1],
                line_y_min,
                line_y_min + line.height,
            ];

            line_y_min += line.height;
        }

        layout.lines = layout_lines;
        layout.glyphs = layout_glyphs;
        layout.bounds = bounds;
        self.layout_valid = true;
    }

    pub fn output_reserve(&mut self, output: &mut ImageMap<Vec<ItfVertInfo>>) {
        for (image_key, image_info_op) in self.image_info_cache.iter() {
            if !image_key.is_invalid() && image_info_op.is_some() {
                output.try_insert(image_key, Vec::new);
            }
        }
    }

    pub fn output_vertexes(
        &mut self,
        tlwh: [f32; 4],
        z: f32,
        opacity: f32,
        text_body: &TextBody,
        context: &UpdateContext,
        output: &mut ImageMap<Vec<ItfVertInfo>>,
    ) {
        let layout = match self.layout_op.as_ref() {
            Some(layout) => layout,
            None => {
                if let Some(([l, r, t, b], _)) = self.get_cursor_bounds(
                    text_body.cursor,
                    tlwh,
                    text_body,
                    context.default_font.height,
                ) {
                    output.try_insert_then(
                        &ImageKey::INVALID,
                        Vec::new,
                        |vertexes: &mut Vec<ItfVertInfo>| {
                            vertexes.extend(
                                [
                                    [r, t, z],
                                    [l, t, z],
                                    [l, b, z],
                                    [r, t, z],
                                    [l, b, z],
                                    [r, b, z],
                                ]
                                .into_iter()
                                .map(|position| {
                                    ItfVertInfo {
                                        position,
                                        coords: [0.0; 2],
                                        color: text_body.cursor_color.rgbaf_array(),
                                        ty: 0,
                                        tex_i: 0,
                                    }
                                }),
                            );
                        },
                    );
                }

                return;
            },
        };

        for glyph in layout.glyphs.iter() {
            if !glyph.image_key.is_invalid() {
                output.try_insert_then(
                    &glyph.image_key,
                    Vec::new,
                    |vertexes: &mut Vec<ItfVertInfo>| {
                        let t = ((tlwh[0] + glyph.offset[1]) * self.layout_scale).round()
                            / self.layout_scale;
                        let b = ((tlwh[0] + glyph.extent[1] + glyph.offset[1]) * self.layout_scale)
                            .round()
                            / self.layout_scale;
                        let l = ((tlwh[1] + glyph.offset[0]) * self.layout_scale).round()
                            / self.layout_scale;
                        let r = ((tlwh[1] + glyph.extent[0] + glyph.offset[0]) * self.layout_scale)
                            .round()
                            / self.layout_scale;

                        let mut color = layout.spans[glyph.span_i].text_color;
                        color.a *= opacity;
                        let color = color.rgbaf_array();

                        vertexes.extend(
                            [
                                ([r, t, z], [glyph.image_extent[0], 0.0]),
                                ([l, t, z], [0.0, 0.0]),
                                ([l, b, z], [0.0, glyph.image_extent[1]]),
                                ([r, t, z], [glyph.image_extent[0], 0.0]),
                                ([l, b, z], [0.0, glyph.image_extent[1]]),
                                ([r, b, z], glyph.image_extent),
                            ]
                            .into_iter()
                            .map(|(position, coords)| {
                                ItfVertInfo {
                                    position,
                                    coords,
                                    color,
                                    ty: glyph.vertex_type,
                                    tex_i: 0,
                                }
                            }),
                        );
                    },
                );
            }

            if let Some(selection) = text_body.selection.as_ref() {
                if glyph.span_i < selection.start.span || glyph.span_i > selection.end.span {
                    continue;
                }

                if glyph.span_i == selection.start.span {
                    if glyph.byte_s < selection.start.byte_s {
                        continue;
                    }

                    if glyph.byte_s == selection.start.byte_s
                        && selection.start.affinity == TextCursorAffinity::After
                    {
                        continue;
                    }
                }

                if glyph.span_i == selection.end.span {
                    if glyph.byte_s > selection.end.byte_s {
                        continue;
                    }

                    if glyph.byte_s == selection.end.byte_s
                        && selection.end.affinity == TextCursorAffinity::Before
                    {
                        continue;
                    }
                }

                output.try_insert_then(
                    &ImageKey::INVALID,
                    Vec::new,
                    |vertexes: &mut Vec<ItfVertInfo>| {
                        let t = tlwh[0] + glyph.hitbox[2];
                        let b = tlwh[0] + glyph.hitbox[3];
                        let l = tlwh[1] + glyph.hitbox[0];
                        let r = tlwh[1] + glyph.hitbox[1];

                        vertexes.extend(
                            [
                                [r, t, z],
                                [l, t, z],
                                [l, b, z],
                                [r, t, z],
                                [l, b, z],
                                [r, b, z],
                            ]
                            .into_iter()
                            .map(|position| {
                                ItfVertInfo {
                                    position,
                                    coords: [0.0; 2],
                                    color: text_body.selection_color.rgbaf_array(),
                                    ty: 0,
                                    tex_i: 0,
                                }
                            }),
                        );
                    },
                );
            }
        }

        if let Some(selection) = text_body.selection.as_ref() {
            for (line_i, line) in layout.lines.iter().enumerate() {
                if line_i + 1 == layout.lines.len()
                    || layout.lines[line_i + 1].s_cursor <= selection.start
                    || layout.lines[line_i + 1].s_cursor > selection.end
                {
                    continue;
                }

                let t = tlwh[0] + line.hitbox[2];
                let b = tlwh[0] + line.hitbox[3];
                let l = tlwh[1] + line.hitbox[1];
                let r = tlwh[1] + line.hitbox[1] + (line.height / 4.0).round();

                output.try_insert_then(
                    &ImageKey::INVALID,
                    Vec::new,
                    |vertexes: &mut Vec<ItfVertInfo>| {
                        vertexes.extend(
                            [
                                [r, t, z],
                                [l, t, z],
                                [l, b, z],
                                [r, t, z],
                                [l, b, z],
                                [r, b, z],
                            ]
                            .into_iter()
                            .map(|position| {
                                ItfVertInfo {
                                    position,
                                    coords: [0.0; 2],
                                    color: text_body.selection_color.rgbaf_array(),
                                    ty: 0,
                                    tex_i: 0,
                                }
                            }),
                        );
                    },
                );
            }
        }

        if text_body.selection.is_none() {
            if let Some(([l, r, t, b], _)) = self.get_cursor_bounds(
                text_body.cursor,
                tlwh,
                text_body,
                context.default_font.height,
            ) {
                output.try_insert_then(
                    &ImageKey::INVALID,
                    Vec::new,
                    |vertexes: &mut Vec<ItfVertInfo>| {
                        vertexes.extend(
                            [
                                [r, t, z],
                                [l, t, z],
                                [l, b, z],
                                [r, t, z],
                                [l, b, z],
                                [r, b, z],
                            ]
                            .into_iter()
                            .map(|position| {
                                ItfVertInfo {
                                    position,
                                    coords: [0.0; 2],
                                    color: text_body.cursor_color.rgbaf_array(),
                                    ty: 0,
                                    tex_i: 0,
                                }
                            }),
                        );
                    },
                );
            }
        }
    }

    pub fn bounds(&self, tlwh: [f32; 4]) -> Option<[f32; 4]> {
        self.layout_op.as_ref().map(|layout| {
            [
                tlwh[1] + layout.bounds[0],
                tlwh[1] + layout.bounds[1],
                tlwh[0] + layout.bounds[2],
                tlwh[0] + layout.bounds[3],
            ]
        })
    }
}
