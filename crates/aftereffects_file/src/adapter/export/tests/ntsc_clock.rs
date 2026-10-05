use super::*;
use tesseract_file::{AssetKind, TesseractFileBuilder};

fn named_layer<'a>(
    project: &'a crate::structure::StructuralProject,
    name: &str,
) -> &'a crate::structure::Layer {
    project
        .items
        .iter()
        .find_map(|item| {
            let ItemKind::Composition(comp) = &item.kind else {
                return None;
            };
            if item.name == name {
                comp.layers.first()
            } else {
                native_layer_named(project, &comp.layers, name)
            }
        })
        .unwrap()
}

#[test]
fn native_ntsc_source_descriptor_and_editable_rate_survive_fresh_export() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ntsc_media_clock");
    let movie = fs::read(fixtures.join("movie.mov")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&movie)),
        "264464691358ee2d43c7960d4630e9149e222d92398798de60ad6486a370c3f3"
    );
    let oracle_bytes = fs::read(fixtures.join("native-control.aep")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&oracle_bytes)),
        "2a75cc845dea790f714d9e8098dfb9b4c541fc360e20ab5a9949ecf0e748058a"
    );
    let oracle = read_project(&oracle_bytes).unwrap();
    let oracle_source = oracle
        .items
        .iter()
        .find_map(|item| item.media.as_ref())
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(
        (
            oracle_source.native_frame_rate.integer,
            oracle_source.native_frame_rate.fractional
        ),
        (29, 63570)
    );
    assert_eq!(
        (
            oracle_source.duration.numerator,
            oracle_source.duration.denominator
        ),
        (3000, 2997)
    );
    let root = tempfile::tempdir().unwrap();
    let seed = TesseractFile::open(import_solid(root.path())).unwrap();
    for (name, output_duration, stretch) in
        [("NTSC original", 1000, 1.0), ("NTSC rate edit", 500, 0.5)]
    {
        let oracle_layer = named_layer(&oracle, name);
        assert_eq!(oracle_layer.record.stretch(), Some(stretch));
        let mut value = seed.project_json().unwrap();
        value["dimensions"] = json!({"width":160,"height":90});
        value["composition"]["name"] = json!(name);
        value["duration"] = json!(f64::from(output_duration) / 1000.0);
        value["composition"]["layers"] = json!([{
            "type":"Video", "id":700, "name":name, "parent":null,
            "transform":{"anchorPoint":[0,0],"position":[0,0],"scale":[100,100],"rotation":0,"opacity":100},
            "playback":{"type":"windowed","inputRange":{"start":0,"duration":output_duration},"mapping":{"type":"linear","input":{"start":0,"duration":output_duration},"output":{"start":0,"duration":1000}},"inputOffsetMs":0},
            "sourceRange":{"start":0,"duration":1000}, "sourceIntrinsicDuration":1001,
            "source":{"assetId":"ntsc-movie"}, "volume":0
        }]);
        value["composition"]["dynamics"] = json!({"entries":[]});
        let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
        let archive = root.path().join(format!("{output_duration}.tsrct"));
        TesseractFileBuilder::try_new(document)
            .unwrap()
            .add_asset("ntsc-movie", fixtures.join("movie.mov"), AssetKind::Video)
            .unwrap()
            .write(&archive)
            .unwrap();
        let output = root.path().join(format!("export-{output_duration}"));
        let options = AfterEffectsExportOptions { fps: 30.0 };
        let checked = AfterEffects
            .export_from_tesseract(&archive, &output, &options, ConversionMode::Check)
            .unwrap();
        assert!(!output.exists());
        let written = AfterEffects
            .export_from_tesseract(&archive, &output, &options, ConversionMode::Write)
            .unwrap();
        assert_eq!(checked, written);
        assert_eq!(
            written.artifacts.len(),
            2,
            "NTSC video must not be omitted: {written:?}"
        );
        let generated = read_project(&fs::read(output.join("project.aep")).unwrap()).unwrap();
        let source = generated
            .items
            .iter()
            .find_map(|item| item.media.as_ref())
            .unwrap()
            .as_ref()
            .unwrap();
        assert_eq!(source.native_frame_rate, oracle_source.native_frame_rate);
        assert_eq!(source.display_frame_rate, oracle_source.display_frame_rate);
        assert_eq!(source.conform_frame_rate, oracle_source.conform_frame_rate);
        assert_eq!(source.duration, oracle_source.duration);
        assert_eq!((source.width, source.height), (160, 90));
        assert_eq!(fs::read(&source.authored_path).unwrap(), movie);
        let layer = named_layer(&generated, name);
        assert_eq!(layer.record.stretch(), oracle_layer.record.stretch());
        assert_eq!(layer.record.start_time(), oracle_layer.record.start_time());
        assert_eq!(layer.record.in_point(), oracle_layer.record.in_point());
        assert_eq!(layer.record.out_point(), Some(1.0));
        assert_ne!(layer.record.source_id(), 0);
        let ItemKind::Composition(comp) = &generated.item(1).unwrap().kind else {
            panic!("root composition");
        };
        assert_eq!((comp.width, comp.height), (160, 90));
        assert_eq!(comp.duration_secs, f64::from(output_duration) / 1000.0);
    }
}
