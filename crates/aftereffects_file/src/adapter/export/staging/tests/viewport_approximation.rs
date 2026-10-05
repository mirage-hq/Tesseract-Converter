use super::*;

fn oversized_value() -> Value {
    let mut value: Value = serde_json::from_str(RECT).unwrap();
    let mut child = value["composition"]["layers"][0].clone();
    child["name"] = json!("Editable oversized child");
    child["parent"] = json!(901);
    child["rect"]["size"] = json!([1_000_000.0, 80.0]);
    child["effects"] = json!([{"id":902,"enabled":true,"effect":{
        "type":"directionalBlur","direction":90.0,"blurLength":10.0
    }}]);
    let transform = child["transform"].clone();
    value["composition"]["layers"] = json!([{
        "type":"Group","id":901,"name":"Oversized viewport owner","parent":null,
        "transform":transform,"layers":[child],"isHidden":false,"blendMode":"normal",
        "trackMatte":null,"masks":[],"effects":[{"id":903,"enabled":true,"effect":{
            "type":"radialBlur","centerX":0.5,"centerY":0.5,"amount":10.0
        }}],"motionBlur":true,
        "playback":{"type":"windowed","inputRange":{"start":0,"duration":2000},
            "mapping":{"type":"linear","input":{"start":0,"duration":2000},
                       "output":{"start":0,"duration":2000}},"inputOffsetMs":0},
        "paddingTop":0.0,"paddingRight":0.0,"paddingBottom":0.0,"paddingLeft":0.0
    }]);
    value
}

const APPROXIMATION: &str = "Oversized source viewport approximation";

fn staged_output(value: Value) -> (Vec<u8>, Vec<String>) {
    let parent = tempfile::tempdir().unwrap();
    let archive = TesseractFileBuilder::try_new(document(value))
        .unwrap()
        .write(parent.path().join("viewport.tsrct"))
        .unwrap();
    let staged = AfterEffects
        .stage_document(
            &archive,
            archive.project(),
            parent.path(),
            &AfterEffectsExportOptions { fps: 30.0 },
        )
        .unwrap();
    let bytes = fs::read(staged.directory().join("project.aep")).unwrap();
    let diagnostics = staged
        .report()
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.clone())
        .collect();
    (bytes, diagnostics)
}

fn names(bytes: &[u8]) -> Vec<String> {
    let native = read_project(bytes).unwrap();
    native
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(comp) => {
                Some(comp.layers.iter().map(|layer| layer.name.to_string()))
            }
            _ => None,
        })
        .flatten()
        .collect()
}

#[test]
fn viewport_approximation_picture_only_and_selected_writers_retain_content_by_default() {
    let parent = tempfile::tempdir().unwrap();
    let archive = TesseractFileBuilder::try_new(document(path_value()))
        .unwrap()
        .write(parent.path().join("picture.tsrct"))
        .unwrap();
    for selected in [false, true] {
        let options = AfterEffectsExportOptions { fps: 30.0 };
        let staged = if selected {
            AfterEffects.stage_picture_layers(
                &archive,
                archive.project(),
                0..1,
                parent.path(),
                &options,
            )
        } else {
            AfterEffects.stage_picture_only_document(
                &archive,
                archive.project(),
                parent.path(),
                &options,
            )
        }
        .unwrap();
        let bytes = fs::read(staged.directory().join("project.aep")).unwrap();
        let actual = names(&bytes);
        for expected in ["Oversized viewport owner", "Editable oversized child"] {
            assert!(
                actual.iter().any(|name| name == expected),
                "default picture writer omitted {expected}: {actual:?}"
            );
        }
    }
}

#[test]
fn viewport_approximation_default_retains_oversized_blurred_content() {
    let (bytes, diagnostics) = staged_output(path_value());
    let actual = names(&bytes);
    for expected in ["Oversized viewport owner", "Editable oversized child"] {
        assert!(
            actual.iter().any(|name| name == expected),
            "default export omitted {expected}: {actual:?}"
        );
    }
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains(APPROXIMATION))
    );
}

