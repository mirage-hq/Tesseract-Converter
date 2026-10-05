//! Empty composition support is pinned by native sources, not a placeholder layer.
use super::*;

fn empty_document() -> Value {
    json!({
        "$schema": "https://jerboa.dev/schemas/fx-composition/editable/v1/document.schema.json",
        "formatVersion": 1,
        "dimensions": {"width": 1080, "height": 1920},
        "duration": 3.0,
        "composition": {"id": "empty-root", "name": "Empty editable root", "layers": []}
    })
}

fn empty_archive(value: &Value, path: &Path) {
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(value).unwrap())
        .unwrap()
        .write(path)
        .unwrap();
}

#[test]
fn empty_root_native_compositions_have_real_duration_without_layers() {
    // Independently authored AE26 source, then separately Adobe-resaved writer
    // controls. These establish native empty structure, not new render proof.
    for bytes in [
        include_bytes!("../../../../../crates/aftereffects_file/tests/fixtures/ae26_one_comp.aep")
            .as_slice(),
        include_bytes!(
            "../../../../../crates/aftereffects_file/tests/fixtures/ae26_rust_wide_resaved.aep"
        )
        .as_slice(),
        include_bytes!(
            "../../../../../crates/aftereffects_file/tests/fixtures/ae26_rust_tall_resaved.aep"
        )
        .as_slice(),
    ] {
        let empty = aftereffects_file::reader::read_empty_composition(bytes).unwrap();
        assert!(empty.duration_secs > 0.0);
        assert!(empty.width > 0 && empty.height > 0);
    }
}

#[test]
fn empty_root_exports_source_composition_and_full_length_link_without_authored_layers() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("empty.tsrct");
    empty_archive(&empty_document(), &input);
    let original = fs::read(&input).unwrap();
    for rate in [
        premiere_file::FrameRate::Fps24,
        premiere_file::FrameRate::Fps30,
    ] {
        let output = root.path().join(format!("output-{rate}"));
        let options = PremiereExportOptions {
            frame_rate: Some(rate),
        };
        let checked = export(&request(&input, &output, ConversionMode::Check), &options).unwrap();
        assert!(!output.exists());
        assert_eq!(
            checked.artifacts.len(),
            2,
            "only Premiere and AEP, no fabricated media"
        );
        let written = export(&request(&input, &output, ConversionMode::Write), &options).unwrap();
        assert_eq!(checked.artifacts, written.artifacts);
        let bytes = fs::read(output.join("media/ae-0001/compositions.aep")).unwrap();
        let project = aftereffects_file::structure::read_project(&bytes).unwrap();
        let aftereffects_file::structure::ItemKind::Composition(composition) =
            &project.item(1).unwrap().kind
        else {
            panic!("root composition")
        };
        assert_eq!(project.items.len(), 1);
        assert_eq!(
            project.item(1).unwrap().name.as_str(),
            "Empty editable root"
        );
        assert!(
            composition.layers.is_empty(),
            "no invented layer or dropped-content stand-in"
        );
        assert_eq!(composition.record.dimensions(), (1080, 1920));
        assert_eq!(composition.duration_secs, 3.0);
        assert_eq!(
            composition.record.frame_rate(),
            if rate == premiere_file::FrameRate::Fps24 {
                24.0
            } else {
                30.0
            }
        );
        let native = xml(&output.join("project.prproj"));
        assert_eq!(native.matches("<VideoClipTrackItem ObjectID=").count(), 1);
        assert!(!native.contains("<AudioClipTrackItem ObjectID="));
        assert_native_forward_parts(&native, "compositions.aep", &[(0, 3000, 0, 3000, 1.0)]);
        assert!(native.contains("0,0,1080,1920"));
    }
    assert_eq!(fs::read(input).unwrap(), original);
}

#[test]
fn empty_root_does_not_turn_omitted_nonempty_input_into_success() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("unsupported.tsrct");
    let mut value = empty_document();
    value["composition"]["layers"] = json!([{
        "type":"Pag", "id":1, "name":"Unsupported content",
        "activeRange":{"start":0,"duration":3000},
        "items":[{"assetId":"unsupported-pag"}]
    }]);
    // Neither exporter supports PAG: its payload must never be decoded or
    // substituted by the genuine-empty route.
    let pag = root.path().join("unsupported.pag");
    fs::write(&pag, b"opaque unsupported PAG payload").unwrap();
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .add_asset("unsupported-pag", &pag, AssetKind::Pag)
        .unwrap()
        .write(&input)
        .unwrap();
    let output = root.path().join("output");
    let error = export(
        &request(&input, &output, ConversionMode::Write),
        &Default::default(),
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("no convertible video or audio layers"),
        "{error}"
    );
    assert!(!output.exists());
}

#[test]
fn empty_root_rejects_unknown_dimensions_without_publication() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("unknown-dimensions.tsrct");
    let mut value = empty_document();
    value["dimensions"]["unmappedControl"] = json!(true);
    empty_archive(&value, &input);
    let original = fs::read(&input).unwrap();
    let output = root.path().join("output");
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        let error = export(&request(&input, &output, mode), &Default::default()).unwrap_err();
        let cause = error
            .downcast_ref::<premiere_file::ConversionError>()
            .expect("typed Premiere rejection");
        assert!(cause.is_unsupported(), "{error:#}");
        assert!(!output.exists());
        assert_eq!(
            fs::read_dir(root.path()).unwrap().count(),
            1,
            "private cleanup"
        );
    }
    assert_eq!(fs::read(input).unwrap(), original);
}

#[test]
fn empty_root_rejects_unrepresentable_clock_and_background_without_publication() {
    let root = tempfile::tempdir().unwrap();
    for (index, duration, background, rate) in [
        (0, 3.01, Value::Null, premiere_file::FrameRate::Fps30),
        (
            1,
            3.0,
            Value::Null,
            premiere_file::FrameRate::Fps30000Over1001,
        ),
        (
            2,
            3.0,
            json!([1.0, 0.0, 0.0, 1.0]),
            premiere_file::FrameRate::Fps30,
        ),
    ] {
        let mut value = empty_document();
        value["duration"] = json!(duration);
        if !background.is_null() {
            value["backgroundColor"] = background;
        }
        let input = root.path().join(format!("empty-{index}.tsrct"));
        empty_archive(&value, &input);
        let output = root.path().join(format!("output-{index}"));
        assert!(export(
            &request(&input, &output, ConversionMode::Write),
            &PremiereExportOptions {
                frame_rate: Some(rate)
            }
        )
        .is_err());
        assert!(!output.exists());
    }
}
