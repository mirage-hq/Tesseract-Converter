//! Source-based reproduction of native editable PolyStar enclosure omissions.
use super::*;

#[test]
fn static_polystar_native_opacity_group_retains_geometry_and_sibling() {
    let source = read_project(include_bytes!(
        "../../../tests/fixtures/static_polystar_enclosure/source.aep"
    ))
    .unwrap();
    for composition_id in [1, 14] {
        let mut input = to_structural_fx_document(&source, Some(composition_id))
            .unwrap()
            .document
            .to_json_value()
            .unwrap();
        fn star(layers: &[Layer]) -> Option<&fx_schema::layer::ShapePolyStar> {
            layers.iter().find_map(|layer| match layer.data() {
                LayerData::Shape(shape) => shape.shape.poly_star.as_ref(),
                LayerData::Group(group) => star(&group.layers),
                _ => None,
            })
        }
        let document = EditableFxCompositionDocument::from_json_value(input.clone()).unwrap();
        let geometry = star(document.composition().layers()).expect("native PolyStar source");
        assert_eq!(geometry.points, 5.0);
        assert_eq!(geometry.outer_radius, 210.0);
        assert_eq!(geometry.outer_roundness, 0.0);
        assert_eq!(geometry.inner_roundness, 0.0);
        let group = &mut input["composition"]["layers"][0];
        assert!(group["layers"].is_array(), "native composition Group");
        group["transform"]["opacity"] = json!(50.0);
        fn edit_radius(layer: &mut Value) {
            if layer["shape"]["polyStar"].is_object() {
                layer["shape"]["polyStar"]["outerRadius"] = json!(125.0);
            }
            if let Some(children) = layer["layers"].as_array_mut() {
                for child in children {
                    edit_radius(child);
                }
            }
        }
        edit_radius(group);
        let group_name = group["name"].as_str().unwrap().to_owned();
        let sibling = rect(&imported(), 900_182);
        let sibling_name = sibling["name"].as_str().unwrap().to_owned();
        input["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .push(sibling);
        let output = export(input);
        {
            let directory = crate::adobe_test_support::artifact_directory();
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(
                std::path::Path::new(&directory).join(format!("polystar-{composition_id}.aep")),
                &output.bytes,
            )
            .unwrap();
        }
        let native = read_project(&output.bytes).unwrap();
        assert_eq!(
            layers(&native).len(),
            2,
            "{composition_id}: {:?}",
            output.diagnostics
        );
        assert_eq!(layers(&native)[0].name.as_ref(), group_name);
        assert_eq!(layers(&native)[1].name.as_ref(), sibling_name);
        let reimported = to_structural_fx_document(&native, Some(1)).unwrap();
        let actual =
            star(reimported.document.composition().layers()).expect("editable exported PolyStar");
        assert_eq!(actual.points, 5.0);
        assert_eq!(actual.outer_radius, 125.0);
    }
}

#[test]
fn static_polystar_enclosure_keeps_unproved_profiles_guarded() {
    let source = read_project(include_bytes!(
        "../../../tests/fixtures/static_polystar_enclosure/source.aep"
    ))
    .unwrap();
    for (field, replacement) in [
        ("points", 5.5),
        ("points", 2.0),
        ("points", 1001.0),
        ("outerRoundness", 20.0),
        ("innerRoundness", 20.0),
        ("innerRadius", -1.0),
        ("outerRadius", -1.0),
        ("outerRadius", 1e308),
    ] {
        let mut input = to_structural_fx_document(&source, Some(1))
            .unwrap()
            .document
            .to_json_value()
            .unwrap();
        fn replace(layer: &mut Value, field: &str, replacement: f64) {
            if layer["shape"]["polyStar"].is_object() {
                layer["shape"]["polyStar"][field] = json!(replacement);
            }
            if let Some(children) = layer["layers"].as_array_mut() {
                for child in children {
                    replace(child, field, replacement);
                }
            }
        }
        let group = &mut input["composition"]["layers"][0];
        group["transform"]["opacity"] = json!(50.0);
        replace(group, field, replacement);
        let sibling = rect(&imported(), 900_183);
        input["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .push(sibling);
        let output = export(input);
        let native = read_project(&output.bytes).unwrap();
        // A finite but oversized enclosure may retain its owner Groups through
        // the bounded viewport; the unproved PolyStar itself is never emitted.
        let expected_roots = if field == "outerRadius" && replacement > 0.0 {
            2
        } else {
            1
        };
        assert_eq!(
            layers(&native).len(),
            expected_roots,
            "{field}={replacement}: {:?}",
            output.diagnostics
        );
        fn has_star(layers: &[Layer]) -> bool {
            layers.iter().any(|layer| match layer.data() {
                LayerData::Shape(shape) => shape.shape.poly_star.is_some(),
                LayerData::Group(group) => has_star(&group.layers),
                _ => false,
            })
        }
        let reimported = to_structural_fx_document(&native, Some(1)).unwrap();
        assert!(
            !has_star(reimported.document.composition().layers()),
            "{field}={replacement}: unproved PolyStar emitted"
        );
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.message.contains("omitted") })
        );
    }
}
