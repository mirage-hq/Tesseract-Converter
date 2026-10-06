//! Font-derived classifier geometry. This copy is never emitted as render content.

use fx_schema::{
    GroupLayer, Justification, Layer, LayerData, PropType, RectLayer, RectShape, TextLayer,
};

use super::ArchiveFonts;
use crate::export_document::{AnimationIndex, hierarchy::Bounds};

impl ArchiveFonts {
    /// Keep native Text in the output; only the hierarchy's finite-source proof
    /// consumes these rectangles. Unqualified Text remains unknown, so existing
    /// source/consumer certificates still govern its enclosure. No host font,
    /// cached donor layout or text-box estimate participates in the proof.
    pub(in crate::export_document) fn bounds_geometry(
        &self,
        group: &GroupLayer,
        dynamics: &AnimationIndex<'_>,
    ) -> Result<GroupLayer, &'static str> {
        let (layers, has_outlines) = self.project_layers(&group.layers, dynamics)?;
        if !has_outlines {
            return Err("No Text has verified embedded-font outline bounds");
        }
        let mut projected = group.clone();
        projected.layers = layers;
        Ok(projected)
    }

    fn project_layers(
        &self,
        layers: &[Layer],
        dynamics: &AnimationIndex<'_>,
    ) -> Result<(Vec<Layer>, bool), &'static str> {
        let mut has_outlines = false;
        let projected = layers
            .iter()
            .map(|layer| match layer.data() {
                LayerData::Text(text) => {
                    // An unqualified sibling remains an unknown Text bound. It
                    // may use an independently certified consumer/source domain;
                    // it must not erase the verified outlines of other Text.
                    let Ok(painted_bounds) = self.point_bounds(text, dynamics) else {
                        return Ok(layer.clone());
                    };
                    has_outlines = true;
                    // Whitespace remains native editable Text in the emitted view.
                    // Its hidden classifier proxy contributes no painted enclosure.
                    let bounds = painted_bounds.unwrap_or(Bounds {
                        min: [0.0; 2],
                        max: [0.0; 2],
                    });
                    Layer::from_data(&LayerData::Rect(RectLayer {
                        id: text.id,
                        name: text.name.clone(),
                        description: text.description.clone(),
                        is_hidden: text.is_hidden || painted_bounds.is_none(),
                        parent: text.parent,
                        blend_mode: text.blend_mode,
                        track_matte: None,
                        masks: Vec::new(),
                        active_range: text.active_range,
                        effects: Vec::new(),
                        motion_blur: false,
                        transform: text.transform,
                        rect: RectShape {
                            size: std::array::from_fn(|axis| bounds.max[axis] - bounds.min[axis]),
                            position: bounds.min,
                            roundness: 0.0,
                            fill_enabled: true,
                            fill_color: [1.0; 4],
                            fill_paint: None,
                            fill_blend_mode: None,
                            stroke_enabled: false,
                            stroke_color: None,
                            stroke_width: Default::default(),
                            stroke_dashes: Vec::new(),
                            stroke_dash_offset: 0.0,
                            stroke_join: Default::default(),
                            stroke_miter_limit: 4.0,
                        },
                    }))
                    .map_err(|_| "Font-derived Text bounds could not be represented")
                }
                LayerData::Group(group) => {
                    let (layers, child_outlines) = self.project_layers(&group.layers, dynamics)?;
                    has_outlines |= child_outlines;
                    let mut projected = group.clone();
                    projected.layers = layers;
                    Layer::from_data(&LayerData::Group(projected))
                        .map_err(|_| "Font-derived Group bounds could not be represented")
                }
                _ => Ok(layer.clone()),
            })
            .collect::<Result<_, _>>()?;
        Ok((projected, has_outlines))
    }

    fn point_bounds(
        &self,
        text: &TextLayer,
        dynamics: &AnimationIndex<'_>,
    ) -> Result<Option<Bounds>, &'static str> {
        let document = &text.source_text;
        if document.box_text
            || document.font_variations.is_some()
            || document.underline
            || document.strikethrough
            || document.scale_box_text_with_transform
            || document.vertical_align.is_some()
            || document.baseline_shift != 0.0
            || !text.animators.is_empty()
            || text.path_options.is_some()
            || text.anchor_options.is_some()
            || text.motion_blur
            || (!text.effects.is_empty()
                && !crate::export_document::effects::omitted_shader_adjustment(&text.effects))
            || !text.masks.is_empty()
            || text.track_matte.is_some()
            || dynamics.iter().any(|entry| {
                entry.target.layer_id() == Some(text.id)
                    && !entry.target.as_property().is_some_and(|target| {
                        matches!(
                            target.property_type(),
                            PropType::Opacity
                                | PropType::PositionX
                                | PropType::PositionY
                                | PropType::AnchorPointX
                                | PropType::AnchorPointY
                                | PropType::ScaleX
                                | PropType::ScaleY
                                | PropType::Rotation
                                | PropType::TextContent
                                | PropType::FontFamily
                                | PropType::FontStyle
                                | PropType::FontSize
                                | PropType::Leading
                                | PropType::Tracking
                                | PropType::FillColor
                        )
                    })
            })
        {
            return Err("Text layout is outside the verified horizontal Point profile");
        }
        // Bound the same native documents that ordinary lowering will emit.
        // Continuous Tracking/FillColor omissions keep their original diagnostics
        // and static base; this is not a new mapping for those unsupported tracks.
        let documents = crate::export_document::text::bounds_documents(text, dynamics, self)?;
        let mut bounds: Option<Bounds> = None;
        for key in &documents.keys {
            if let Some(current) = self.outline_bounds(&key.document)? {
                if let Some(bounds) = &mut bounds {
                    bounds.include(current);
                } else {
                    bounds = Some(current);
                }
            }
        }
        Ok(bounds)
    }

    fn outline_bounds(
        &self,
        document: &crate::writer::text::TextDocumentSpec,
    ) -> Result<Option<Bounds>, &'static str> {
        // Constants and Hold keys may replace the typed base. Qualify each
        // document actually emitted, not stale text, size, font or paint fields.
        if document.box_size.is_some()
            || document.all_caps
            || document.apply_stroke
            || !document.apply_fill
            || !document
                .text
                .bytes()
                .all(|byte| byte == b' ' || byte.is_ascii_graphic())
        {
            return Err("Emitted Text document is outside the verified horizontal Point profile");
        }
        if document.text.is_empty() {
            return Ok(None);
        }
        let tracking = document.tracking;
        if !tracking.is_finite() || tracking != tracking.round() {
            return Err("Font-derived Text bounds require integral native Tracking");
        }
        // Match the writer's actual f32 font size, without a clamp or padding.
        let native_size = document.font_size as f32;
        if !native_size.is_finite() || native_size <= 0.0 {
            return Err("Font-derived Text bounds have no positive native font size");
        }
        let archived = self.resolve_face(&document.font_postscript, "")?;
        let face = rustybuzz::Face::from_slice(&archived.bytes, archived.metadata.face_index)
            .ok_or("Embedded font has no readable shaping face")?;
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(&document.text);
        buffer.guess_segment_properties();
        let shaped = rustybuzz::shape(&face, &[], buffer);
        // Ligatures, reordered clusters and glyph expansion have not been proved
        // against native Tracking; do not silently accept them as ASCII layout.
        if shaped.glyph_infos().len() != document.text.len()
            || shaped
                .glyph_infos()
                .iter()
                .enumerate()
                .any(|(index, glyph)| {
                    usize::try_from(glyph.cluster).ok() != Some(index) || glyph.glyph_id == 0
                })
        {
            return Err("Shaped Text clusters are outside the verified native Tracking profile");
        }
        let scale = f64::from(native_size) / f64::from(face.units_per_em());
        let letter_spacing = tracking * f64::from(native_size) / 1000.0;
        let mut pen = 0.0;
        let mut bounds: Option<Bounds> = None;
        for (index, (glyph, position)) in shaped
            .glyph_infos()
            .iter()
            .zip(shaped.glyph_positions())
            .enumerate()
        {
            if position.y_advance != 0 || position.y_offset != 0 {
                return Err("Vertical glyph positioning has no verified native Point bounds");
            }
            let id = ttf_parser::GlyphId(
                u16::try_from(glyph.glyph_id).map_err(|_| "Shaped glyph identity overflow")?,
            );
            if let Some(rect) = face.glyph_bounding_box(id) {
                let x = pen + f64::from(position.x_offset) * scale;
                let glyph_bounds = Bounds {
                    min: [
                        x + f64::from(rect.x_min) * scale,
                        -f64::from(rect.y_max) * scale,
                    ],
                    max: [
                        x + f64::from(rect.x_max) * scale,
                        -f64::from(rect.y_min) * scale,
                    ],
                };
                if let Some(bounds) = &mut bounds {
                    bounds.include(glyph_bounds);
                } else {
                    bounds = Some(glyph_bounds);
                }
            } else if document.text.as_bytes()[index] != b' ' {
                return Err("Painted glyph has no verified outline bounds");
            }
            pen += f64::from(position.x_advance) * scale;
            if index + 1 < shaped.glyph_infos().len() {
                pen += letter_spacing;
            }
        }
        let shift = match document.justification {
            Justification::Left => 0.0,
            Justification::Center => pen * 0.5,
            Justification::Right => pen,
            Justification::Justify => return Err("Justified Text has no verified Point bounds"),
        };
        let Some(mut bounds) = bounds else {
            return Ok(None);
        };
        bounds.min[0] -= shift;
        bounds.max[0] -= shift;
        if !bounds
            .min
            .iter()
            .chain(bounds.max.iter())
            .all(|value| value.is_finite())
        {
            return Err("Font-derived Text bounds overflowed");
        }
        Ok(Some(bounds))
    }
}
