//! Fresh static-solid timeline records. No source project is loaded at runtime.
//!
//! The AE26 field/default layout follows pinned py-aep e12a451's LdtaChunk,
//! SspcChunk and SoliOptiChunk and the native transform fixtures. These records
//! are experimental until independently opened/rendered in Adobe.

use crate::{
    rifx::Chunk,
    schema::{ItemRecord, layer_records::LayerRecord},
    timing::Duration24,
};

use super::{AepWriteError, CompositionSpec, NumericTrack, checked_duration, root, views};

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TransformAnimations {
    pub anchor: Option<NumericTrack>,
    pub position: Option<NumericTrack>,
    pub scale: Option<NumericTrack>,
    pub rotation: Option<NumericTrack>,
    pub opacity: Option<NumericTrack>,
}

/// Static AE 2D layer Transform in user-facing units.
#[derive(Clone, Debug, PartialEq)]
pub struct SolidTransform {
    /// Source-space anchor measured from the solid's top-left, in pixels.
    pub anchor: [f64; 2],
    /// Composition-space anchor position, in pixels.
    pub position: [f64; 2],
    /// Axis scale percentages, including zero and negative scale.
    pub scale: [f64; 2],
    /// Clockwise Z rotation in degrees.
    pub rotation: f64,
    /// Layer opacity percentage in 0..=100.
    pub opacity: f64,
}

/// One static, full-composition-span solid. Input order is AE timeline order.
#[derive(Clone, Debug, PartialEq)]
pub struct SolidLayerSpec {
    /// Explicit layer/source name, at most 255 UTF-8 bytes and without NUL.
    pub name: String,
    /// Positive source width in square pixels.
    pub width: u16,
    /// Positive source height in square pixels.
    pub height: u16,
    /// Straight source RGB, each component in 0..=1.
    pub color: [f32; 3],
    /// Native editable layer Transform, not baked into source geometry.
    pub transform: SolidTransform,
}

/// Build a fresh AE26 project containing editable static solid layers.
///
/// This bounded writer slice supports only square-pixel 24fps compositions,
/// full-span 2D solids and Normal blending. It is not yet the FX exporter, and
/// successful decoding by our reader is not independent Adobe acceptance.
pub fn write_solid_composition(
    spec: &CompositionSpec,
    layers: &[SolidLayerSpec],
) -> Result<Vec<u8>, AepWriteError> {
    let duration = checked_duration(spec)?;
    for layer in layers {
        validate(layer)?;
    }
    let mut timeline = root::Timeline::default();
    for layer in layers {
        let source_id = timeline.next_id;
        let (layer_id, next_id) = checked_solid_ids(source_id)?;
        timeline.next_id = next_id;
        timeline.sources.push(source_item(layer, source_id)?);
        timeline
            .layers
            .push(timeline_layer(layer, layer_id, source_id, duration, None)?);
        timeline.layers.push(Chunk::list(*b"Ewst", Vec::new()));
        timeline.layers.extend(root::item_envelope_tail());
        timeline.layers.extend(root::item_envelope_tail());
    }
    let views = views::build_views(spec.width, spec.height, duration)?;
    let project = root::build_project_with_timeline(
        &spec.name,
        spec.width,
        spec.height,
        duration,
        views,
        timeline,
    )?;
    Ok(project.encode()?)
}

fn checked_solid_ids(source_id: u32) -> Result<(u32, u32), AepWriteError> {
    let layer_id = source_id
        .checked_add(1)
        .ok_or(AepWriteError::Invalid("native ID overflow"))?;
    let next_id = layer_id
        .checked_add(1)
        .ok_or(AepWriteError::Invalid("native ID overflow"))?;
    Ok((layer_id, next_id))
}

