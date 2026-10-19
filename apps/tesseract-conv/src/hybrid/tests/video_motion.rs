use super::*;

#[test]
fn scaled_plain_video_retains_bilinear_sampling_in_linked_picture() {
    let parent = tempfile::tempdir().unwrap();
    let mut document: Value = serde_json::from_str(include_str!(
        "../../../../../crates/premiere_file/tests/fixtures/editable-video.json"
    ))
    .unwrap();
    document["composition"]["layers"][0]["transform"]["scale"] = json!([50.0, 50.0]);
    let media = fixture("video-30fps.mp4");
    let input = parent.path().join("scaled.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset("premiere-video-1", &media, AssetKind::Video)
        .unwrap()
        .write(&input)
        .unwrap();
    let output = parent.path().join("exported");
    let written =
        crate::formats::export_premiere(&request(&input, &output, ConversionMode::Write)).unwrap();
    assert!(
        xml(&output.join("project.prproj")).contains("./media/ae-0001/compositions.aep"),
        "native Motion must not replace bilinear video sampling with bicubic"
    );
    let project = aftereffects_file::structure::read_project(
        &fs::read(output.join("media/ae-0001/compositions.aep")).unwrap(),
    )
    .unwrap();
    let layer = project
        .items
        .iter()
        .find_map(|item| match &item.kind {
            aftereffects_file::structure::ItemKind::Composition(comp) => comp.layers.first(),
            _ => None,
        })
        .unwrap();
    assert_eq!(layer.record.quality(), 2);
    assert!(!layer.record.flags().sampling_quality);
    let properties = aftereffects_file::properties::read_transform(&layer.content).unwrap();
    let scale = properties
        .iter()
        .find(|p| p.match_name == "ADBE Scale")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert_eq!(scale.values, [0.5, 0.5, 1.0]);
    let source = project.item(layer.record.source_id()).unwrap();
    let facts = source.media.as_ref().unwrap().as_ref().unwrap();
    assert_eq!([facts.width, facts.height], [1920, 1080]);
    let native = aftereffects_file::structure::read_project(
        &fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../crates/aftereffects_file/tests/fixtures/layers/avlayer_flags.aep"),
        )
        .unwrap(),
    )
    .unwrap();
    let bicubic = native
        .items
        .iter()
        .find_map(|item| match &item.kind {
            aftereffects_file::structure::ItemKind::Composition(comp)
                if item.name == "samplingQuality_BICUBIC" =>
            {
                Some(&comp.layers[0])
            }
            _ => None,
        })
        .unwrap();
    assert!(bicubic.record.flags().sampling_quality);
    let packaged = written
        .artifacts
        .iter()
        .find(|artifact| {
            artifact
                .path
                .extension()
                .is_some_and(|extension| extension == "mp4")
        })
        .unwrap();
    assert_eq!(
        fs::read(output.join(&packaged.path)).unwrap(),
        fs::read(media).unwrap()
    );
}
