//! Native plugin Point properties.
//!
//! Adobe stores static and keyed plugin Points as source-relative spatial
//! pairs. Generic independent-component Pair records can be ignored by Adobe.

use crate::rifx::{Chunk, RifxError};

use super::keyframes::{Easing, Track};

#[cfg(test)]
fn descriptor(owner_size: [f64; 2], animated: bool) -> [u8; 124] {
    descriptor_with_clock(
        owner_size,
        animated,
        super::keyframes::PropertyClock::DEFAULT,
    )
}

fn descriptor_with_clock(
    owner_size: [f64; 2],
    animated: bool,
    clock: super::keyframes::PropertyClock,
) -> [u8; 124] {
    let mut record = crate::schema::view_records::StaticPropertyRecord::new(
        2,
        if animated { 14 } else { 15 },
        3,
        u32::MAX,
        4,
        6,
        false,
    );
    if !animated {
        record.set_initialized();
    }
    let mut bytes = record.encode();
    bytes[12..16].copy_from_slice(&clock.ticks().to_be_bytes());
    // Native plugin Point tolerance and source aspect. These controls do not
    // set the AV spatial tangent-mode descriptor bytes.
    bytes[16..24].copy_from_slice(&f64::from_bits(0x3d9b_7cdf_d9d7_bdbc).to_be_bytes());
    bytes[24..32].copy_from_slice(&(owner_size[0] / owner_size[1]).to_be_bytes());
    bytes[68] = u8::from(animated);
    bytes
}

#[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
pub(super) fn static_property(values: &[f64], owner_size: [f64; 2]) -> Result<Chunk, RifxError> {
    static_property_with_clock(values, owner_size, super::keyframes::PropertyClock::DEFAULT)
}