pub(super) fn validate(layer: &SolidLayerSpec) -> Result<(), AepWriteError> {
    if layer.name.is_empty() || layer.name.len() > 255 || layer.name.contains('\0') {
        return Err(AepWriteError::Invalid(
            "solid name must be 1..=255 UTF-8 bytes without NUL",
        ));
    }
    if layer.width == 0 || layer.height == 0 {
        return Err(AepWriteError::Invalid("zero solid dimension"));
    }
    if layer
        .color
        .iter()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err(AepWriteError::Invalid("solid color is outside 0..=1"));
    }
    let transform = &layer.transform;
    if transform
        .anchor
        .iter()
        .chain(&transform.position)
        .chain(&transform.scale)
        .any(|v| !v.is_finite())
        || !transform.rotation.is_finite()
        || !transform.opacity.is_finite()
        || !(0.0..=100.0).contains(&transform.opacity)
    {
        return Err(AepWriteError::Invalid(
            "non-finite Transform or invalid opacity",
        ));
    }
    Ok(())
}

fn raw(tag: [u8; 4], bytes: impl Into<Vec<u8>>) -> Chunk {
    // Only literal non-LIST tags are used by this private constructor.
    Chunk::data(tag, bytes).expect("solid writer raw tags are never LIST")
}

pub(super) fn source_item(layer: &SolidLayerSpec, id: u32) -> Result<Chunk, AepWriteError> {
    let mut settings = [0_u8; 222];
    settings[22..26].copy_from_slice(b"Soli");
    settings[32..34].copy_from_slice(&layer.width.to_be_bytes());
    settings[36..38].copy_from_slice(&layer.height.to_be_bytes());
    // A synthetic still has no intrinsic duration/frame rate or source stamp.
    settings[42..46].copy_from_slice(&1_u32.to_be_bytes());
    settings[52..54].copy_from_slice(&600_u16.to_be_bytes());
    settings[73] = 3; // No alpha channel; opacity belongs to the layer.
    settings[105] = 1; // Synthetic-source flags.
    settings[109] = 1;
    settings[125] = 12; // Observed native depth flag.
    settings[126..130].copy_from_slice(&1_u32.to_be_bytes()); // One loop.
    settings[134] = 1;
    settings[136..140].copy_from_slice(&1_u32.to_be_bytes());
    settings[140..144].copy_from_slice(&1_u32.to_be_bytes());
    settings[188..196].fill(255); // No Photoshop layer ID/index.
    settings[212] = 1; // AE26 extended-settings default.

    let mut options = [0_u8; 282];
    options[..4].copy_from_slice(b"Soli");
    options[4..6].copy_from_slice(&9_u16.to_be_bytes());
    options[6..10].copy_from_slice(&282_u32.to_be_bytes());
    options[10..14].copy_from_slice(&1_f32.to_be_bytes());
    for (offset, value) in [14, 18, 22].into_iter().zip(layer.color) {
        options[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    options[26..26 + layer.name.len()].copy_from_slice(layer.name.as_bytes());
    let pin = Chunk::list(
        *b"Pin ",
        vec![
            raw(*b"sspc", settings),
            raw(*b"Utf8", Vec::new()),
            raw(*b"opti", options),
            raw(*b"pgui", [0; 16]),
            // Unassigned profiles and AE26's default source color settings.
            // These required records are authored afresh, not fixture subtrees.
            Chunk::list(
                *b"CLRS",
                vec![
                    raw(*b"epid", [255; 16]),
                    raw(*b"apid", [255; 16]),
                    raw(*b"linl", 2_u32.to_le_bytes()),
                    raw(*b"embp", [1]),
                    raw(*b"ipws", [1]),
                    raw(*b"dcui", [1]),
                    raw(*b"prgb", [1]),
                    raw(*b"Mcsp", [1]),
                    raw(*b"Utf8", Vec::new()),
                    raw(*b"ocsp", [1]),
                    raw(*b"Utf8", Vec::new()),
                    raw(*b"hdrm", [1]),
                    raw(*b"Utf8", b"{}".to_vec()),
                ],
            ),
            raw(*b"Utf8", Vec::new()),
        ],
    );
    Ok(Chunk::list(
        *b"Item",
        vec![
            raw(*b"iide", id.to_le_bytes()),
            raw(*b"idpc", 0_u64.to_be_bytes()),
            raw(*b"idta", ItemRecord::solid_ae26(id)?.encode()),
            raw(*b"Utf8", layer.name.as_bytes().to_vec()),
            pin,
            raw(
                *b"ftgi",
                [0_u32, 1, u32::MAX, 600]
                    .into_iter()
                    .flat_map(u32::to_be_bytes)
                    .collect::<Vec<_>>(),
            ),
            raw(*b"Utf8", Vec::new()),
        ],
    ))
}

pub(super) fn timeline_layer(
    layer: &SolidLayerSpec,
    id: u32,
    source_id: u32,
    duration: Duration24,
    animations: Option<&TransformAnimations>,
) -> Result<Chunk, AepWriteError> {
    timeline_layer_with_clock(
        layer,
        id,
        source_id,
        duration,
        animations,
        super::keyframes::PropertyClock::DEFAULT,
    )
}

pub(super) fn timeline_layer_with_clock(
    layer: &SolidLayerSpec,
    id: u32,
    source_id: u32,
    duration: Duration24,
    animations: Option<&TransformAnimations>,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    use views::{ValueKind, group};
    let t = &layer.transform;
    let dimensions = [f64::from(layer.width), f64::from(layer.height)];
    let mut anchor_animation = animations.and_then(|value| value.anchor.clone());
    if let Some(track) = &mut anchor_animation {
        for key in &mut track.keys {
            for values in [&mut key.values, &mut key.spatial_in, &mut key.spatial_out] {
                for (value, dimension) in values.iter_mut().zip(dimensions) {
                    *value /= dimension;
                }
            }
        }
    }
    let transform = group(
        1,
        "-_0_/-",
        vec![
            (
                "ADBE Anchor Point",
                views::property_with_clock(
                    ValueKind::Spatial,
                    &[
                        t.anchor[0] / dimensions[0],
                        t.anchor[1] / dimensions[1],
                        0.0,
                    ],
                    None,
                    anchor_animation.as_ref(),
                    clock,
                )?,
            ),
            (
                "ADBE Position",
                views::property_with_clock(
                    ValueKind::Spatial,
                    &[t.position[0], t.position[1], 0.0],
                    None,
                    animations.and_then(|value| value.position.as_ref()),
                    clock,
                )?,
            ),
            (
                "ADBE Scale",
                views::property_with_clock(
                    ValueKind::Scale,
                    &[t.scale[0] / 100.0, t.scale[1] / 100.0, 1.0],
                    Some((0.0, 0.0)),
                    animations.and_then(|value| value.scale.as_ref()),
                    clock,
                )?,
            ),
            (
                "ADBE Rotate Z",
                views::property_with_clock(
                    ValueKind::Angle,
                    &[t.rotation],
                    None,
                    animations.and_then(|value| value.rotation.as_ref()),
                    clock,
                )?,
            ),
            (
                "ADBE Opacity",
                views::property_with_clock(
                    ValueKind::Scalar,
                    &[t.opacity / 100.0],
                    Some((0.0, 100.0)),
                    animations.and_then(|value| value.opacity.as_ref()),
                    clock,
                )?,
            ),
        ],
    )?;
    let properties = group(1, "", vec![("ADBE Transform Group", transform)])?;
    Ok(Chunk::list(
        *b"Layr",
        vec![
            raw(
                *b"ldta",
                LayerRecord::solid_ae26(id, source_id, duration)?.encode(),
            ),
            raw(*b"Utf8", layer.name.as_bytes().to_vec()),
            properties,
        ],
    ))
}

#[cfg(test)]
mod tests;
