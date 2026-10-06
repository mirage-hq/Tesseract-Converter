//! Bounded static same-composition Point Control copies; no expression evaluator.

use super::{finished, quoted, token, unique_run};
use crate::{
    properties::{self, NumericValueKind, PropertyError},
    rifx::Chunk,
    structure::Composition,
};

const KIND: &str = "ADBE Point Control";
const VALUE: &str = "ADBE Point Control-0001";

fn reference(mut text: &str) -> Result<(&str, &str), PropertyError> {
    token(&mut text, "thisComp")
        .and_then(|_| token(&mut text, "."))
        .and_then(|_| token(&mut text, "layer"))
        .and_then(|_| token(&mut text, "("))
        .ok_or(PropertyError::Layout(
            "not a same-composition Point Control alias",
        ))?;
    let layer =
        quoted(&mut text).ok_or(PropertyError::Layout("invalid Point Control layer name"))?;
    token(&mut text, ")")
        .and_then(|_| token(&mut text, "."))
        .and_then(|_| token(&mut text, "effect"))
        .and_then(|_| token(&mut text, "("))
        .ok_or(PropertyError::Layout("invalid Point Control effect access"))?;
    let effect =
        quoted(&mut text).ok_or(PropertyError::Layout("invalid Point Control effect name"))?;
    token(&mut text, ")")
        .and_then(|_| token(&mut text, "("))
        .ok_or(PropertyError::Layout(
            "invalid Point Control parameter access",
        ))?;
    let parameter =
        quoted(&mut text).ok_or(PropertyError::Layout("invalid Point Control parameter"))?;
    if parameter != VALUE || token(&mut text, ")").is_none() || !finished(text) {
        return Err(PropertyError::Layout(
            "not a complete native Point Control alias",
        ));
    }
    Ok((layer, effect))
}

pub(super) fn resolve(
    property: &[Chunk],
    composition: &Composition,
    consumer_id: u32,
) -> Result<[f64; 2], PropertyError> {
    let (name, effect_name) = reference(super::expression(property)?)?;
    let mut layers = composition
        .layers
        .iter()
        .filter(|layer| layer.name.as_ref() == name);
    let source = layers
        .next()
        .ok_or(PropertyError::Layout("Point Control layer missing"))?;
    if layers.next().is_some() || source.record.id() == consumer_id {
        return Err(PropertyError::Layout(
            "ambiguous or self Point Control layer",
        ));
    }
    selected(composition, source, effect_name)
}

/// Read a selected native controller without changing its chunks or occurrence.
pub(super) fn selected(
    composition: &Composition,
    source: &crate::structure::Layer,
    effect_name: &str,
) -> Result<[f64; 2], PropertyError> {
    read_selected(composition, source, effect_name, true)
}

/// The policy route also admits explicitly typed native instances without a
/// local value declaration. It must never infer defaults from that absence.
pub(super) fn selected_with_sparse_explicit(
    composition: &Composition,
    source: &crate::structure::Layer,
    effect_name: &str,
) -> Result<[f64; 2], PropertyError> {
    read_selected(composition, source, effect_name, false)
}

