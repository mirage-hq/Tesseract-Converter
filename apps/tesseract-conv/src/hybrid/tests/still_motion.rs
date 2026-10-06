//! Public native still source edited through the production linked-picture route.
//! Serialized controls are structural evidence, not Adobe render acceptance.
use super::*;
use sha2::{Digest, Sha256};

#[test]
fn native_still_motion_scripts_keep_source_relative_anchor_in_linked_export() {
    let source = fixture("premiere_isolated_still_image.prproj");
    let source_bytes = fs::read(&source).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&source_bytes)),
        "c86cf610c4ee427ce1bb08c7b87f322a568ef6e65a0d9e015330aef55bbb3f56"
    );
    let parent = tempfile::tempdir().unwrap();
    let imported = parent.path().join("imported");
    let mut import = request(&source, &imported, ConversionMode::Write);
    import.sequence = Some("c8acf9c1-34b2-4086-9f55-d528950a7059");
    import_premiere(&import).unwrap();
    let archive = TesseractFile::open(imported.join("project.tsrct")).unwrap();
    let mut document = archive.project_json().unwrap();
    let mut image = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Image" && layer["activeRange"]["start"] == 5000)
        .unwrap()
        .clone();
    assert_eq!(image["transform"]["anchorPoint"], json!([960.0, 540.0]));
    assert_eq!(image["transform"]["position"], json!([960.0, 540.0]));
    let owner = image["id"].as_u64().unwrap();
    let asset = image["source"]["assetId"].as_str().unwrap().to_owned();
    image["activeRange"] = json!({"start":0,"duration":1350});
    // Motion blur selects the same existing linked-AEP route as the opening.
    image["motionBlur"] = json!(true);
    document["composition"]["layers"] = json!([image]);
    // Different X/Y curves must retain independent native Position followers.
    let entries = [
        ("positionX", "return 960 - 40 * input.time.seconds;"),
        (
            "positionY",
            "return 540 - 10 * input.time.seconds * input.time.seconds;",
        ),
        ("rotation", "return -2 * input.time.seconds;"),
        ("scaleX", "return 100 - 10 * input.time.seconds;"),
        ("scaleY", "return 100 - 10 * input.time.seconds;"),
    ]
    .map(|(property, code)| {
        json!({
            "target":{"kind":"layer","layerId":owner,"propertyType":property},
            "animator":{"type":"jsScript","layerTimeJsCode":code}
        })
    });
    document["composition"]["dynamics"] = json!({"entries": entries});
    let media = fixture("feature_still_transparent.png");
    let input = parent.path().join("still.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(&asset, &media, AssetKind::Image)
        .unwrap()
        .write(&input)
        .unwrap();
    let input_bytes = fs::read(&input).unwrap();
    let output = parent.path().join("exported");
    let mut conversion = request(&input, &output, ConversionMode::Check);
    conversion.fps = Some("30");
    let checked = crate::formats::export_premiere(&conversion).unwrap();
    assert!(!output.exists());
    conversion.mode = ConversionMode::Write;
    let written = crate::formats::export_premiere(&conversion).unwrap();
    assert_eq!(checked, written);
    assert!(xml(&output.join("project.prproj")).contains("./media/ae-0001/compositions.aep"));
    let project = aftereffects_file::structure::read_project(
        &fs::read(output.join("media/ae-0001/compositions.aep")).unwrap(),
    )
    .unwrap();
    let layers = project
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            aftereffects_file::structure::ItemKind::Composition(comp) => Some(&comp.layers),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(
        layers.len(),
        1,
        "still must remain one editable footage layer"
    );
    let layer = layers[0];
    assert!(layer.record.flags().motion_blur);
    assert!(!layer.record.flags().three_d_layer);
    let properties = aftereffects_file::properties::read_transform(&layer.content).unwrap();
    let numeric = |name| {
        properties
            .iter()
            .find(|property| property.match_name == name)
            .unwrap()
            .numeric
            .as_ref()
            .unwrap()
    };
    assert_eq!(numeric("ADBE Anchor Point").values, [0.5, 0.5, 0.0]);
    assert!(numeric("ADBE Position").dimensions_separated);
    for (name, expected) in [
        ("ADBE Position_0", vec![960.0]),
        ("ADBE Position_1", vec![540.0]),
        ("ADBE Rotate Z", vec![0.0]),
        ("ADBE Scale", vec![1.0, 1.0, 1.0]),
    ] {
        let track = numeric(name);
        assert!(track.animated, "{name} must keep editable keys");
        assert_eq!(track.keyframes.first().unwrap().values, expected);
        assert!(track.keyframes.last().unwrap().time_secs > 1.3);
    }
    let source = project.item(layer.record.source_id()).unwrap();
    assert_eq!(source.media.as_ref().unwrap().as_ref().unwrap().width, 1920);
    assert_eq!(
        source.media.as_ref().unwrap().as_ref().unwrap().height,
        1080
    );
    let packaged = written
        .artifacts
        .iter()
        .find(|artifact| artifact.path.extension().is_some_and(|e| e == "png"))
        .unwrap();
    assert_eq!(
        fs::read(output.join(&packaged.path)).unwrap(),
        fs::read(media).unwrap()
    );
    assert_eq!(fs::read(input).unwrap(), input_bytes);
    assert_eq!(
        fs::read(fixture("premiere_isolated_still_image.prproj")).unwrap(),
        source_bytes
    );
}
