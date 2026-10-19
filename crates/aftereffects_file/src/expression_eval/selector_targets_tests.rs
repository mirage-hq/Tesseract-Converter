use super::*;
use crate::{rifx::Chunk, structure::ItemKind};

fn enable_expression(chunks: &mut [Chunk], target: &str, code: &str) -> usize {
    let mut changed = 0;
    let mut name = String::new();
    for chunk in chunks {
        if chunk.id() == *b"tdmn" {
            name = String::from_utf8_lossy(chunk.data_payload().unwrap())
                .trim_end_matches('\0')
                .to_owned();
        }
        let property = chunk.list_kind() == Some(*b"tdbs") && name == target;
        if let Some(children) = chunk.children_mut() {
            if property {
                let meta = children.iter_mut().find(|c| c.id() == *b"tdb4").unwrap();
                let mut bytes = meta.data_payload().unwrap().to_vec();
                bytes[119] &= !1;
                bytes[120] |= 1;
                *meta = Chunk::data(*b"tdb4", bytes).unwrap();
                children.retain(|c| !matches!(&c.id(), b"Utf8" | b"expr"));
                children.push(Chunk::data(*b"Utf8", code.as_bytes()).unwrap());
                changed += 1;
            } else {
                changed += enable_expression(children, target, code);
            }
        }
    }
    changed
}

fn marker_property() -> Vec<Chunk> {
    let mut meta = vec![0; 124];
    meta[..8].copy_from_slice(&[0xdb, 0x99, 0, 1, 0, 0, 0, 0]);
    meta[12..16].copy_from_slice(&30000_u32.to_be_bytes());
    let mut header = vec![0; 52];
    header[..4].copy_from_slice(&[0, 0xd0, 0x0b, 0xee]);
    header[11] = 1;
    header[19] = 16;
    header[23] = 4;
    let mut key = vec![0; 16];
    key[..4].copy_from_slice(&15000_i32.to_be_bytes());
    let mut name = b"ADBE Marker".to_vec();
    name.resize(40, 0);
    let mut value = vec![Chunk::data(*b"NmHd", vec![0; 20]).unwrap()];
    for text in ["reveal", "", "", "", ""] {
        value.push(Chunk::data(*b"Utf8", text.as_bytes()).unwrap());
    }
    vec![
        Chunk::data(*b"tdmn", name).unwrap(),
        Chunk::list(
            *b"mrst",
            vec![
                Chunk::list(
                    *b"tdbs",
                    vec![
                        Chunk::data(*b"tdb4", meta).unwrap(),
                        Chunk::list(
                            *b"list",
                            vec![
                                Chunk::data(*b"lhd3", header).unwrap(),
                                Chunk::data(*b"ldat", key).unwrap(),
                            ],
                        ),
                    ],
                ),
                Chunk::list(*b"mrky", vec![Chunk::list(*b"Nmrd", value)]),
            ],
        ),
    ]
}

#[test]
fn range_selector_expressions_become_editable_percentage_and_index_tracks() {
    let source = include_bytes!("../../tests/fixtures/text/import_selector_animation.aep");
    for (comp_id, native_name, target_name, scale) in [
        (1, "ADBE Text Percent Start", "start", 0.01),
        (14, "ADBE Text Percent End", "end", 0.01),
        (27, "ADBE Text Percent Offset", "offset", 0.01),
        (40, "ADBE Text Index Start", "start", 1.0),
        (53, "ADBE Text Index End", "end", 1.0),
        (66, "ADBE Text Index Offset", "offset", 1.0),
        (79, "ADBE Text Selector Max Amount", "amount", 0.01),
    ] {
        let mut project = crate::structure::read_project(source).unwrap();
        let item = project
            .items
            .iter_mut()
            .find(|item| item.id == comp_id)
            .unwrap();
        let ItemKind::Composition(comp) = &mut item.kind else {
            panic!("composition expected")
        };
        // Mutated public fixture: converter regression, not an Adobe expression oracle.
        assert_eq!(
            enable_expression(
                &mut comp.layers[0].content,
                native_name,
                "var trigger=thisLayer.marker.key('reveal').time; (time-trigger)*20+10;"
            ),
            1
        );
        comp.layers[0]
            .content
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
            .unwrap()
            .children_mut()
            .unwrap()
            .extend(marker_property());
        let converted =
            crate::structure_document::to_structural_fx_document(&project, Some(comp_id)).unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&converted.document.to_json_vec().unwrap()).unwrap();
        fn selector_id(value: &serde_json::Value) -> Option<u64> {
            if let Some(selectors) = value.get("selectors").and_then(serde_json::Value::as_array) {
                return selectors.first()?.get("id")?.as_u64();
            }
            match value {
                serde_json::Value::Object(map) => map.values().find_map(selector_id),
                serde_json::Value::Array(values) => values.iter().find_map(selector_id),
                _ => None,
            }
        }
        let id = selector_id(&json).unwrap();
        let target = fx_schema::PropertyTarget::fx_item(fx_schema::FxItemId::new(id), target_name);
        let entries = converted.document.composition().dynamics().entries();
        let entry = entries
            .iter()
            .find(|entry| entry.target == target)
            .unwrap_or_else(|| {
                panic!(
                    "missing evaluated {native_name}: {:?}",
                    converted.diagnostics
                )
            });
        let track = entry.animator.keyframe_track().unwrap();
        track.validate_for_target(&entry.target).unwrap();
        let keys = track.keyframes();
        assert_eq!(keys[0].layer_time().as_millis(), 0);
        assert_eq!(keys.last().unwrap().layer_time().as_millis(), 3000);
        assert_eq!(keys[0].value(), &fx_schema::PropertyValue::Float(0.0));
        assert_eq!(
            keys.last().unwrap().value(),
            &fx_schema::PropertyValue::Float(60.0 * scale)
        );
        assert!(!json.to_string().to_lowercase().contains("jsscript"));
    }
}

#[test]
fn effect_active_reads_the_native_enable_switch() {
    let code = "thisComp.layer('controller').effect('amount').active ? 7 : 9";
    evaluate(code, 0.5, &[7.0]);
    let mut model = model(code, 1);
    model.comps[0].layers[1].effects[0].enabled = false;
    assert_eq!(
        evaluate_grid(&mut prepare(&model).unwrap(), 0, &[0.5]).unwrap(),
        vec![vec![9.0]]
    );
}

#[test]
fn named_markers_choose_the_first_matching_key_and_keep_numeric_indices() {
    evaluate(
        "thisComp.layer('controller').marker.key('exit').time",
        0.5,
        &[0.7],
    );
    evaluate(
        "thisComp.layer('controller').marker.key(1).time",
        0.5,
        &[0.2],
    );
    evaluate(
        "var t=9; try { t=thisComp.layer('controller').marker.key('missing').time; } catch(e) {} t;",
        0.5,
        &[9.0],
    );
    let code = "thisComp.layer('controller').marker.key('reveal').time";
    let mut model = model(code, 1);
    model.comps[0].layers[1].markers.names[1] = "reveal".into();
    assert_eq!(
        evaluate_grid(&mut prepare(&model).unwrap(), 0, &[0.5]).unwrap(),
        vec![vec![0.2]]
    );
}