fn read_selected(
    composition: &Composition,
    source: &crate::structure::Layer,
    effect_name: &str,
    require_local_declaration: bool,
) -> Result<[f64; 2], PropertyError> {
    // Native readback establishes the canvas plane for planar source-less Shape
    // controllers. Do not guess a Null, Text, raster or 3D controller's plane.
    if source.record.layer_type() != 4
        || source.record.source_id() != 0
        || source.record.flags().three_d_layer
        || composition.width == 0
        || composition.height == 0
    {
        return Err(PropertyError::Layout(
            "Point Control owner plane is not established",
        ));
    }
    let roots = properties::root_runs(&source.content)?;
    let parade = unique_run(&roots, "ADBE Effect Parade")?;
    let effects = properties::runs(properties::unique_list(parade, *b"tdgp")?)?;
    let mut target = None;
    for (kind, run) in &effects {
        let plugin = properties::unique_list(run, *b"sspc")?;
        let body = properties::unique_list(plugin, *b"tdgp")?;
        if super::effect_name(plugin, body) == Some(effect_name)
            && target.replace((*kind, plugin, body)).is_some()
        {
            return Err(PropertyError::Layout("ambiguous Point Control effect"));
        }
    }
    let (kind, plugin, body) =
        target.ok_or(PropertyError::Layout("Point Control effect missing"))?;
    if kind != KIND {
        return Err(PropertyError::Layout("not a native Point Control"));
    }
    let declarations = properties::runs(properties::unique_list(plugin, *b"parT")?)?;
    let matching: Vec<_> = declarations
        .iter()
        .filter(|(name, _)| *name == VALUE)
        .collect();
    let declaration = match matching.as_slice() {
        [] if !require_local_declaration => None,
        [] => {
            return Err(PropertyError::Layout(
                "local Point Control declaration missing",
            ));
        }
        [(_, run)] => {
            let bytes = properties::data(run, *b"pard")?;
            if bytes.len() != 148 || crate::effects::native::declaration_kind(run)? != 6 {
                return Err(PropertyError::Layout(
                    "invalid local Point Control declaration",
                ));
            }
            Some(bytes)
        }
        _ => {
            return Err(PropertyError::Layout(
                "ambiguous local Point Control declaration",
            ));
        }
    };
    let plane = [f64::from(composition.width), f64::from(composition.height)];
    let controls = properties::runs(body)?;
    let matching: Vec<_> = controls.iter().filter(|(name, _)| *name == VALUE).collect();
    match matching.as_slice() {
        [] => percentage_default(
            declaration.ok_or(PropertyError::Layout(
                "Point Control defaults require a local declaration",
            ))?,
            plane,
        ),
        [(_, run)] => {
            let leaf = properties::unique_list(run, *b"tdbs")?;
            let meta = properties::data(leaf, *b"tdb4")?;
            // Admit only the native plugin Point leaf whose decoder already
            // validates its two-component spatial representation.
            if meta.len() != 124 || meta[59] != 4 {
                return Err(PropertyError::Layout(
                    "unestablished explicit Point Control storage",
                ));
            }
            let numeric = properties::read_effect_point(leaf)?;
            if numeric.animated
                || !numeric.keyframes.is_empty()
                || numeric.expression_enabled
                || numeric.dimensions_separated
                || numeric.value_kind != NumericValueKind::Continuous
            {
                return Err(PropertyError::Layout(
                    "Point Control must be static and expression-free",
                ));
            }
            let [x, y] = numeric.values.as_slice() else {
                return Err(PropertyError::Layout(
                    "Point Control requires two components",
                ));
            };
            let point = [x * plane[0], y * plane[1]];
            if point.iter().any(|value| !value.is_finite()) {
                return Err(PropertyError::NonFinite);
            }
            Ok(point)
        }
        _ => Err(PropertyError::Layout(
            "duplicate explicit Point Control storage",
        )),
    }
}