#[test]
fn viewport_approximation_does_not_relax_native_rect_child_limits() {
    let (bytes, diagnostics) = staged_output(oversized_value());
    assert!(
        names(&bytes)
            .iter()
            .any(|name| name == "Oversized viewport owner")
    );
    assert!(
        !names(&bytes)
            .iter()
            .any(|name| name == "Editable oversized child")
    );
    assert!(diagnostics.iter().any(|message| {
        message.contains("Rectangle geometry/Transform exceeds native static value bounds")
    }));
}

fn path_value() -> Value {
    let mut value = oversized_value();
    let child = &mut value["composition"]["layers"][0]["layers"][0];
    child["type"] = json!("Shape");
    child.as_object_mut().unwrap().remove("rect");
    child["shape"] = json!({"path":{"commands":[
        {"type":"moveTo","x":0.0,"y":0.0},
        {"type":"lineTo","x":1_000_000.0,"y":0.0},
        {"type":"lineTo","x":1_000_000.0,"y":80.0},
        {"type":"lineTo","x":0.0,"y":80.0},{"type":"close"}
    ]},"fills":[{"paint":{"type":"solid","color":[0.2,0.4,0.6,1.0]},
        "fillRule":"nonZeroWinding","blendMode":"normal","opacity":1.0}]});
    value
}

#[test]
fn viewport_approximation_representable_output_is_byte_identical() {
    let mut value = oversized_value();
    value["composition"]["layers"][0]["layers"][0]["rect"]["size"] = json!([120.0, 80.0]);
    let normal = staged_output(value);
    // SHA-256 of the pre-fix standard export: representable output is unchanged.
    assert_eq!(
        format!("{:x}", Sha256::digest(&normal.0)),
        "e56a9d3d05d22fe94ef35e6640fe6ac646caf342859a520f8d8cacdd7389d2e4"
    );
    assert!(
        !normal
            .1
            .iter()
            .any(|message| message.contains(APPROXIMATION))
    );
    assert!(
        names(&normal.0)
            .iter()
            .any(|name| name == "Oversized viewport owner")
    );
}

#[test]
fn viewport_approximation_exact_collapsed_output_is_byte_identical() {
    let mut value = path_value();
    let group = &mut value["composition"]["layers"][0];
    group["effects"] = json!([]);
    group["motionBlur"] = json!(false);
    group["layers"][0]["effects"] = json!([]);
    let collapsed = staged_output(value);
    // SHA-256 of the pre-fix standard export: exact collapse is unchanged.
    assert_eq!(
        format!("{:x}", Sha256::digest(&collapsed.0)),
        "3bea957a309bf75db37ba040f42e774938d92d989e743f6393e4d4651c353122"
    );
    assert!(
        !collapsed
            .1
            .iter()
            .any(|message| message.contains(APPROXIMATION))
    );
    let native = read_project(&collapsed.0).unwrap();
    let owner = native
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some(comp.layers.iter()),
            _ => None,
        })
        .flatten()
        .find(|layer| layer.name.as_ref() == "Oversized viewport owner")
        .unwrap();
    assert!(owner.record.flags().collapse_transformation);
}

