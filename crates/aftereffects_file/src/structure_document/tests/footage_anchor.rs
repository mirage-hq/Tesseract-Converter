//! File-footage Anchor units: native import evidence, not render-fidelity proof.

use super::*;

#[test]
fn native_file_footage_absent_anchor_retains_pixel_center() {
    // This independent native fixture omits the Anchor leaf; it is a default
    // boundary oracle, not proof of explicitly stored fractional values.
    let project = read_project(include_bytes!(
        "../../../tests/fixtures/pr4442_native/sources/media_video.aep"
    ))
    .unwrap();
    let (comp_id, layer) = project
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some((item.id, comp)),
            _ => None,
        })
        .flat_map(|(id, comp)| comp.layers.iter().map(move |layer| (id, layer)))
        .find(|(_, layer)| {
            project
                .item(layer.record.source_id())
                .is_some_and(|source| {
                    source.footage.as_ref().is_some_and(|footage| {
                        footage.main_source == crate::structure::FootageSourceKind::File
                    })
                })
        })
        .unwrap();
    assert_eq!(
        crate::properties::read_static_source_relative_anchor(&layer.content).unwrap(),
        None
    );
    let media = project
        .item(layer.record.source_id())
        .unwrap()
        .media
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    let converted =
        to_structural_fx_document_with_assets(&project, Some(comp_id), &mut |_| true).unwrap();
    let occurrence = as_group(&root(&converted).layers[0]);
    assert_eq!(
        occurrence.transform.anchor_point,
        [f64::from(media.width) / 2.0, f64::from(media.height) / 2.0]
    );
}

#[test]
fn supplemental_file_footage_anchor_uses_dimensions_not_value_magnitude() {
    // Synthetic property edits supplement the pinned local native assertion.
    let mut project = read_project(include_bytes!(
        "../../../tests/fixtures/pr4442_native/sources/media_video.aep"
    ))
    .unwrap();
    let comp_id = project
        .items
        .iter()
        .find(|item| matches!(item.kind, ItemKind::Composition(_)))
        .unwrap()
        .id;
    let layer = &mut composition_mut(&mut project, comp_id).layers[0];
    let source_id = layer.record.source_id();
    // The helper encodes XY fractions using 1920x1080, independently of
    // this footage's dimensions. Out-of-bounds anchors remain legal.
    set_static_transform(
        layer,
        &[
            ("ADBE Anchor Point", &[2400.0, -270.0, 7.0]),
            ("ADBE Position", &[12.0, 34.0, 0.0]),
        ],
    );
    let source = project.item(source_id).unwrap();
    let media = source.media.as_ref().unwrap().as_ref().unwrap();
    let dimensions = [media.width, media.height];
    assert_eq!(
        solid_anchor_scale(Some(source), dimensions),
        dimensions.map(f64::from),
        "static and keyed Anchor share the same source-owned XY scale"
    );
    let converted =
        to_structural_fx_document_with_assets(&project, Some(comp_id), &mut |_| true).unwrap();
    let occurrence = as_group(&root(&converted).layers[0]);
    assert_eq!(
        occurrence.transform.anchor_point,
        [
            f64::from(dimensions[0]) * 1.25,
            f64::from(dimensions[1]) * -0.25
        ]
    );
    assert_eq!(
        occurrence.transform.position,
        fx_schema::Position::TwoD([12.0, 34.0])
    );
}

#[test]
fn supplemental_keyed_file_footage_anchor_scales_xy_tracks() {
    // Reassign a native Solid's keyed Anchor to a File source. This is a
    // supplementary importer boundary test, not native keyed-footage proof.
    let mut project = read_project(include_bytes!(
        "../../../tests/fixtures/properties/import_numeric_animation_cases.aep"
    ))
    .unwrap();
    let footage = read_project(include_bytes!(
        "../../../tests/fixtures/pr4442_native/sources/media_video.aep"
    ))
    .unwrap();
    let mut source = footage
        .items
        .iter()
        .find(|item| {
            item.footage.as_ref().is_some_and(|footage| {
                footage.main_source == crate::structure::FootageSourceKind::File
            })
        })
        .unwrap()
        .clone();
    let layer = &composition(&project, 50).layers[0];
    let source_id = layer.record.source_id();
    let numeric = crate::properties::read_transform(&layer.content)
        .unwrap()
        .into_iter()
        .find(|property| property.match_name == "ADBE Anchor Point")
        .unwrap()
        .numeric
        .unwrap();
    assert_eq!(numeric.keyframes.len(), 2);
    let media = source.media.as_ref().unwrap().as_ref().unwrap();
    let dimensions = [media.width, media.height];
    source.id = source_id;
    *project
        .items
        .iter_mut()
        .find(|item| item.id == source_id)
        .unwrap() = source;
    let converted =
        to_structural_fx_document_with_assets(&project, Some(50), &mut |_| true).unwrap();
    let json: Value = serde_json::from_slice(&converted.document.to_json_vec().unwrap()).unwrap();
    let entries = json["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    for (axis, property) in ["anchorPointX", "anchorPointY"].into_iter().enumerate() {
        let entry = entries
            .iter()
            .find(|entry| entry["target"]["propertyType"] == property)
            .unwrap();
        let keys = entry["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(keys.len(), 2);
        for (key, native) in keys.iter().zip(&numeric.keyframes) {
            assert_eq!(
                key["layerTime"].as_f64().unwrap(),
                native.time_secs * 1000.0
            );
            assert_eq!(
                key["value"]["value"].as_f64().unwrap(),
                native.values[axis] * f64::from(dimensions[axis])
            );
        }
    }
    assert!(
        entries
            .iter()
            .all(|entry| entry["target"]["propertyType"] != "anchorPointZ")
    );
}
