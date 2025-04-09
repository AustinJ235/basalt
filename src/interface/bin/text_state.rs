use std::hash::BuildHasher;
use std::sync::Arc;

use cosmic_text as ct;

use crate::image::{ImageCache, ImageData, ImageFormat, ImageInfo, ImageKey, ImageMap, ImageSet};
use crate::interface::bin::{ImageCacheLifetime, UpdateContext};
use crate::interface::{BinStyle, Color, ItfVertInfo, TextHoriAlign, TextVertAlign, TextWrap};
use crate::ulps_eq;

#[derive(Debug, Clone, Default)]
pub struct TextState {
    inner_op: Option<Inner>,
}

#[derive(Clone, Debug)]
struct Inner {
    hash: u64,
    z_index: f32,
    buffer_width: Option<f32>,
    metrics: ct::Metrics,
    attrs: ct::AttrsOwned,
    wrap: TextWrap,
    vert_align: TextVertAlign,
    hori_align: TextHoriAlign,
    buffer: ct::Buffer,
    update_layout: bool,
    update_vertexes: bool,
    layout_tlwh: [f32; 4],
    glyph_infos: Vec<GlyphInfo>,
    image_keys: Vec<ImageKey>,
    image_info: ImageMap<Option<ImageInfo>>,
    vertex_tlwh: [f32; 4],
    vertex_data: ImageMap<Vec<ItfVertInfo>>,
}

#[derive(Debug, Clone)]
struct GlyphInfo {
    cache_key: Option<ImageKey>,
    tlwh: [f32; 4],
    image_dim: [u32; 2],
    vertex_type: Option<i32>,
    color: Color,
}

struct GlyphImageAssociatedData {
    vertex_type: i32,
    placement_top: i32,
    placement_left: i32,
}

impl TextState {
    pub fn bounds(&self) -> Option<[f32; 4]> {
        let inner = self.inner_op.as_ref()?;

        if inner.glyph_infos.is_empty() {
            return None;
        }

        let mut bounds = [f32::MAX, f32::MIN, f32::MAX, f32::MIN];

        for glyph_info in inner.glyph_infos.iter() {
            let t = glyph_info.tlwh[0] + inner.layout_tlwh[0];
            let l = glyph_info.tlwh[1] + inner.layout_tlwh[1];
            bounds[0] = bounds[0].min(l);
            bounds[1] = bounds[1].max(l + glyph_info.tlwh[2]);
            bounds[2] = bounds[2].min(t);
            bounds[3] = bounds[3].max(t + glyph_info.tlwh[3]);
        }

        if bounds == [f32::MAX, f32::MIN, f32::MAX, f32::MIN] {
            return None;
        }

        Some(bounds)
    }