#[test]
fn viewport_approximation_mixed_3d_path_preserves_native_children() {
    let mut value = path_value();
    let group = &mut value["composition"]["layers"][0];
    group["layers"][0]["transform"]["rotationX"] = json!(15.0);
    let mut floor: Value = serde_json::from_str(RECT).unwrap();
    floor = floor["composition"]["layers"][0].clone();
    floor["id"] = json!(904);
    floor["parent"] = json!(901);
    floor["name"] = json!("Ordinary editable floor");
    group["layers"].as_array_mut().unwrap().push(floor);
    let (bytes, diagnostics) = staged_output(value);
    let native = read_project(&bytes).unwrap();
    let all_names = names(&bytes);
    for name in [
        "Oversized viewport owner",
        "Editable oversized child",
        "Ordinary editable floor",
    ] {
        assert!(
            all_names.iter().any(|actual| actual == name),
            "{name}: {diagnostics:?}"
        );
    }
    let source = native
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(comp)
                if comp
                    .layers
                    .iter()
                    .any(|layer| layer.name.as_ref() == "Editable oversized child") =>
            {
                Some(comp)
            }
            _ => None,
        })
        .unwrap();
    assert!(source.width > 0 && source.height > 0);
    assert_eq!(source.frame_rate, 30.0);
    assert_eq!(source.duration_secs, 2.0);
    assert!(
        source.layers.len() >= 3,
        "native children plus camera retained"
    );
    let camera = source
        .layers
        .iter()
        .find(|layer| layer.name.as_ref() == "FX root projection")
        .expect("source camera retained");
    let camera_properties = crate::properties::read_transform(&camera.content).unwrap();
    let camera_position = camera_properties
        .iter()
        .find(|property| property.match_name == "ADBE Position")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert_eq!(
        camera_position.values,
        [
            f64::from(source.width) * 0.5,
            f64::from(source.height) * 0.5,
            -crate::writer::NativeCameraSpec::root(1920, 1080).distance,
        ]
    );
    let child = source
        .layers
        .iter()
        .find(|layer| layer.name.as_ref() == "Editable oversized child")
        .unwrap();
    assert_ne!(child.record.id(), 0);
    let owner = native
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some(comp.layers.iter()),
            _ => None,
        })
        .flatten()
        .find(|layer| layer.name.as_ref() == "Oversized viewport owner")
        .unwrap();
    assert!(owner.record.flags().motion_blur);
    let item = native.item(owner.record.source_id()).unwrap();
    assert!(matches!(&item.kind, ItemKind::Composition(comp)
        if comp.layers.iter().any(|layer| layer.record.id() == child.record.id())));
    let (effects, _) = crate::effects::native::read_effects(
        &child.content,
        [f64::from(source.width), f64::from(source.height)],
    );
    assert!(
        effects
            .iter()
            .any(|effect| effect.match_name == "ADBE Motion Blur" && effect.enabled)
    );
    assert!(child.record.flags().three_d_layer);
    assert!(
        crate::properties::read_transform(&child.content)
            .unwrap()
            .iter()
            .any(|property| property.match_name == "ADBE Rotate X")
    );
}

#[test]
fn viewport_approximation_nested_motion_rotation_and_opacity_keys_survive() {
    let mut value = path_value();
    let mut inner = value["composition"]["layers"][0].clone();
    inner["id"] = json!(905);
    inner["parent"] = json!(901);
    inner["name"] = json!("Nested oversized world");
    inner["effects"][0]["id"] = json!(906);
    inner["layers"][0]["parent"] = json!(905);
    value["composition"]["layers"][0]["layers"] = json!([inner]);
    value["composition"]["dynamics"]["entries"] = json!([
        {"target":{"kind":"layer","layerId":901,"propertyType":"rotation"},
         "animator":{"type":"keyframes","enabled":true,"keyframes":[
             {"id":"r0","layerTime":0,"value":{"type":"float","value":0.0},"easing":{"type":"linear"}},
             {"id":"r1","layerTime":1000,"value":{"type":"float","value":20.0},"easing":{"type":"linear"}}]}},
        {"target":{"kind":"layer","layerId":901,"propertyType":"opacity"},
         "animator":{"type":"keyframes","enabled":true,"keyframes":[
             {"id":"o0","layerTime":0,"value":{"type":"float","value":100.0},"easing":{"type":"linear"}},
             {"id":"o1","layerTime":1000,"value":{"type":"float","value":25.0},"easing":{"type":"linear"}}]}}
    ]);
    let (bytes, diagnostics) = staged_output(value);
    let native = read_project(&bytes).unwrap();
    let all_names = names(&bytes);
    for name in [
        "Oversized viewport owner",
        "Nested oversized world",
        "Editable oversized child",
    ] {
        assert!(
            all_names.iter().any(|actual| actual == name),
            "{name}: {diagnostics:?}"
        );
    }
    assert_eq!(
        diagnostics
            .iter()
            .filter(|message| message.contains(APPROXIMATION))
            .count(),
        2
    );
    let owner = native
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some(comp.layers.iter()),
            _ => None,
        })
        .flatten()
        .find(|layer| layer.name.as_ref() == "Oversized viewport owner")
        .unwrap();
    assert!(owner.record.flags().motion_blur);
    let properties = crate::properties::read_transform(&owner.content).unwrap();
    for (match_name, values) in [
        ("ADBE Rotate Z", [0.0, 20.0]),
        ("ADBE Opacity", [1.0, 0.25]),
    ] {
        let numeric = properties
            .iter()
            .find(|property| property.match_name == match_name)
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        assert_eq!(
            numeric
                .keyframes
                .iter()
                .map(|key| key.time_secs)
                .collect::<Vec<_>>(),
            [0.0, 1.0]
        );
        assert_eq!(
            numeric
                .keyframes
                .iter()
                .map(|key| key.values[0])
                .collect::<Vec<_>>(),
            values
        );
    }
}

