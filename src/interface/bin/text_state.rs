use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use cosmic_text as ct;

use crate::image_cache::{ImageCache, ImageCacheKey};
use crate::interface::{BinStyle, ItfVertInfo, TextHoriAlign, TextVertAlign, TextWrap};
use crate::render::{ImageSource, UpdateContext};

#[derive(Default)]
pub struct TextState {
    inner_op: Option<Inner>,
}

struct Inner {
    hash: u64,
    tlwh: [f32; 4],
    buffer_width: f32,
    metrics: ct::Metrics,
    attrs: ct::AttrsOwned,
    wrap: TextWrap,
    vert_align: TextVertAlign,
    hori_align: TextHoriAlign,
    buffer: ct::Buffer,
    update_vertexes: bool,
    vertex_data: HashMap<ImageCacheKey, Vec<ItfVertInfo>>,
}

impl TextState {
    pub fn update_buffer(&mut self, tlwh: [f32; 4], style: &BinStyle, context: &mut UpdateContext) {
        if style.text.is_empty() {
            self.inner_op = None;
            return;
        }

        let text = if style.text_secret == Some(true) {
            (0..style.text.len()).map(|_| '*').collect::<String>()
        } else {
            style.text.clone()
        };

        let hash = {
            let mut hasher = DefaultHasher::new();
            text.hash(&mut hasher);
            hasher.finish()
        };

        let font_size = style.text_height.unwrap_or(12.0) * context.scale;
        let line_height = match style.line_spacing {
            Some(spacing) => font_size + (spacing * context.scale),
            None => font_size * 1.2,
        };

        let metrics = ct::Metrics {
            font_size,
            line_height,
        };

        let attrs = ct::AttrsOwned {
            color_opt: None,
            family_owned: style
                .font_family
                .clone()
                .map(|family| ct::FamilyOwned::Name(family))
                .unwrap_or(ct::FamilyOwned::SansSerif),
            stretch: style.font_stretch.unwrap_or_default().into(),
            style: style.font_style.unwrap_or_default().into(),
            weight: style.font_weight.unwrap_or_default().into(),
            metadata: 0,
            cache_key_flags: ct::CacheKeyFlags::empty(),
        };

        let wrap = style.text_wrap.unwrap_or_default();
        let vert_align = style.text_vert_align.unwrap_or_default();
        let hori_align = style.text_hori_align.unwrap_or_default();

        let buffer_width = matches!(wrap, TextWrap::Shift | TextWrap::None)
            .then_some(f32::MAX)
            .unwrap_or_else(|| tlwh[2] * context.scale);

        if let Some(inner) = self.inner_op.as_mut() {
            let metrics_eq = inner.metrics == metrics;
            let buffer_width_eq = inner.buffer_width == buffer_width;
            let text_and_attrs_eq = inner.hash == hash && inner.attrs == attrs;

            if metrics_eq
                && buffer_width_eq
                && text_and_attrs_eq
                && wrap == inner.wrap
                && inner.vert_align == inner.vert_align
                && hori_align == inner.hori_align
            {
                return;
            } else {
                inner.update_vertexes = true;
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
                        f32::MAX,
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
                    .set_size(&mut context.font_system, buffer_width, f32::MAX);
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

            inner.tlwh = tlwh;
            inner.wrap = wrap;
            inner.vert_align = vert_align;
            inner.hori_align = hori_align;
            return;
        }

        let mut buffer = ct::Buffer::new(&mut context.font_system, metrics);
        buffer.set_size(&mut context.font_system, buffer_width, f32::MAX);

        buffer.set_text(
            &mut context.font_system,
            text.as_str(),
            attrs.as_attrs(),
            ct::Shaping::Advanced,
        );

        self.inner_op = Some(Inner {
            hash,
            tlwh,
            buffer_width,
            metrics,
            attrs,
            wrap,
            vert_align,
            hori_align,
            buffer,
            update_vertexes: true,
            vertex_data: HashMap::new(),
        });
    }

    pub fn update_vertexes(
        &mut self,
        tlwh: [f32; 4],
        context: &mut UpdateContext,
        image_cache: &Arc<ImageCache>,
        output_op: Option<&mut HashMap<ImageSource, Vec<ItfVertInfo>>>,
    ) {
        if let Some(inner) = self.inner_op.as_mut() {
            if !inner.update_vertexes {
                if ulps_eq(inner.tlwh[0], tlwh[0], 4) && ulps_eq(inner.tlwh[1], tlwh[1], 4) {
                    if let Some(output) = output_op {
                        output.extend(inner.vertex_data.clone().into_iter().map(
                            |(image_cache_key, vertexes)| {
                                (ImageSource::Cache(image_cache_key), vertexes)
                            },
                        ));
                    }
                } else {
                    let translate_x = tlwh[1] - inner.tlwh[1];
                    let translate_y = tlwh[0] - inner.tlwh[0];

                    match output_op {
                        Some(output) => {
                            output.extend(inner.vertex_data.iter_mut().map(
                                |(image_cache_key, vertexes)| {
                                    vertexes.iter_mut().for_each(|vertex| {
                                        vertex.position[0] += translate_x;
                                        vertex.position[1] += translate_y;
                                    });

                                    (
                                        ImageSource::Cache(image_cache_key.clone()),
                                        vertexes.clone(),
                                    )
                                },
                            ));
                        },
                        None => {
                            inner
                                .vertex_data
                                .iter_mut()
                                .for_each(|(image_cache_key, vertexes)| {
                                    vertexes.iter_mut().for_each(|vertex| {
                                        vertex.position[0] += translate_x;
                                        vertex.position[1] += translate_y;
                                    });
                                });
                        },
                    }

                    inner.tlwh = tlwh;
                }
            } else {
                inner.update_vertexes = false;
                // TODO: Layout & Vertexes
            }
        }
    }
}

fn ulps_eq(a: f32, b: f32, tol: u32) -> bool {
    if a.is_nan() || b.is_nan() {
        false
    } else if a.is_sign_positive() != b.is_sign_positive() {
        a == b
    } else {
        let a_bits = a.to_bits();
        let b_bits = b.to_bits();
        let max = a_bits.max(b_bits);
        let min = a_bits.min(b_bits);
        (max - min) <= tol
    }
}