    pub fn update_buffer(
        &mut self,
        tlwh: [f32; 4],
        z_index: f32,
        opacity: f32,
        style: &BinStyle,
        context: &mut UpdateContext,
    ) {
        if style.text.is_empty() {
            self.inner_op = None;
            return;
        }

        let text = if style.text_secret == Some(true) {
            (0..style.text.len()).map(|_| '*').collect::<String>()
        } else {
            style.text.clone()
        };

        let hash = foldhash::fast::FixedState::with_seed(0).hash_one(&text);
        let font_size = style.text_height.unwrap_or(12.0) * context.scale;

        let line_height = match style.line_spacing {
            Some(spacing) => font_size + (spacing * context.scale),
            None => font_size * 1.2,
        };

        let metrics = ct::Metrics {
            font_size,
            line_height,
        };

        let mut color = style.text_color.unwrap_or_else(|| Color::shex("000000"));

        color.a *= opacity;
        let [r, g, b, a] = color.srgba8_array();

        let attrs = ct::AttrsOwned {
            color_opt: Some(ct::Color::rgba(r, g, b, a)),
            family_owned: style
                .font_family
                .clone()
                .map(ct::FamilyOwned::Name)
                .unwrap_or(ct::FamilyOwned::SansSerif),
            stretch: style
                .font_stretch
                .unwrap_or_default()
                .into_cosmic()
                .unwrap(),
            style: style.font_style.unwrap_or_default().into_cosmic().unwrap(),
            weight: style.font_weight.unwrap_or_default().into_cosmic().unwrap(),
            metadata: 0,
            cache_key_flags: ct::CacheKeyFlags::empty(),
            metrics_opt: None,
        };

        let wrap = style.text_wrap.unwrap_or_default();
        let vert_align = style.text_vert_align.unwrap_or_default();
        let hori_align = style.text_hori_align.unwrap_or_default();

        let buffer_width = match wrap {
            TextWrap::Shift | TextWrap::None => None,
            _ => Some(tlwh[2] * context.scale),
        };

        if let Some(inner) = self.inner_op.as_mut() {
            let metrics_eq = inner.metrics == metrics;
            let buffer_width_eq = inner.buffer_width == buffer_width;
            let text_and_attrs_eq = inner.hash == hash && inner.attrs == attrs;

            if metrics_eq
                && buffer_width_eq
                && text_and_attrs_eq
                && wrap == inner.wrap
                && vert_align == inner.vert_align
                && hori_align == inner.hori_align
                && ulps_eq(z_index, inner.z_index, 4)
                && ulps_eq(inner.layout_tlwh[2], tlwh[2], 4)
                && ulps_eq(inner.layout_tlwh[3], tlwh[3], 4)
            {
                inner.layout_tlwh = tlwh;
                return;
            } else {
                inner.update_layout = true;
            }

            if !metrics_eq {
                inner.metrics = metrics;

                if !text_and_attrs_eq {
                    inner.buffer.set_text(
                        &mut context.font_system,
                        "",
                        attrs.as_attrs(),
                        ct::Shaping::Advanced,
                    );
                }

                if !buffer_width_eq {
                    inner.buffer_width = buffer_width;

                    inner.buffer.set_metrics_and_size(
                        &mut context.font_system,
                        metrics,
                        buffer_width,
                        None,
                    );
                } else {
                    inner.buffer.set_metrics(&mut context.font_system, metrics)
                }
            } else if !buffer_width_eq {
                inner.buffer_width = buffer_width;

                if !text_and_attrs_eq {
                    inner.buffer.set_text(
                        &mut context.font_system,
                        "",
                        attrs.as_attrs(),
                        ct::Shaping::Advanced,
                    );
                }

                inner
                    .buffer
                    .set_size(&mut context.font_system, buffer_width, None);
            }

            if !text_and_attrs_eq {
                inner.hash = hash;
                inner.attrs = attrs;

                inner.buffer.set_text(
                    &mut context.font_system,
                    text.as_str(),
                    inner.attrs.as_attrs(),
                    ct::Shaping::Advanced,
                );
            }

            inner.layout_tlwh = tlwh;
            inner.z_index = z_index;
            inner.wrap = wrap;
            inner.vert_align = vert_align;
            inner.hori_align = hori_align;
            return;
        }

        let mut buffer = ct::Buffer::new(&mut context.font_system, metrics);
        buffer.set_size(&mut context.font_system, buffer_width, None);

        buffer.set_text(
            &mut context.font_system,
            text.as_str(),
            attrs.as_attrs(),
            ct::Shaping::Advanced,
        );

        self.inner_op = Some(Inner {
            hash,
            z_index,
            buffer_width,
            metrics,
            attrs,
            wrap,
            vert_align,
            hori_align,
            buffer,
            update_layout: true,
            update_vertexes: false,
            layout_tlwh: tlwh,
            glyph_infos: Vec::new(),
            image_keys: Vec::new(),
            image_info: ImageMap::new(),
            vertex_tlwh: tlwh,
            vertex_data: ImageMap::new(),
        });
    }