#[test]
fn viewport_approximation_full_parent_demand_is_not_replaced_by_root_canvas() {
    let mut value = path_value();
    let mut outer = value["composition"]["layers"][0].clone();
    outer["id"] = json!(909);
    outer["name"] = json!("Representable unknown-blur parent");
    outer["effects"][0]["id"] = json!(910);
    let inner = &mut value["composition"]["layers"][0];
    inner["parent"] = json!(909);
    inner["transform"]["scale"] = json!([0.01, 0.01]);
    outer["layers"] = json!([inner.clone()]);
    value["composition"]["layers"] = json!([outer]);
    let output = staged_output(value);
    assert!(
        !output
            .1
            .iter()
            .any(|message| message.contains(APPROXIMATION))
    );
    assert!(
        names(&output.0)
            .iter()
            .any(|name| name == "Representable unknown-blur parent")
    );
    assert!(
        !names(&output.0)
            .iter()
            .any(|name| name == "Oversized viewport owner")
    );
}

#[test]
fn viewport_approximation_known_blur_reach_remains_additive() {
    fn source_size(bytes: &[u8]) -> (u16, u16) {
        read_project(bytes)
            .unwrap()
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(comp)
                    if comp
                        .layers
                        .iter()
                        .any(|layer| layer.name.as_ref() == "Editable oversized child") =>
                {
                    Some((comp.width, comp.height))
                }
                _ => None,
            })
            .unwrap()
    }
    let mut value = path_value();
    let without = staged_output(value.clone());
    value["composition"]["layers"][0]["effects"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":911,"enabled":true,"effect":{"type":"gaussianBlur","blurriness":8.0}}));
    let with = staged_output(value);
    let (width, height) = source_size(&without.0);
    assert_eq!(source_size(&with.0), (width + 16, height + 16));
}

fn native_named_layer(bytes: &[u8], name: &str) -> crate::structure::Layer {
    read_project(bytes)
        .unwrap()
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some(comp.layers.iter()),
            _ => None,
        })
        .flatten()
        .find(|layer| layer.name.as_ref() == name)
        .unwrap()
        .clone()
}