fn percentage_default(bytes: &[u8], plane: [f64; 2]) -> Result<[f64; 2], PropertyError> {
    // PF_PointDef dephaults are signed 16.16 percentages, independently verified
    // by AE26.5x89 readback (50/50 -> [960,540] on a 1920x1080 canvas).
    // This rule is local to Point Control; other point defaults are unchanged.
    let mut point = [0.0; 2];
    for (index, value) in point.iter_mut().enumerate() {
        let offset = 56 + 4 * index;
        let raw = i32::from_be_bytes(
            bytes
                .get(offset..offset + 4)
                .and_then(|bytes| bytes.try_into().ok())
                .ok_or(PropertyError::Layout("Point Control default missing"))?,
        );
        *value = f64::from(raw) / 65_536.0 / 100.0 * plane[index];
    }
    if point.iter().any(|value| !value.is_finite()) {
        return Err(PropertyError::NonFinite);
    }
    Ok(point)
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::tests::{data, list, name, numeric};
    use super::*;
    use crate::{
        schema::layer_records::LayerRecord,
        structure::{ItemKind, read_project},
    };

    pub(in crate::structure_document::control_links) fn controller(
        explicit: Option<Chunk>,
    ) -> Composition {
        // Supplementary profile built on a real native layer envelope. This is
        // not an independently Adobe-authored Point feature fixture.
        let project = read_project(include_bytes!(
            "../../../tests/fixtures/properties/property_1D_opacity.aep"
        ))
        .unwrap();
        let mut composition = project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(comp) => Some((**comp).clone()),
                _ => None,
            })
            .unwrap();
        composition.width = 3260;
        composition.height = 533;
        let mut layer = composition.layers[0].clone();
        let mut bytes = layer.record.encode();
        bytes[..4].copy_from_slice(&367_u32.to_be_bytes());
        bytes[40..44].copy_from_slice(&0_u32.to_be_bytes());
        bytes[131] = 4;
        layer.record = LayerRecord::decode(&bytes)
            .unwrap()
            .with_three_d_layer(false)
            .unwrap();
        layer.name = "Controller".into();
        let mut declaration = vec![0; 148];
        declaration[12..16].copy_from_slice(&6_u32.to_be_bytes());
        for offset in [56, 60] {
            declaration[offset..offset + 4].copy_from_slice(&(50_i32 * 65_536).to_be_bytes());
        }
        let mut controls = vec![name("Center")];
        if let Some(explicit) = explicit {
            controls.extend([data(b"tdmn", VALUE.as_bytes()), explicit]);
        }
        layer.content = vec![list(
            b"tdgp",
            vec![
                data(b"tdmn", b"ADBE Effect Parade"),
                list(
                    b"tdgp",
                    vec![
                        data(b"tdmn", KIND.as_bytes()),
                        list(
                            b"sspc",
                            vec![
                                list(
                                    b"parT",
                                    vec![
                                        data(b"tdmn", VALUE.as_bytes()),
                                        data(b"pard", declaration),
                                    ],
                                ),
                                list(b"tdgp", controls),
                            ],
                        ),
                    ],
                ),
            ],
        )];
        composition.layers = vec![layer];
        composition
    }

    pub(in crate::structure_document::control_links) fn point(
        values: &[f64],
        expression: Option<&str>,
    ) -> Chunk {
        let mut leaf = numeric(values, expression);
        let meta = &mut leaf.children_mut().unwrap()[0];
        let mut bytes = meta.data_payload().unwrap().to_vec();
        bytes[59] = 4;
        *meta = data(b"tdb4", bytes);
        leaf
    }

    fn alias() -> Chunk {
        numeric(
            &[0.0, 0.0],
            Some("thisComp.layer(\"Controller\").effect(\"Center\")(\"ADBE Point Control-0001\")"),
        )
    }

    #[test]
    fn point_alias_explicit_precedence_owner_plane_and_occurrence_isolation() {
        let property = alias();
        let base = controller(None);
        assert_eq!(
            resolve(property.children().unwrap(), &base, 371).unwrap(),
            [1630.0, 266.5]
        );
        // w10 Logo14's explicit vector and non-square owning canvas: neither
        // sparse 50/50 nor a root 3840x2160 canvas may overwrite these values.
        let occurrence = controller(Some(point(&[0.05338078291814947, 0.0], None)));
        let xy = resolve(property.children().unwrap(), &occurrence, 371).unwrap();
        assert!((xy[0] - 174.02135231316726).abs() < 1e-10);
        assert_eq!(xy[1], 0.0);
        // Disabled native expression text does not change a static authored
        // point. Enabled programs remain rejected, without a runtime evaluator.
        let mut disabled = point(&[0.25, 0.75], Some("value + [1,2]"));
        let meta = &mut disabled.children_mut().unwrap()[0];
        let mut bytes = meta.data_payload().unwrap().to_vec();
        bytes[119] = 1;
        *meta = data(b"tdb4", bytes);
        assert_eq!(
            resolve(
                property.children().unwrap(),
                &controller(Some(disabled)),
                371
            )
            .unwrap(),
            [815.0, 399.75]
        );
        let zero = controller(Some(point(&[0.0, 0.0], None)));
        assert_eq!(
            resolve(property.children().unwrap(), &zero, 371).unwrap(),
            [0.0, 0.0]
        );
        assert_eq!(
            resolve(property.children().unwrap(), &base, 371).unwrap(),
            [1630.0, 266.5]
        );
    }

    #[test]
    fn point_alias_invalid_targets_decline_without_default_or_cache_fallback() {
        let property = alias();
        for explicit in [
            point(&[f64::NAN, 0.0], None),
            point(&[0.5], None),
            point(&[0.5, 0.5], Some("value + [1,2]")),
            numeric(&[0.5, 0.5], None),
        ] {
            assert!(
                resolve(
                    property.children().unwrap(),
                    &controller(Some(explicit)),
                    371
                )
                .is_err()
            );
        }
        let base = controller(None);
        assert!(resolve(property.children().unwrap(), &base, 367).is_err());
        let mut missing = base.clone();
        missing.layers.clear();
        assert!(resolve(property.children().unwrap(), &missing, 371).is_err());
        let mut duplicate_effect = base.clone();
        let parade = duplicate_effect.layers[0].content[0]
            .children_mut()
            .unwrap()[1]
            .children_mut()
            .unwrap();
        parade.extend(parade.clone());
        assert!(resolve(property.children().unwrap(), &duplicate_effect, 371).is_err());
        for mode in 0..3 {
            let mut changed = base.clone();
            let plugin = changed.layers[0].content[0].children_mut().unwrap()[1]
                .children_mut()
                .unwrap()[1]
                .children_mut()
                .unwrap();
            let declarations = plugin[0].children_mut().unwrap();
            match mode {
                0 => declarations.extend(declarations.clone()),
                1 => declarations[1] = data(b"pard", vec![0; 147]),
                _ => declarations[1] = data(b"pard", vec![0; 148]),
            }
            assert!(resolve(property.children().unwrap(), &changed, 371).is_err());
        }
        let mut duplicate_point = controller(Some(point(&[0.5, 0.5], None)));
        let plugin = duplicate_point.layers[0].content[0].children_mut().unwrap()[1]
            .children_mut()
            .unwrap()[1]
            .children_mut()
            .unwrap();
        let body = plugin[1].children_mut().unwrap();
        body.extend(body[1..].to_vec());
        assert!(resolve(property.children().unwrap(), &duplicate_point, 371).is_err());
        let mut duplicate = base.clone();
        duplicate.layers.push(duplicate.layers[0].clone());
        assert!(resolve(property.children().unwrap(), &duplicate, 371).is_err());
        for (kind, source, three_d) in [
            (0, 100, false),
            (3, 0, false),
            (4, 0, true),
            (4, 100, false),
        ] {
            let mut changed = base.clone();
            let mut bytes = changed.layers[0].record.encode();
            bytes[131] = kind;
            bytes[40..44].copy_from_slice(&u32::to_be_bytes(source));
            changed.layers[0].record = LayerRecord::decode(&bytes)
                .unwrap()
                .with_three_d_layer(three_d)
                .unwrap();
            assert!(resolve(property.children().unwrap(), &changed, 371).is_err());
        }
        // Present malformed explicit data never falls through to valid defaults.
        assert!(
            resolve(
                property.children().unwrap(),
                &controller(Some(list(b"tdbs", vec![]))),
                371
            )
            .is_err()
        );
        for (offset, value) in [(68, 1), (119, 0)] {
            let mut leaf = point(&[0.5, 0.5], None);
            let meta = &mut leaf.children_mut().unwrap()[0];
            let mut bytes = meta.data_payload().unwrap().to_vec();
            bytes[offset] = value;
            if offset == 119 {
                bytes[120] = 1;
            }
            *meta = data(b"tdb4", bytes);
            assert!(resolve(property.children().unwrap(), &controller(Some(leaf)), 371).is_err());
        }
        let mut leaf = point(&[0.5, 0.5], None);
        leaf.children_mut().unwrap()[1] = data(b"tdsb", vec![0, 0, 8, 1]);
        assert!(resolve(property.children().unwrap(), &controller(Some(leaf)), 371).is_err());
    }

    #[test]
    fn complete_point_alias_grammar_rejects_programs_and_other_parameters() {
        let alias =
            "thisComp.layer(\"Controller\").effect(\"Center\")(\"ADBE Point Control-0001\")";
        assert_eq!(reference(alias).unwrap(), ("Controller", "Center"));
        for expression in [
            format!("{alias} + [1,2]"),
            format!("{alias}.value"),
            alias.replace("thisComp", "comp(\"Other\")"),
            alias.replace(VALUE, "Point"),
            format!("var p = {alias}; p"),
            format!("{alias}; value"),
        ] {
            assert!(reference(&expression).is_err(), "{expression}");
        }
    }
    #[test]
    fn point_alias_defaults_are_percentages_not_global_normalized_defaults() {
        let mut bytes = [0; 148];
        bytes[56..60].copy_from_slice(&(50_i32 * 65_536).to_be_bytes());
        bytes[60..64].copy_from_slice(&(25_i32 * 65_536).to_be_bytes());
        assert_eq!(
            percentage_default(&bytes, [1920.0, 1080.0]).unwrap(),
            [960.0, 270.0]
        );
        assert!(percentage_default(&bytes[..60], [1920.0, 1080.0]).is_err());
    }
}