    pub fn update_layout(&mut self, context: &mut UpdateContext, image_cache: &Arc<ImageCache>) {
        if let Some(inner) = self.inner_op.as_mut() {
            if !inner.update_layout {
                return;
            }

            let mut min_line_y = None;
            let mut max_line_y = None;
            let mut image_keys = ImageSet::new();
            let mut glyph_infos = Vec::new();
            let scaled_layout_w = inner.layout_tlwh[2] * context.scale;
            let scaled_layout_h = inner.layout_tlwh[3] * context.scale;

            for run in inner.buffer.layout_runs() {
                if run.line_i == 0 {
                    min_line_y = Some(run.line_y - inner.metrics.font_size);
                }

                if max_line_y.is_none() || *max_line_y.as_ref().unwrap() < run.line_y {
                    max_line_y = Some(run.line_y);
                }

                let hori_align = if inner.wrap == TextWrap::Shift && run.line_w > scaled_layout_w {
                    TextHoriAlign::Right
                } else {
                    inner.hori_align
                };

                let hori_align_offset = match hori_align {
                    TextHoriAlign::Left => 0.0,
                    TextHoriAlign::Center => (scaled_layout_w - run.line_w) / 2.0,
                    TextHoriAlign::Right => scaled_layout_w - run.line_w,
                };

                for glyph in run.glyphs.iter() {
                    let color = glyph
                        .color_opt
                        .as_ref()
                        .map(|color| {
                            let [r, g, b, a] = color.as_rgba();
                            Color::srgba8(r, g, b, a)
                        })
                        .unwrap();

                    let glyph = glyph.physical((0.0, 0.0), 1.0);
                    let image_key = ImageKey::glyph(glyph.cache_key);
                    image_keys.insert(image_key.clone());

                    glyph_infos.push((
                        image_key,
                        color,
                        glyph.x as f32 + hori_align_offset,
                        run.line_y
                            - ((inner.metrics.line_height - inner.metrics.font_size) / 2.0).floor(),
                    ));
                }
            }

            if glyph_infos.is_empty() {
                inner.glyph_infos = Vec::new();
                inner.update_layout = false;
                inner.update_vertexes = true;
                return;
            }

            let mut prev_image_info = ImageMap::new();
            std::mem::swap(&mut prev_image_info, &mut inner.image_info);
            inner.image_keys.clear();
            let mut obtain_image_infos = Vec::new();

            for image_key in image_keys {
                match prev_image_info.remove(&image_key) {
                    Some(image_info_op) => {
                        if image_info_op.is_some() {
                            inner.image_keys.push(image_key.clone());
                        }

                        inner.image_info.insert(image_key, image_info_op);
                    },
                    None => {
                        obtain_image_infos.push(image_key);
                    },
                }
            }

            if !obtain_image_infos.is_empty() {
                for (image_info_op, image_key) in image_cache
                    .obtain_image_infos(obtain_image_infos.iter())
                    .into_iter()
                    .zip(obtain_image_infos.clone())
                {
                    if let Some(image_info) = image_info_op {
                        inner.image_keys.push(image_key.clone());
                        inner.image_info.insert(image_key.clone(), Some(image_info));
                        continue;
                    }

                    let swash_cache_id = image_key.as_glyph().unwrap();

                    if let Some(swash_image) = context
                        .glyph_cache
                        .get_image_uncached(&mut context.font_system, *swash_cache_id)
                    {
                        if swash_image.placement.width == 0
                            || swash_image.placement.height == 0
                            || swash_image.data.is_empty()
                        {
                            inner.image_info.insert(image_key.clone(), None);
                            continue;
                        }

                        let (vertex_type, image_format): (i32, _) = match swash_image.content {
                            ct::SwashContent::Mask => (2, ImageFormat::LMono),
                            ct::SwashContent::SubpixelMask => (2, ImageFormat::LRGBA),
                            ct::SwashContent::Color => (100, ImageFormat::LRGBA),
                        };

                        let image_info = image_cache
                            .load_raw_image(
                                image_key.clone(),
                                ImageCacheLifetime::Indefinite,
                                image_format,
                                swash_image.placement.width,
                                swash_image.placement.height,
                                GlyphImageAssociatedData {
                                    vertex_type,
                                    placement_top: swash_image.placement.top,
                                    placement_left: swash_image.placement.left,
                                },
                                ImageData::D8(swash_image.data.into_iter().collect()),
                            )
                            .unwrap();

                        inner.image_keys.push(image_key.clone());
                        inner.image_info.insert(image_key.clone(), Some(image_info));
                    }
                }
            }

            let buffer_height = max_line_y.unwrap() - min_line_y.unwrap();
            let vert_align_offset = match inner.vert_align {
                TextVertAlign::Top => 0.0,
                TextVertAlign::Center => (scaled_layout_h - buffer_height) / 2.0,
                TextVertAlign::Bottom => scaled_layout_h - buffer_height,
            };

            inner.glyph_infos = glyph_infos
                .into_iter()
                .map(|(image_key, color, mut glyph_x, mut glyph_y)| {
                    match inner.image_info.get(&image_key) {
                        Some(Some(image_info)) => {
                            let associated_data = image_info
                                .associated_data::<GlyphImageAssociatedData>()
                                .unwrap();

                            let image_dim = [image_info.width, image_info.height];
                            glyph_y += vert_align_offset - associated_data.placement_top as f32;
                            glyph_x += associated_data.placement_left as f32;

                            let glyph_tlwh = [
                                glyph_y / context.scale,
                                glyph_x / context.scale,
                                image_dim[0] as f32 / context.scale,
                                image_dim[1] as f32 / context.scale,
                            ];

                            GlyphInfo {
                                cache_key: Some(image_key),
                                tlwh: glyph_tlwh,
                                image_dim,
                                vertex_type: Some(associated_data.vertex_type),
                                color,
                            }
                        },
                        _ => {
                            GlyphInfo {
                                cache_key: None,
                                tlwh: [glyph_y / context.scale, glyph_x / context.scale, 0.0, 0.0],
                                image_dim: [0; 2],
                                vertex_type: None,
                                color,
                            }
                        },
                    }
                })
                .collect();

            inner.update_layout = false;
            inner.update_vertexes = true;
        }
    }