#[test]
fn viewport_approximation_review_checked_offset_clock_retains_late_nested_source() {
    let mut value = path_value();
    value["duration"] = json!(4.0);
    let mut inner = value["composition"]["layers"][0].clone();
    inner["id"] = json!(905);
    inner["parent"] = json!(901);
    inner["name"] = json!("Late nested world");
    inner["effects"][0]["id"] = json!(906);
    let late = json!({"start":2000,"duration":1000});
    inner["playback"]["inputRange"] = late.clone();
    inner["playback"]["mapping"]["input"] = late.clone();
    inner["playback"]["mapping"]["output"] = late.clone();
    inner["layers"][0]["activeRange"] = late;
    inner["layers"][0]["parent"] = json!(905);
    let outer = &mut value["composition"]["layers"][0];
    let window = json!({"start":1000,"duration":2000});
    outer["playback"]["inputRange"] = window.clone();
    outer["playback"]["mapping"]["input"] = window.clone();
    outer["playback"]["mapping"]["output"] = window;
    outer["layers"] = json!([inner]);
    let (bytes, diagnostics) = staged_output(value);
    let all_names = names(&bytes);
    for name in [
        "Oversized viewport owner",
        "Late nested world",
        "Editable oversized child",
    ] {
        assert!(
            all_names.iter().any(|actual| actual == name),
            "visible {name} omitted: {diagnostics:?}"
        );
    }
    for (name, start, first, last, visible_first, visible_last) in [
        ("Oversized viewport owner", 0.0, 1.0, 3.0, 1.0, 3.0),
        ("Late nested world", 0.0, 2.0, 3.0, 2.0, 3.0),
        // with_active_range stores a Shape's start plus local zero/length,
        // unlike the source-zero, identity-mapped Group occurrence records.
        ("Editable oversized child", 2.0, 0.0, 1.0, 2.0, 3.0),
    ] {
        let layer = native_named_layer(&bytes, name);
        assert_eq!(layer.record.start_time(), Some(start), "{name}");
        assert_eq!(layer.record.in_point(), Some(first), "{name}");
        assert_eq!(layer.record.out_point(), Some(last), "{name}");
        assert_eq!(
            start + first,
            visible_first,
            "{name}: source-domain visibility start"
        );
        assert_eq!(
            start + last,
            visible_last,
            "{name}: source-domain visibility end"
        );
        assert_eq!(layer.record.stretch(), Some(1.0), "{name}");
        assert!(
            !crate::properties::root_runs(&layer.content)
                .unwrap()
                .iter()
                .any(|(match_name, _)| *match_name == "ADBE Time Remapping"),
            "unit affine source mapping stays affine"
        );
    }
    let native = read_project(&bytes).unwrap();
    for name in ["Oversized viewport owner", "Late nested world"] {
        let owner = native_named_layer(&bytes, name);
        let ItemKind::Composition(source) = &native.item(owner.record.source_id()).unwrap().kind
        else {
            panic!("editable source composition");
        };
        assert_eq!(
            source.duration_secs, 3.0,
            "{name}: explicit child source domain"
        );
        assert_eq!(source.frame_rate, 30.0);
    }
}

#[test]
fn viewport_approximation_review_radial_center_preserves_logical_points_and_keys() {
    for animated in [false, true] {
        let mut value = path_value();
        let group = &mut value["composition"]["layers"][0];
        group["transform"]["anchorPoint"] = json!([30.0, 70.0]);
        group["transform"]["position"] = json!([777.0, 333.0]);
        group["transform"]["rotation"] = json!(27.0);
        group["effects"][0]["effect"]["centerX"] = json!(0.25);
        group["effects"][0]["effect"]["centerY"] = json!(0.75);
        if animated {
            value["composition"]["dynamics"]["entries"] = json!([
                {"target":{"kind":"effectProperty","effectId":903,"paramName":"centerX"},
                 "animator":{"type":"keyframes","enabled":true,"keyframes":[
                    {"id":"cx0","layerTime":0,"value":{"type":"float","value":0.25},"easing":{"type":"linear"}},
                    {"id":"cx1","layerTime":1500,"value":{"type":"float","value":0.5},"easing":{"type":"linear"}}
                 ]}},
                {"target":{"kind":"effectProperty","effectId":903,"paramName":"centerY"},
                 "animator":{"type":"keyframes","enabled":true,"keyframes":[
                    {"id":"cy0","layerTime":500,"value":{"type":"float","value":0.75},"easing":{"type":"linear"}},
                    {"id":"cy1","layerTime":1000,"value":{"type":"float","value":0.25},"easing":{"type":"linear"}}
                 ]}}
            ]);
        }
        let (bytes, diagnostics) = staged_output(value);
        assert!(
            diagnostics
                .iter()
                .any(|message| message.contains(APPROXIMATION))
        );
        let owner = native_named_layer(&bytes, "Oversized viewport owner");
        let transform = crate::properties::read_transform(&owner.content).unwrap();
        let anchor = transform
            .iter()
            .find(|property| property.match_name == "ADBE Anchor Point")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        let native = read_project(&bytes).unwrap();
        let ItemKind::Composition(source) = &native.item(owner.record.source_id()).unwrap().kind
        else {
            panic!("source composition");
        };
        // Native AV anchors are stored as fractions of their source dimensions.
        let origin = [
            30.0 - anchor.values[0] * f64::from(source.width),
            70.0 - anchor.values[1] * f64::from(source.height),
        ];
        assert!(
            origin.iter().all(|coordinate| *coordinate != 0.0),
            "discriminating nonzero origin"
        );
        let (effects, _) = crate::effects::native::read_effects(
            &owner.content,
            [f64::from(source.width), f64::from(source.height)],
        );
        let radial = effects
            .iter()
            .find(|effect| effect.match_name == "ADBE Radial Blur")
            .unwrap();
        let center = radial
            .parameters
            .iter()
            .find(|parameter| parameter.match_name == "ADBE Radial Blur-0002")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        if !animated {
            for axis in 0..2 {
                assert!(
                    (center.values[axis] + origin[axis] - [480.0, 810.0][axis]).abs() < 1e-6,
                    "static logical point moved: center={:?}, origin={origin:?}",
                    center.values
                );
            }
        }
        if animated {
            assert_eq!(
                center
                    .keyframes
                    .iter()
                    .map(|key| key.time_secs)
                    .collect::<Vec<_>>(),
                [0.0, 0.5, 1.0, 1.5]
            );
            for (key, expected) in center.keyframes.iter().zip([
                [480.0, 810.0],
                [640.0, 810.0],
                [800.0, 270.0],
                [960.0, 270.0],
            ]) {
                for axis in 0..2 {
                    assert!(
                        (key.values[axis] + origin[axis] - expected[axis]).abs() < 1e-6,
                        "logical keyed point moved at {}: {:?} + {origin:?} != {expected:?}",
                        key.time_secs,
                        key.values
                    );
                }
            }
        } else {
            assert!(center.keyframes.is_empty());
        }
    }
}

