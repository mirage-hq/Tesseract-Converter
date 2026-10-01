//! Fresh AE26 plugin records from a versioned parameter ABI registry.
//! Registry entries contain no source project, user values, expressions or keys.

use super::{
    AepWriteError,
    keyframes::Track,
    views::{self, ValueKind},
};
use crate::{
    effects::definitions,
    rifx::{Chunk, RifxError},
};
use std::collections::BTreeSet;

mod hue_saturation;

#[cfg(test)]
mod animation_tests;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeEffectProperty {
    pub match_name: String,
    pub display_name: String,
    pub kind: ValueKind,
    /// Native UI units, except colors which are normalized RGBA.
    pub values: Vec<f64>,
    pub bounds: Option<(f64, f64)>,
    pub animation: Option<Track>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeEffect {
    pub match_name: String,
    pub name: String,
    pub enabled: bool,
    pub properties: Vec<NativeEffectProperty>,
}

pub(crate) fn new_effect(
    match_name: &str,
    enabled: bool,
    size: [f64; 2],
) -> Result<NativeEffect, RifxError> {
    let definition = definitions::definition(match_name).ok_or(RifxError::Invalid(
        "effect has no canonical AE26 parameter definition",
    ))?;
    let properties = definition
        .parameters
        .iter()
        .filter_map(|parameter| {
            if parameter.match_name.ends_with("-0000") {
                return None;
            }
            let kind = match parameter.kind {
                1 | 4 | 7 => ValueKind::Toggle,
                2 | 10 => ValueKind::Scalar,
                3 => ValueKind::Angle,
                5 => ValueKind::Color,
                6 => ValueKind::Pair,
                _ => return None,
            };
            let values = parameter.values(size)?;
            Some(NativeEffectProperty {
                match_name: parameter.match_name.clone(),
                display_name: parameter.label.clone(),
                kind,
                values,
                bounds: match parameter.kind {
                    1 => Some((
                        f64::from(parameter.payload_words[19] as i32),
                        f64::from(parameter.payload_words[20] as i32),
                    )),
                    2 => Some((
                        f64::from(parameter.payload_words[19] as i32) / 65536.0,
                        f64::from(parameter.payload_words[20] as i32) / 65536.0,
                    )),
                    10 => Some((
                        f64::from(f32::from_bits(parameter.payload_words[14])),
                        f64::from(f32::from_bits(parameter.payload_words[15])),
                    )),
                    _ => None,
                },
                animation: None,
            })
        })
        .collect();
    Ok(NativeEffect {
        match_name: match_name.into(),
        name: definition.name.clone(),
        enabled,
        properties,
    })
}

fn match_name(value: &str) -> Result<Chunk, RifxError> {
    if value.len() > 39 || value.contains('\0') {
        return Err(RifxError::Invalid("invalid effect match name"));
    }
    let mut bytes = vec![0; 40];
    bytes[..value.len()].copy_from_slice(value.as_bytes());
    Chunk::data(*b"tdmn", bytes)
}
fn string(tag: [u8; 4], value: &str) -> Result<Chunk, RifxError> {
    let payload = views::name_payload(value)?;
    Chunk::data(
        tag,
        payload
            .data_payload()
            .ok_or(RifxError::Invalid("invalid effect string"))?
            .to_vec(),
    )
}

/// Native Master parameter words are signed 16:16, unlike ordinary f64 leaves.
pub(crate) fn hue_master_fixed(value: f64) -> Result<u32, RifxError> {
    let fixed = (value * 65_536.0).round();
    if !fixed.is_finite() || fixed < f64::from(i32::MIN) || fixed > f64::from(i32::MAX) {
        return Err(RifxError::Invalid(
            "Hue/Saturation Master value exceeds native 16:16 range",
        ));
    }
    // Bounds above make the float cast exact; reinterpret the signed bits.
    Ok(u32::from_be_bytes((fixed as i32).to_be_bytes()))
}

fn plugin(effect: &NativeEffect, owner_id: u32, owner_size: [f64; 2]) -> Result<Chunk, RifxError> {
    plugin_with_clock(
        effect,
        owner_id,
        owner_size,
        super::keyframes::PropertyClock::DEFAULT,
    )
}

fn plugin_with_clock(
    effect: &NativeEffect,
    owner_id: u32,
    owner_size: [f64; 2],
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, RifxError> {
    let definition = definitions::definition(&effect.match_name)
        .ok_or(RifxError::Invalid("unknown native effect definition"))?;
    let count = u32::try_from(definition.parameters.len())
        .map_err(|_| RifxError::Invalid("too many effect definitions"))?;
    let mut table = vec![Chunk::data(*b"parn", count.to_be_bytes())?];
    for parameter in &definition.parameters {
        table.push(match_name(&parameter.match_name)?);
        // Match AE's visible Master defaults as well as its rendered arbitrary
        // state (inserted below). pard alone does not drive Adobe rendering.
        // Only the owner receives edits; global definitions stay canonical.
        let mut instance = parameter.clone();
        if effect.match_name == "ADBE HUE SATURATION"
            && matches!(
                parameter.match_name.as_str(),
                "ADBE HUE SATURATION-0004"
                    | "ADBE HUE SATURATION-0005"
                    | "ADBE HUE SATURATION-0006"
            )
            && let Some(property) = effect
                .properties
                .iter()
                .find(|property| property.match_name == parameter.match_name)
        {
            let value = property
                .values
                .first()
                .ok_or(RifxError::Invalid("missing Hue/Saturation Master value"))?;
            instance.payload_words[0] = hue_master_fixed(*value)?;
        }
        table.push(definitions::encode_parameter(&instance)?);
        if effect.match_name == "ADBE HUE SATURATION"
            && parameter.match_name == "ADBE HUE SATURATION-0003"
        {
            table.push(hue_saturation::default_data()?);
        }
        if let Some(popup) = &parameter.popup {
            table.push(string(*b"pdnm", popup)?);
        }
    }
    let root_name = format!("{}-0000", effect.match_name);
    let mut root = views::property_with_clock(ValueKind::Toggle, &[0.0], None, None, clock)?;
    if let Some(children) = root.children_mut() {
        children.push(Chunk::data(*b"tdpi", owner_id.to_be_bytes())?);
        children.push(Chunk::data(*b"tdps", 0_u32.to_be_bytes())?);
    }
    let mut nodes = vec![(root_name.as_str(), root)];
    for property in &effect.properties {
        let parameter = definition
            .parameters
            .iter()
            .find(|parameter| parameter.match_name == property.match_name)
            .ok_or(RifxError::Invalid("invalid native effect parameter"))?;
        if property.values.iter().any(|v| !v.is_finite()) {
            return Err(RifxError::Invalid("invalid native effect parameter"));
        }
        let mut values = property.values.clone();
        let mut animation = property.animation.clone();
        if matches!(property.kind, ValueKind::Color) {
            if values.len() != 4 {
                return Err(RifxError::Invalid("native color requires four components"));
            }
            values.rotate_right(1);
            values.iter_mut().for_each(|v| *v *= 255.0);
            if let Some(track) = &mut animation {
                for key in &mut track.keys {
                    key.values.rotate_right(1);
                    key.values.iter_mut().for_each(|v| *v *= 255.0);
                    key.easing.rotate_right(1);
                }
            }
        }
        let mut node = if matches!(property.kind, ValueKind::Pair) {
            match animation.as_ref() {
                Some(track) => {
                    super::effect_points::animated_property_with_clock(track, owner_size, clock)?
                }
                None => {
                    super::effect_points::static_property_with_clock(&values, owner_size, clock)?
                }
            }
        } else {
            // Plugin animation records depend on the native parameter type,
            // not the generic FX numeric arity. Transform-style records expose
            // keys in Adobe but can freeze continuous rendering at the first key.
            // Keep static serialization unchanged, including the established
            // Brightness & Contrast scalar variant.
            let kind = if animation.is_some() {
                match parameter.kind {
                    1 => ValueKind::EffectInteger,
                    4 | 7 => ValueKind::EffectToggle,
                    2 | 3 => ValueKind::EffectScalar,
                    5 => ValueKind::EffectColor,
                    10 => ValueKind::EffectFloat,
                    _ => property.kind,
                }
            } else if effect.match_name == "ADBE Brightness & Contrast 2"
                && property.kind == ValueKind::Scalar
            {
                ValueKind::EffectScalar
            } else {
                property.kind
            };
            views::property_with_clock(kind, &values, property.bounds, animation.as_ref(), clock)?
        };
        if let Some(children) = node.children_mut()
            && let Some(name) = children.iter_mut().find(|c| c.id() == *b"tdsn")
        {
            *name = views::name_payload(&property.display_name)?;
        }
        nodes.push((property.match_name.as_str(), node));
    }
    nodes.push((
        "ADBE Effect Built In Params",
        views::group(1, "Compositing Options", vec![])?,
    ));
    let mut group = views::group(u32::from(effect.enabled), "-_0_/-", nodes)?;
    if effect.match_name == "ADBE HUE SATURATION" {
        hue_saturation::insert_instance(&mut group, effect)?;
    }
    Ok(Chunk::list(
        *b"sspc",
        vec![
            string(*b"fnam", &effect.name)?,
            Chunk::list(*b"parT", table),
            group,
            Chunk::data(*b"pgui", vec![0; 16])?,
            Chunk::data(*b"elab", vec![255])?,
        ],
    ))
}

#[cfg(test)]
pub(crate) fn effect_parade(
    effects: &[NativeEffect],
    owner_id: u32,
    owner_size: [f64; 2],
) -> Result<Chunk, RifxError> {
    effect_parade_with_clock(
        effects,
        owner_id,
        owner_size,
        super::keyframes::PropertyClock::DEFAULT,
    )
}

pub(super) fn effect_parade_with_clock(
    effects: &[NativeEffect],
    owner_id: u32,
    owner_size: [f64; 2],
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, RifxError> {
    let nodes = effects
        .iter()
        .map(|effect| {
            Ok((
                effect.match_name.as_str(),
                plugin_with_clock(effect, owner_id, owner_size, clock)?,
            ))
        })
        .collect::<Result<Vec<_>, RifxError>>()?;
    views::group(1, "-_0_/-", nodes)
}

#[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
pub(crate) fn apply(
    layer: &mut Chunk,
    effects: &[NativeEffect],
    owner_size: [f64; 2],
) -> Result<(), AepWriteError> {
    apply_with_clock(
        layer,
        effects,
        owner_size,
        super::keyframes::PropertyClock::DEFAULT,
    )
}

pub(super) fn apply_with_clock(
    layer: &mut Chunk,
    effects: &[NativeEffect],
    owner_size: [f64; 2],
    clock: super::keyframes::PropertyClock,
) -> Result<(), AepWriteError> {
    if effects.is_empty() {
        return Ok(());
    }
    let records = layer
        .children_mut()
        .ok_or(AepWriteError::Invalid("effect owner is not a native layer"))?;
    let owner = records
        .iter()
        .find(|chunk| chunk.id() == *b"ldta")
        .and_then(Chunk::data_payload)
        .ok_or(AepWriteError::Invalid("effect owner has no layer record"))?;
    let owner_id = crate::schema::layer_records::LayerRecord::decode(owner)?.id();
    let root = records
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .and_then(Chunk::children_mut)
        .ok_or(AepWriteError::Invalid("effect owner has no property root"))?;
    if root.iter().any(|chunk| {
        chunk.id() == *b"tdmn"
            && chunk
                .data_payload()
                .is_some_and(|data| data.starts_with(b"ADBE Effect Parade\0"))
    }) {
        return Err(AepWriteError::Invalid("duplicate native effect parade"));
    }
    let index = root
        .iter()
        .position(|chunk| {
            chunk.id() == *b"tdmn"
                && chunk
                    .data_payload()
                    .is_some_and(|data| data.starts_with(b"ADBE Group End\0"))
        })
        .unwrap_or(root.len());
    root.splice(
        index..index,
        [
            match_name("ADBE Effect Parade")?,
            effect_parade_with_clock(effects, owner_id, owner_size, clock)?,
        ],
    );
    Ok(())
}

/// Register only freshly generated plugin instances. Never consult input AEPs.
pub(crate) fn register_definitions(chunks: &mut Vec<Chunk>) -> Result<(), AepWriteError> {
    fn collect(chunks: &[Chunk], definitions: &mut BTreeSet<String>) {
        for pair in chunks.windows(2) {
            if pair[0].id() == *b"tdmn"
                && pair[1].list_kind() == Some(*b"sspc")
                && let Some(bytes) = pair[0].data_payload()
                && let Ok(name) =
                    std::str::from_utf8(bytes.split(|b| *b == 0).next().unwrap_or_default())
                && definitions::definition(name).is_some()
            {
                definitions.insert(name.to_owned());
            }
        }
        for chunk in chunks {
            if let Some(children) = chunk.children() {
                collect(children, definitions);
            }
        }
    }
    let mut definitions = BTreeSet::new();
    collect(chunks, &mut definitions);
    if definitions.is_empty() {
        return Ok(());
    }
    if chunks.iter().any(|c| c.list_kind() == Some(*b"EfdG")) {
        return Err(AepWriteError::Invalid(
            "duplicate effect definition registry",
        ));
    }
    let count = u32::try_from(definitions.len())
        .map_err(|_| AepWriteError::Invalid("too many effect definitions"))?;
    let plugin_index = chunks
        .iter()
        .position(|chunk| chunk.list_kind() == Some(*b"Pefl"))
        .ok_or(AepWriteError::Invalid("missing project plugin list"))?;
    let plugin_names = definitions
        .iter()
        .map(|name| Chunk::data(*b"pjef", name.as_bytes().to_vec()))
        .collect::<Result<Vec<_>, _>>()?;
    chunks[plugin_index] = Chunk::list(*b"Pefl", plugin_names);
    let mut records = vec![Chunk::data(*b"EfDC", count.to_le_bytes())?];
    for name in definitions {
        // A definition needs its root property group, but must not inherit the
        // first layer's edited controls, enable state, owner or animation keys.
        let mut canonical = new_effect(&name, true, [1.0; 2])?;
        canonical.properties.clear();
        records.push(Chunk::list(
            *b"EfDf",
            vec![match_name(&name)?, plugin(&canonical, 0, [1.0; 2])?],
        ));
    }
    let index = plugin_index + 1;
    chunks.insert(index, Chunk::list(*b"EfdG", records));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::{
        CompositionSpec, LayerSpec, NativeLayerOptions, SolidLayerSpec, SolidTransform,
    };

    #[test]
    fn effect_parade_accepts_more_than_former_effect_quota() {
        let effect = new_effect("ADBE Gaussian Blur 2", true, [120.0, 80.0]).unwrap();
        let effects = vec![effect; 513];
        assert!(effect_parade(&effects, 7, [120.0, 80.0]).is_ok());
    }

    #[test]
    fn effect_parade_still_rejects_unknown_effects() {
        let effect = NativeEffect {
            match_name: "unknown".into(),
            name: "Unknown".into(),
            enabled: true,
            properties: Vec::new(),
        };
        assert!(effect_parade(&[effect], 7, [120.0, 80.0]).is_err());
    }

    #[test]
    fn project_definitions_do_not_embed_first_owner_edits_or_keys() {
        fn registered(value: f64, owner: u32, enabled: bool) -> Chunk {
            let mut effect = new_effect("ADBE Gaussian Blur 2", enabled, [120.0, 80.0]).unwrap();
            let blur = effect
                .properties
                .iter_mut()
                .find(|property| property.match_name.ends_with("-0001"))
                .unwrap();
            blur.values = vec![value];
            blur.animation = Some(Track {
                keys: [0, 1000]
                    .into_iter()
                    .map(|time_millis| crate::writer::NumericKeyframe {
                        time_millis,
                        values: vec![value],
                        easing: vec![crate::writer::KeyframeEasing::Linear],
                        spatial_in: Vec::new(),
                        spatial_out: Vec::new(),
                    })
                    .collect(),
            });
            let mut chunks = vec![
                Chunk::list(*b"Pefl", Vec::new()),
                effect_parade(&[effect], owner, [120.0, 80.0]).unwrap(),
            ];
            register_definitions(&mut chunks).unwrap();
            chunks
                .into_iter()
                .find(|chunk| chunk.list_kind() == Some(*b"EfdG"))
                .unwrap()
        }
        assert_eq!(registered(4.0, 13, true), registered(30.0, 42, false));
    }

    #[test]
    fn brightness_contrast_animated_scalar_uses_native_effect_record() {
        // AE-authored brightness_contrast_controls.aep uses a mode-4/subtype-6
        // scalar. Generic mode-8/subtype-9 keys read back correctly but Adobe
        // renders only their initial value (even after saving the project).
        let mut effect = new_effect("ADBE Brightness & Contrast 2", true, [120.0, 80.0]).unwrap();
        let brightness = effect
            .properties
            .iter_mut()
            .find(|property| property.match_name.ends_with("-0001"))
            .unwrap();
        brightness.animation = Some(Track {
            keys: [0, 1000]
                .into_iter()
                .map(|time_millis| crate::writer::NumericKeyframe {
                    time_millis,
                    values: vec![20.0],
                    easing: vec![crate::writer::KeyframeEasing::Linear],
                    spatial_in: Vec::new(),
                    spatial_out: Vec::new(),
                })
                .collect(),
        });
        let instance = plugin(&effect, 13, [120.0, 80.0]).unwrap();
        let group = instance
            .children()
            .unwrap()
            .iter()
            .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
            .unwrap();
        let properties = group.children().unwrap();
        let brightness_index = properties
            .iter()
            .position(|chunk| {
                chunk.id() == *b"tdmn"
                    && chunk.data_payload().is_some_and(|bytes| {
                        bytes.starts_with(b"ADBE Brightness & Contrast 2-0001")
                    })
            })
            .unwrap();
        let children = properties[brightness_index + 1].children().unwrap();
        assert_eq!(children[0].data_payload(), Some(&1_u32.to_be_bytes()[..]));
        let descriptor = children
            .iter()
            .find(|chunk| chunk.id() == *b"tdb4")
            .unwrap()
            .data_payload()
            .unwrap();
        assert_eq!(&descriptor[8..12], &u32::MAX.to_be_bytes());
        assert_eq!(&descriptor[56..61], &[0, 0, 0, 4, 6]);
        assert!(
            children
                .iter()
                .any(|chunk| chunk.list_kind() == Some(*b"list"))
        );
    }

    #[test]
    fn hue_master_fixed_preserves_signed_fractions_and_rejects_overflow() {
        assert_eq!(hue_master_fixed(-60.5).unwrap(), 0xffc3_8000);
        assert_eq!(hue_master_fixed(50.25).unwrap(), 0x0032_4000);
        assert_eq!(hue_master_fixed(-32_768.0).unwrap(), 0x8000_0000);
        assert_eq!(
            hue_master_fixed(f64::from(i32::MAX) / 65_536.0).unwrap(),
            0x7fff_ffff
        );
        for value in [
            32_768.0,
            -32_769.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert!(
                hue_master_fixed(value).is_err(),
                "must not wrap or saturate {value}"
            );
        }
    }

    #[test]
    fn hue_master_values_are_encoded_in_the_native_parameter_table() {
        // Adobe-authored with Colorize off and nonzero Master controls. The
        // observed native layout stores these values in parT/pard, unlike
        // generic scalar serialization. Matching it is structural evidence;
        // this test does not establish Adobe rendering of generated output.
        const SOURCE: &[u8] =
            include_bytes!("../../tests/fixtures/effects_coverage/hue_master_static_adobe.aep");

        fn master_value(panel: &Chunk, name: &str) -> Option<f64> {
            let table = panel
                .children()?
                .iter()
                .find(|chunk| chunk.list_kind() == Some(*b"parT"))?
                .children()?;
            let definitions = crate::properties::runs(table).ok()?;
            let (_, definition) = definitions.into_iter().find(|(id, _)| *id == name)?;
            let bytes = crate::properties::data(definition, *b"pard").ok()?;
            let fixed = i32::from_be_bytes(bytes.get(56..60)?.try_into().ok()?);
            Some(f64::from(fixed) / 65_536.0)
        }

        fn panels<'a>(chunks: &'a [Chunk], out: &mut Vec<&'a Chunk>) {
            for chunk in chunks {
                if chunk.list_kind() == Some(*b"sspc") {
                    out.push(chunk);
                }
                if let Some(children) = chunk.children() {
                    panels(children, out);
                }
            }
        }

        let source = crate::aep::Project::parse(SOURCE).unwrap();
        let mut found = Vec::new();
        panels(&source.chunks, &mut found);
        let hue = "ADBE HUE SATURATION";
        let source_panel = found
            .into_iter()
            .find(|panel| master_value(panel, "ADBE HUE SATURATION-0004") == Some(50.0))
            .expect("independent Adobe source with nonzero Master Hue");
        let mut effect = new_effect(hue, true, [120.0, 80.0]).unwrap();
        for (suffix, value) in [("0004", 50.0), ("0005", -60.0), ("0006", 20.0)] {
            let name = format!("{hue}-{suffix}");
            let property = effect
                .properties
                .iter_mut()
                .find(|property| property.match_name == name)
                .unwrap();
            property.values = vec![value];
            assert_eq!(master_value(source_panel, &name), Some(value));
        }
        let exported = plugin(&effect, 13, [120.0, 80.0]).unwrap();
        for suffix in ["0004", "0005", "0006"] {
            let name = format!("{hue}-{suffix}");
            assert_eq!(
                master_value(&exported, &name),
                master_value(source_panel, &name)
            );
        }

        fn registered(effect: NativeEffect) -> Chunk {
            let mut chunks = vec![
                Chunk::list(*b"Pefl", Vec::new()),
                effect_parade(&[effect], 13, [120.0, 80.0]).unwrap(),
            ];
            register_definitions(&mut chunks).unwrap();
            chunks
                .into_iter()
                .find(|chunk| chunk.list_kind() == Some(*b"EfdG"))
                .unwrap()
        }
        fn channel_range(panel: &Chunk, table: [u8; 4]) -> Vec<Chunk> {
            let children = panel
                .children()
                .unwrap()
                .iter()
                .find(|chunk| chunk.list_kind() == Some(table))
                .unwrap()
                .children()
                .unwrap();
            crate::properties::runs(children)
                .unwrap()
                .into_iter()
                .find(|(name, _)| *name == "ADBE HUE SATURATION-0003")
                .expect("native Channel Range state must be present")
                .1
                .to_vec()
        }
        // pard values alone were an insufficient oracle: Adobe renders the
        // Channel Range arbitrary state. Compare its descriptor and all 45
        // signed words with the independent source, not our own reader.
        assert_eq!(
            channel_range(&exported, *b"tdgp"),
            channel_range(source_panel, *b"tdgp")
        );
        assert_eq!(
            channel_range(&exported, *b"parT").last(),
            channel_range(source_panel, *b"parT").last()
        );
        let default = new_effect(hue, true, [120.0, 80.0]).unwrap();
        assert_eq!(
            registered(effect.clone()),
            registered(default.clone()),
            "global definitions must not contain owner values"
        );
        let default_panel = plugin(&default, 42, [120.0, 80.0]).unwrap();
        let default_range = channel_range(&default_panel, *b"tdgp");
        assert_eq!(
            default_range.last().unwrap().children().unwrap().first(),
            channel_range(source_panel, *b"parT").last(),
            "second owner's rendered state must retain independent native defaults"
        );
        for suffix in ["0004", "0005", "0006"] {
            assert_eq!(
                master_value(&default_panel, &format!("{hue}-{suffix}")),
                Some(0.0),
                "second owner must remain unchanged"
            );
        }
        let table = |panel: &Chunk| {
            panel
                .children()
                .unwrap()
                .iter()
                .find(|chunk| chunk.list_kind() == Some(*b"parT"))
                .unwrap()
                .clone()
        };
        let edited_table = table(&exported);
        let default_table = table(&default_panel);
        let edited = edited_table.children().unwrap();
        let original = default_table.children().unwrap();
        assert_eq!(edited.len(), original.len());
        assert_eq!(
            edited.iter().zip(original).filter(|(a, b)| a != b).count(),
            3,
            "only the three Master parameter records may differ"
        );
    }

    #[test]
    fn canonical_plugin_panel_can_be_freshly_serialized() {
        let directory =
            std::env::var_os("AEP_EFFECTS_NATIVE_PANEL_DIR").map(std::path::PathBuf::from);
        if let Some(path) = &directory {
            std::fs::create_dir_all(path).unwrap();
        }
        for (index, definition) in definitions::registry().iter().enumerate() {
            let solid = SolidLayerSpec {
                name: "Subject".into(),
                width: 120,
                height: 80,
                color: [0.9, 0.2, 0.1],
                transform: SolidTransform {
                    anchor: [60.0, 40.0],
                    position: [160.0, 90.0],
                    scale: [100.0, 100.0],
                    rotation: 0.0,
                    opacity: 100.0,
                },
            };
            let effect = new_effect(&definition.match_name, true, [120.0, 80.0]).unwrap();
            let options = NativeLayerOptions {
                fx_id: fx_schema::LayerId::new(1),
                parent: None,
                matte: None,
                enabled: true,
                adjustment_layer: false,
                motion_blur: false,
                blend_mode: 2,
                masks: vec![],
                effects: vec![effect],
                styles: Vec::new(),
                source_clock: None,
                transform_3d: None,
            };
            let bytes = crate::writer::write_composition(
                &CompositionSpec {
                    name: "FreshEffect".into(),
                    width: 320,
                    height: 180,
                    duration_frames: 48,
                },
                &[LayerSpec::Options(
                    Box::new(LayerSpec::Solid(solid)),
                    options,
                )],
            )
            .unwrap();
            let project = crate::structure::read_project(&bytes).unwrap();
            let crate::structure::ItemKind::Composition(comp) = &project.item(1).unwrap().kind
            else {
                panic!("composition")
            };
            let (effects, _) =
                crate::effects::native::read_effects(&comp.layers[0].content, [120.0, 80.0]);
            assert_eq!(effects[0].match_name, definition.match_name);
            if let Some(path) = &directory {
                std::fs::write(path.join(format!("effect-{index:02}.aep")), bytes).unwrap();
                std::fs::write(
                    path.join(format!("effect-{index:02}.name")),
                    &definition.match_name,
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn fresh_gaussian_plugin_has_editable_keys_and_optional_adobe_probe() {
        let mut effect = new_effect("ADBE Gaussian Blur 2", true, [120.0, 80.0]).unwrap();
        let blur = effect
            .properties
            .iter_mut()
            .find(|p| p.match_name.ends_with("-0001"))
            .unwrap();
        blur.values = vec![4.0];
        blur.animation = Some(Track {
            keys: [4.0, 18.0]
                .into_iter()
                .enumerate()
                .map(|(i, value)| super::super::NumericKeyframe {
                    time_millis: i64::try_from(i).unwrap() * 1000,
                    values: vec![value],
                    easing: vec![super::super::KeyframeEasing::Linear],
                    spatial_in: vec![],
                    spatial_out: vec![],
                })
                .collect(),
        });
        let solid = SolidLayerSpec {
            name: "Subject".into(),
            width: 120,
            height: 80,
            color: [0.9, 0.2, 0.1],
            transform: SolidTransform {
                anchor: [60.0, 40.0],
                position: [160.0, 90.0],
                scale: [100.0, 100.0],
                rotation: 0.0,
                opacity: 100.0,
            },
        };
        let mut options = NativeLayerOptions {
            fx_id: fx_schema::LayerId::new(1),
            parent: None,
            matte: None,
            enabled: true,
            adjustment_layer: false,
            motion_blur: false,
            blend_mode: 2,
            masks: vec![],
            effects: vec![effect],
            styles: Vec::new(),
            source_clock: None,
            transform_3d: None,
        };
        let bytes = crate::writer::write_composition(
            &CompositionSpec {
                name: "FreshEffect".into(),
                width: 320,
                height: 180,
                duration_frames: 48,
            },
            &[LayerSpec::Options(
                Box::new(LayerSpec::Solid(solid.clone())),
                options.clone(),
            )],
        )
        .unwrap();
        let project = crate::structure::read_project(&bytes).unwrap();
        let crate::structure::ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
            panic!("composition")
        };
        let (effects, warnings) =
            crate::effects::native::read_effects(&comp.layers[0].content, [120.0, 80.0]);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(effects.len(), 1);
        let blur = effects[0]
            .parameters
            .iter()
            .find(|p| p.match_name.ends_with("-0001"))
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        assert_eq!(blur.keyframes[1].values, vec![18.0]);
        fn owner_references(chunks: &[Chunk], output: &mut Vec<u32>) {
            for chunk in chunks {
                if chunk.id() == *b"tdpi" {
                    output.push(u32::from_be_bytes(
                        chunk.data_payload().unwrap().try_into().unwrap(),
                    ));
                }
                if let Some(children) = chunk.children() {
                    owner_references(children, output);
                }
            }
        }
        let mut owners = Vec::new();
        owner_references(&comp.layers[0].content, &mut owners);
        assert_eq!(
            owners,
            [comp.layers[0].record.id()],
            "hidden plugin root refers to its freshly allocated owner, not fixture layer 15"
        );
        let native_blur = options.effects[0]
            .properties
            .iter()
            .find(|property| property.match_name.ends_with("-0001"))
            .unwrap();
        assert_eq!(
            native_blur.bounds,
            Some((0.0, 50.0)),
            "native floating slider requires its declared UI bounds"
        );
        if let Some(path) = std::env::var_os("AEP_EFFECTS_PROBE_DIR") {
            let path = std::path::PathBuf::from(path);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("fresh-gaussian.aep"), bytes).unwrap();
            let spec = CompositionSpec {
                name: "FreshEffect".into(),
                width: 320,
                height: 180,
                duration_frames: 48,
            };
            for property in &mut options.effects[0].properties {
                property.animation = None;
            }
            let static_bytes = crate::writer::write_composition(
                &spec,
                &[LayerSpec::Options(
                    Box::new(LayerSpec::Solid(solid.clone())),
                    options,
                )],
            )
            .unwrap();
            std::fs::write(path.join("fresh-gaussian-static.aep"), static_bytes).unwrap();
            let plain_bytes = crate::writer::write_solid_composition(&spec, &[solid]).unwrap();
            std::fs::write(path.join("fresh-no-effects.aep"), plain_bytes).unwrap();
        }
    }
}