pub(super) fn static_property_with_clock(
    values: &[f64],
    owner_size: [f64; 2],
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, RifxError> {
    validate_owner_size(owner_size)?;
    let mut normalized = values.to_vec();
    normalize_pair(&mut normalized, owner_size, false)?;
    normalized.resize(6, 0.0);
    let data = normalized
        .into_iter()
        .flat_map(f64::to_be_bytes)
        .collect::<Vec<_>>();
    Ok(Chunk::list(
        *b"tdbs",
        vec![
            Chunk::data(*b"tdsb", 1_u32.to_be_bytes())?,
            super::views::name_payload("-_0_/-")?,
            Chunk::data(*b"tdb4", descriptor_with_clock(owner_size, false, clock))?,
            Chunk::data(*b"cdat", data)?,
        ],
    ))
}

fn validate_owner_size(owner_size: [f64; 2]) -> Result<(), RifxError> {
    if owner_size
        .iter()
        .any(|dimension| !dimension.is_finite() || *dimension <= 0.0)
    {
        return Err(RifxError::Invalid(
            "native Point requires positive owner dimensions",
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn animated_property(track: &Track, owner_size: [f64; 2]) -> Result<Chunk, RifxError> {
    animated_property_with_clock(track, owner_size, super::keyframes::PropertyClock::DEFAULT)
}

pub(super) fn animated_property_with_clock(
    track: &Track,
    owner_size: [f64; 2],
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, RifxError> {
    validate_owner_size(owner_size)?;
    validate_animation(track)?;
    let mut normalized = track.clone();
    for key in &mut normalized.keys {
        normalize_pair(&mut key.values, owner_size, false)?;
        normalize_pair(&mut key.spatial_in, owner_size, true)?;
        normalize_pair(&mut key.spatial_out, owner_size, true)?;
        key.easing = vec![shared_easing(&key.easing)?];
    }
    Ok(Chunk::list(
        *b"tdbs",
        vec![
            Chunk::data(*b"tdsb", 1_u32.to_be_bytes())?,
            super::views::name_payload("-_0_/-")?,
            Chunk::data(*b"tdb4", descriptor_with_clock(owner_size, true, clock))?,
            super::keyframes::list_with_clock(&normalized, 2, true, clock)?,
        ],
    ))
}

/// Keep best-effort lowering's animation admission aligned with native writing.
/// Unsupported cubic spatial controls must retain the static authored base.
pub(crate) fn validate_animation(track: &Track) -> Result<(), RifxError> {
    // The right key owns the segment ease, but its spatial curve also uses
    // the left key's outgoing tangent (even when that key's ease is Linear).
    for pair in track.keys.windows(2) {
        if pair[1]
            .easing
            .iter()
            .any(|easing| matches!(easing, Easing::CubicBezier { .. }))
            && pair[0]
                .spatial_out
                .iter()
                .chain(&pair[1].spatial_in)
                .any(|value| *value != 0.0)
        {
            return Err(RifxError::Invalid(
                "cubic native Point requires zero spatial tangents",
            ));
        }
    }
    for key in &track.keys {
        shared_easing(&key.easing)?;
        if key
            .easing
            .iter()
            .any(|easing| matches!(easing, Easing::CubicBezier { .. }))
            && key
                .spatial_in
                .iter()
                .chain(&key.spatial_out)
                .any(|value| *value != 0.0)
        {
            return Err(RifxError::Invalid(
                "cubic native Point requires zero spatial tangents",
            ));
        }
    }
    Ok(())
}

fn normalize_pair(
    values: &mut Vec<f64>,
    owner_size: [f64; 2],
    empty_is_zero: bool,
) -> Result<(), RifxError> {
    if values.is_empty() && empty_is_zero {
        values.resize(2, 0.0);
    }
    let [x, y] = values.as_mut_slice() else {
        return Err(RifxError::Invalid(
            "animated native Point requires two-dimensional values and tangents",
        ));
    };
    *x /= owner_size[0];
    *y /= owner_size[1];
    Ok(())
}

pub(crate) fn shared_easing(easing: &[Easing]) -> Result<Easing, RifxError> {
    if !easing.is_empty() && easing.iter().all(|value| matches!(value, Easing::Linear)) {
        return Ok(Easing::Linear);
    }
    if !easing.is_empty() && easing.iter().all(|value| matches!(value, Easing::Hold)) {
        return Ok(Easing::Hold);
    }
    // Zero endpoint slopes have native speed zero in both pixel and source-relative
    // coordinates. Restrict the shared path-speed mapping to that exact profile;
    // independently eased axes and nonzero-speed unit conversion remain unsupported.
    if let Some(&curve @ Easing::CubicBezier { x1, y1, x2, y2 }) = easing.first()
        && y1 == 0.0
        && y2 == 1.0
        && x1.is_finite()
        && x2.is_finite()
        && x1 > 0.0
        && x1 <= 1.0
        && (0.0..1.0).contains(&x2)
        && easing.iter().all(|value| *value == curve)
    {
        return Ok(curve);
    }
    Err(RifxError::Invalid(
        "animated native Point requires shared Linear, Hold or zero-speed cubic easing",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        properties,
        structure::{ItemKind, read_project},
        writer::{
            CompositionSpec, KeyframeEasing, LayerSpec, NativeLayerOptions, NumericKeyframe,
            NumericTrack, SolidLayerSpec, SolidTransform,
        },
    };

    const ANIMATED_CATALOG: &[u8] =
        include_bytes!("../../tests/fixtures/effects/animated_catalog.aep");

    fn named_property<'a>(chunks: &'a [Chunk], name: &str) -> Option<&'a [Chunk]> {
        for pair in chunks.windows(2) {
            if pair[0].id() == *b"tdmn"
                && pair[0].data_payload()?.split(|byte| *byte == 0).next()? == name.as_bytes()
                && pair[1].list_kind() == Some(*b"tdbs")
            {
                return pair[1].children();
            }
        }
        chunks
            .iter()
            .filter_map(Chunk::children)
            .find_map(|children| named_property(children, name))
    }

    fn point_track(values: [[f64; 2]; 2], easing: KeyframeEasing) -> NumericTrack {
        NumericTrack {
            keys: values
                .into_iter()
                .enumerate()
                .map(|(index, values)| NumericKeyframe {
                    time_millis: i64::try_from(index).expect("two keys") * 1000,
                    values: values.to_vec(),
                    easing: vec![easing; 2],
                    spatial_in: Vec::new(),
                    spatial_out: Vec::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn point_zero_speed_shared_profile_rejects_nonzero_and_independent_eases() {
        let curve = Easing::CubicBezier {
            x1: 1.0 / 3.0,
            y1: 0.0,
            x2: 2.0 / 3.0,
            y2: 1.0,
        };
        assert_eq!(shared_easing(&[curve, curve]).unwrap(), curve);
        for other in [
            Easing::Linear,
            Easing::CubicBezier {
                x1: 0.2,
                y1: 0.0,
                x2: 0.8,
                y2: 1.0,
            },
            Easing::CubicBezier {
                x1: 1.0 / 3.0,
                y1: 0.2,
                x2: 2.0 / 3.0,
                y2: 1.0,
            },
        ] {
            assert!(shared_easing(&[curve, other]).is_err());
        }
        assert!(shared_easing(&[]).is_err());
        assert!(
            shared_easing(&[Easing::CubicBezier {
                x1: 0.0,
                y1: 0.0,
                x2: 1.0,
                y2: 1.0
            }])
            .is_err()
        );
    }

    #[test]
    fn point_zero_speed_cubic_rejects_curved_spatial_handles() {
        let mut track = point_track(
            [[60.0, 40.0], [72.0, 33.0]],
            Easing::CubicBezier {
                x1: 1.0 / 3.0,
                y1: 0.0,
                x2: 2.0 / 3.0,
                y2: 1.0,
            },
        );
        assert!(animated_property(&track, [120.0, 80.0]).is_ok());
        // The destination key owns the segment ease; the source key's unused
        // incoming ease may be Linear even when its outgoing handle is curved.
        track.keys[0].easing.fill(Easing::Linear);
        for (key_index, incoming) in [(0, false), (1, true)] {
            let mut curved = track.clone();
            if incoming {
                curved.keys[key_index].spatial_in = vec![1.0, 0.0];
            } else {
                curved.keys[key_index].spatial_out = vec![1.0, 0.0];
            }
            assert!(animated_property(&curved, [120.0, 80.0]).is_err());
        }
        assert!(animated_property(&track, [120.0, 80.0]).is_ok());
    }

    #[test]
    fn static_point_override_uses_native_source_relative_layout() {
        let project = read_project(include_bytes!(
            "../../tests/fixtures/effects/static_point_controls.aep"
        ))
        .unwrap();
        let ItemKind::Composition(composition) = &project.item(1).unwrap().kind else {
            panic!("native Shape/Twirl composition");
        };
        let native = named_property(&composition.layers[0].content, "ADBE Twirl-0003").unwrap();
        let mut effect =
            super::super::effects::new_effect("ADBE Twirl", true, [120.0, 80.0]).unwrap();
        effect
            .properties
            .iter_mut()
            .find(|property| property.match_name == "ADBE Twirl-0003")
            .unwrap()
            .values = vec![30.0, 60.0];
        let generated =
            super::super::effects::effect_parade(&[effect], 13, [320.0, 180.0]).unwrap();
        let fresh = named_property(generated.children().unwrap(), "ADBE Twirl-0003").unwrap();
        for tag in [*b"tdb4", *b"cdat"] {
            assert_eq!(
                properties::data(fresh, tag).unwrap(),
                properties::data(native, tag).unwrap()
            );
        }
    }

    #[test]
    fn descriptor_and_key_stride_match_pinned_adobe_point() {
        let project = read_project(ANIMATED_CATALOG).expect("pinned Adobe-native project");
        let ItemKind::Composition(composition) = &project.item(114).expect("Radial Blur comp").kind
        else {
            panic!("pinned item 114 is not a composition");
        };
        let native = named_property(&composition.layers[0].content, "ADBE Radial Blur-0002")
            .expect("pinned animated Point");
        assert_eq!(
            properties::data(native, *b"tdb4").expect("native Point descriptor"),
            descriptor([120.0, 80.0], true)
        );
        let native_list = properties::unique_list(native, *b"list").expect("native Point keys");
        assert_eq!(
            &properties::data(native_list, *b"lhd3").expect("native key header")[18..20],
            &104_u16.to_be_bytes()
        );

        let generated = animated_property(
            &point_track([[60.0, 40.0], [72.0, 33.0]], KeyframeEasing::Linear),
            [120.0, 80.0],
        )
        .expect("fresh animated Point");
        let children = generated.children().expect("fresh Point children");
        assert_eq!(
            properties::data(children, *b"tdb4").expect("fresh Point descriptor"),
            properties::data(native, *b"tdb4").expect("native Point descriptor")
        );
        assert_eq!(
            properties::data(children, *b"tdsb").expect("fresh Point flags"),
            properties::data(native, *b"tdsb").expect("native Point flags")
        );
        let generated_list = properties::unique_list(children, *b"list").expect("fresh keys");
        assert_eq!(
            &properties::data(generated_list, *b"lhd3").expect("fresh key header")[18..20],
            &104_u16.to_be_bytes()
        );
    }

    #[test]
    fn fresh_point_reimports_in_owner_pixels() {
        let mut effect = super::super::effects::new_effect("ADBE Radial Blur", true, [120.0, 80.0])
            .expect("Radial Blur definition");
        let point = effect
            .properties
            .iter_mut()
            .find(|property| property.match_name == "ADBE Radial Blur-0002")
            .expect("Radial Blur center");
        point.values = vec![60.0, 40.0];
        point.animation = Some(point_track(
            [[30.0, 20.0], [90.0, 60.0]],
            KeyframeEasing::Hold,
        ));
        let solid = SolidLayerSpec {
            name: "Point owner".into(),
            width: 120,
            height: 80,
            color: [0.0, 0.0, 0.0],
            transform: SolidTransform {
                anchor: [60.0, 40.0],
                position: [160.0, 90.0],
                scale: [100.0, 100.0],
                rotation: 0.0,
                opacity: 100.0,
            },
        };
        let bytes = crate::writer::write_composition(
            &CompositionSpec {
                name: "Point".into(),
                width: 320,
                height: 180,
                duration_frames: 48,
            },
            &[LayerSpec::Options(
                Box::new(LayerSpec::Solid(solid)),
                NativeLayerOptions {
                    fx_id: fx_schema::LayerId::new(1),
                    parent: None,
                    matte: None,
                    enabled: true,
                    adjustment_layer: false,
                    motion_blur: false,
                    blend_mode: 2,
                    masks: Vec::new(),
                    effects: vec![effect],
                    styles: Vec::new(),
                    source_clock: None,
                    transform_3d: None,
                },
            )],
        )
        .expect("fresh Point project");
        let project = read_project(&bytes).expect("reimport fresh project");
        let ItemKind::Composition(composition) = &project.item(1).expect("composition").kind else {
            panic!("item 1 is not a composition");
        };
        let (effects, warnings) =
            crate::effects::native::read_effects(&composition.layers[0].content, [120.0, 80.0]);
        // The plugin's nonnumeric UI control is outside the numeric reader.
        assert!(
            warnings
                .iter()
                .all(|warning| warning.contains("ADBE Radial Blur-0005")),
            "{warnings:?}"
        );
        let point = effects[0]
            .parameters
            .iter()
            .find(|property| property.match_name == "ADBE Radial Blur-0002")
            .expect("reimported center")
            .numeric
            .as_ref()
            .expect("numeric center");
        assert_eq!(point.keyframes[0].values, vec![30.0, 20.0]);
        assert_eq!(point.keyframes[1].values, vec![90.0, 60.0]);
        assert_eq!(point.keyframes[0].spatial_in, vec![0.0, 0.0]);
        assert_eq!(point.keyframes[0].spatial_out, vec![0.0, 0.0]);
        assert_eq!(point.keyframes[0].out_interpolation, 3);
        assert_eq!(point.keyframes[1].in_interpolation, 3);
    }
}