#[test]
fn viewport_approximation_unsafe_sources_keep_original_rejection() {
    let original = path_value();
    let mut cases = Vec::new();
    let mut clock = original.clone();
    clock["composition"]["layers"][0]["playback"]["mapping"]["output"]["duration"] = json!(4000);
    cases.push(("nonunit clock", clock));
    let mut singular = original.clone();
    singular["composition"]["layers"][0]["transform"]["scale"] = json!([0.0, 100.0]);
    cases.push(("singular owner", singular));
    let mut nonfinite = original.clone();
    nonfinite["composition"]["layers"][0]["layers"][0]["transform"]["scale"] =
        json!([1e308, 100.0]);
    cases.push(("overflowing geometry", nonfinite));
    let mut projective = original.clone();
    projective["composition"]["layers"][0]["transform"]["rotationY"] = json!(30.0);
    cases.push(("projective owner", projective));
    let mut near_plane = original.clone();
    near_plane["composition"]["layers"][0]["layers"][0]["transform"]["rotationY"] = json!(90.0);
    cases.push(("near plane", near_plane));
    let mut temporal = original.clone();
    temporal["composition"]["layers"][0]["effects"][0]["effect"] =
        json!({"type":"pixelMotionBlur"});
    cases.push(("unproved temporal effect", temporal));
    let mut temporal_child = original.clone();
    temporal_child["composition"]["layers"][0]["layers"][0]["effects"][0]["effect"] =
        json!({"type":"pixelMotionBlur"});
    cases.push(("unproved descendant temporal effect", temporal_child));
    let mut foreign = original.clone();
    let mut consumer: Value = serde_json::from_str(RECT).unwrap();
    consumer = consumer["composition"]["layers"][0].clone();
    consumer["id"] = json!(908);
    consumer["trackMatte"] = json!({"mode":"alpha","layer":901});
    foreign["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(consumer);
    cases.push(("foreign matte consumer", foreign));
    let mut masked = original;
    masked["composition"]["layers"][0]["masks"] = json!([{
        "id":907,"mode":"add","inverted":false,"feather":[0.0,0.0],
        "opacity":100.0,"expansion":0.0,"path":{"commands":[
            {"type":"moveTo","x":0.0,"y":0.0},{"type":"lineTo","x":1_000_000.0,"y":0.0},
            {"type":"lineTo","x":1_000_000.0,"y":80.0},{"type":"close"}
        ]}
    }]);
    cases.push(("masked owner", masked));
    for (case, value) in cases {
        // The rescue always warns; no warning means the original rejection path ran.
        let (_, diagnostics) = staged_output(value);
        assert!(
            !diagnostics
                .iter()
                .any(|message| message.contains(APPROXIMATION)),
            "{case}: unsafe source must not be rescued"
        );
    }
}
