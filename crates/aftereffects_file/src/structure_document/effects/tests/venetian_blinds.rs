//! Synthetic controls on the public catalog owner; not a new Adobe oracle.
use super::*;

fn leaf_mut<'a>(chunks: &'a mut [Chunk], name: &str) -> Option<&'a mut Chunk> {
    let index = chunks.iter().enumerate().find_map(|(index, chunk)| {
        if chunk.id() != *b"tdmn" || chunk_match_name(chunk) != Some(name) {
            return None;
        }
        let end = chunks[index + 1..]
            .iter()
            .position(|chunk| chunk.id() == *b"tdmn")
            .map_or(chunks.len(), |offset| index + 1 + offset);
        (index + 1..end).find(|index| chunks[*index].list_kind() == Some(*b"tdbs"))
    });
    if let Some(index) = index {
        return Some(&mut chunks[index]);
    }
    for chunk in chunks {
        if let Some(children) = chunk.children_mut()
            && let Some(leaf) = leaf_mut(children, name)
        {
            return Some(leaf);
        }
    }
    None
}

fn soft_keyed_owner() -> Layer {
    let mut width = native_effect("ADBE Gaussian Blur 2", &[("-0001", 100.)]);
    width
        .properties
        .iter_mut()
        .find(|p| p.match_name.ends_with("-0001"))
        .unwrap()
        .animation = Some(NumericTrack {
        keys: [(0, 100.), (2000, 261.)]
            .into_iter()
            .map(|(time_millis, value)| NumericKeyframe {
                time_millis,
                values: vec![value],
                easing: vec![KeyframeEasing::Linear],
                spatial_in: Vec::new(),
                spatial_out: Vec::new(),
            })
            .collect(),
    });
    let mut template = catalog_owner(&[width]);
    let width = leaf_mut(&mut template.content, "ADBE Gaussian Blur 2-0001")
        .unwrap()
        .clone();
    let mut owner = venetian_owner();
    *leaf_mut(&mut owner.content, "ADBE Venetian Blinds-0003").unwrap() = width;
    let feather = leaf_mut(&mut owner.content, "ADBE Venetian Blinds-0004").unwrap();
    let value = feather
        .children_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c.id() == *b"cdat")
        .unwrap();
    *value = Chunk::data(*b"cdat", 1_f64.to_be_bytes()).unwrap();
    owner
}

#[test]
fn soft_venetian_blinds_rejects_unsafe_numeric_keys() {
    let owner = soft_keyed_owner();
    let (sources, _) = native::read_effects(&owner.content, PLANE.map(f64::from));
    for invalid in 0..6 {
        let mut source = sources[0].clone();
        let width = source
            .parameters
            .iter_mut()
            .find(|p| p.match_name == "ADBE Venetian Blinds-0003")
            .unwrap()
            .numeric
            .as_mut()
            .unwrap();
        match invalid {
            0 => width.expression_enabled = true,
            1 => width.dimensions_separated = true,
            2 => width.keyframes[1].values[0] = f64::NAN,
            3 => width.keyframes[1].values[0] = 0.,
            4 => width.keyframes[1].values.push(1.),
            _ => width.keyframes.clear(),
        }
        assert!(
            crate::effects::venetian_blinds::lower(&source, PLANE).is_err(),
            "invalid profile {invalid}"
        );
    }
}

#[test]
fn soft_venetian_blinds_keeps_editable_width_keys() {
    let owner = soft_keyed_owner();
    let imported = import(
        &ExpressionSamples::default(),
        RIPPLE_COMPOSITION,
        &owner,
        PLANE,
        PLANE,
        &mut 100,
        &mut AnimationBudget::default(),
    );
    assert_eq!(imported.effects.len(), 1, "{:?}", imported.warnings);
    let value = serde_json::to_value(&imported.effects[0]).unwrap();
    assert_eq!(value["effect"]["type"], "customShader");
    let params = value["effect"]["params"].as_array().unwrap();
    assert_eq!(params[5]["name"], "pixelCenterOffset");
    assert_eq!(params[5]["default"], -0.5);
    assert_eq!(params[6]["name"], "feather");
    for (name, expected) in [("width", 100.), ("feather", 1.)] {
        assert_eq!(
            params.iter().find(|p| p["name"] == name).unwrap()["default"],
            expected
        );
    }
    let [entry] = imported.animations.as_slice() else {
        panic!("expected one editable width track: {:?}", imported.warnings)
    };
    assert_eq!(
        entry.target,
        PropertyTarget::effect_param(EffectId::new(100), "width")
    );
    let values: Vec<_> = keys(entry)
        .into_iter()
        .map(|(time, value, _)| (time, value))
        .collect();
    assert_eq!(values, [(0, 100.), (2000, 261.)]);
    assert!(
        imported
            .warnings
            .iter()
            .any(|w| w.contains("approximation"))
    );
}