    pub fn nonvisible_vertex_data(&self, output: &mut ImageMap<Vec<ItfVertInfo>>) {
        if let Some(inner) = self.inner_op.as_ref() {
            output.reserve(inner.image_keys.len());
            output.extend(
                inner
                    .image_keys
                    .iter()
                    .map(|image_key| (image_key.clone(), Vec::new())),
            );
        }
    }

    pub fn update_vertexes(&mut self, output_op: Option<&mut ImageMap<Vec<ItfVertInfo>>>) {
        if let Some(inner) = self.inner_op.as_mut() {
            if !inner.update_vertexes {
                if ulps_eq(inner.vertex_tlwh[0], inner.layout_tlwh[0], 4)
                    && ulps_eq(inner.vertex_tlwh[1], inner.layout_tlwh[1], 4)
                {
                    if let Some(output) = output_op {
                        output.reserve(inner.vertex_data.len());
                        output.extend(
                            inner
                                .vertex_data
                                .iter()
                                .map(|(image_key, vertexes)| (image_key.clone(), vertexes.clone())),
                        );
                    }
                } else {
                    let translate_x = inner.layout_tlwh[1] - inner.vertex_tlwh[1];
                    let translate_y = inner.layout_tlwh[0] - inner.vertex_tlwh[0];

                    match output_op {
                        Some(output) => {
                            output.extend(inner.vertex_data.iter_mut().map(
                                |(image_key, vertexes)| {
                                    vertexes.iter_mut().for_each(|vertex| {
                                        vertex.position[0] += translate_x;
                                        vertex.position[1] += translate_y;
                                    });

                                    (image_key.clone(), vertexes.clone())
                                },
                            ));
                        },
                        None => {
                            inner.vertex_data.values_mut().for_each(|vertexes| {
                                vertexes.iter_mut().for_each(|vertex| {
                                    vertex.position[0] += translate_x;
                                    vertex.position[1] += translate_y;
                                });
                            });
                        },
                    }

                    inner.vertex_tlwh = inner.layout_tlwh;
                }
            } else {
                let mut vertex_data = ImageMap::new();
                let z = inner.z_index;

                for image_key in inner.image_keys.iter().cloned() {
                    vertex_data.insert(image_key, Vec::new());
                }

                for glyph_info in inner.glyph_infos.iter() {
                    if let (Some(image_key), Some(ty)) =
                        (glyph_info.cache_key.as_ref(), glyph_info.vertex_type)
                    {
                        let t = [glyph_info.tlwh[0] + inner.layout_tlwh[0], 0.0];
                        let l = [glyph_info.tlwh[1] + inner.layout_tlwh[1], 0.0];
                        let b = [t[0] + glyph_info.tlwh[3], glyph_info.image_dim[1] as f32];
                        let r = [l[0] + glyph_info.tlwh[2], glyph_info.image_dim[0] as f32];
                        let color = glyph_info.color.rgbaf_array();

                        vertex_data.get_mut(image_key).unwrap().append(&mut vec![
                            ItfVertInfo {
                                position: [r[0], t[0], z],
                                coords: [r[1], t[1]],
                                color,
                                ty,
                                tex_i: 0,
                            },
                            ItfVertInfo {
                                position: [l[0], t[0], z],
                                coords: [l[1], t[1]],
                                color,
                                ty,
                                tex_i: 0,
                            },
                            ItfVertInfo {
                                position: [l[0], b[0], z],
                                coords: [l[1], b[1]],
                                color,
                                ty,
                                tex_i: 0,
                            },
                            ItfVertInfo {
                                position: [r[0], t[0], z],
                                coords: [r[1], t[1]],
                                color,
                                ty,
                                tex_i: 0,
                            },
                            ItfVertInfo {
                                position: [l[0], b[0], z],
                                coords: [l[1], b[1]],
                                color,
                                ty,
                                tex_i: 0,
                            },
                            ItfVertInfo {
                                position: [r[0], b[0], z],
                                coords: [r[1], b[1]],
                                color,
                                ty,
                                tex_i: 0,
                            },
                        ]);
                    }
                }

                inner.vertex_data = vertex_data;
                inner.update_vertexes = false;
                inner.vertex_tlwh = inner.layout_tlwh;

                if let Some(output) = output_op {
                    output.reserve(inner.vertex_data.len());
                    output.extend(
                        inner
                            .vertex_data
                            .iter()
                            .map(|(image_key, vertexes)| (image_key.clone(), vertexes.clone())),
                    );
                }
            }
        }
    }
}
