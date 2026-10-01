//! Shared adapter contract, not Adobe acceptance or render-fidelity evidence.

use aftereffects_file::{AfterEffects, AfterEffectsExportOptions, AfterEffectsImportOptions};
use fx_conv::{
    Artifact, ArtifactKind, ConversionDiagnostic, ConversionMode, ConversionReport,
    ExportFromTesseract, ImportToTesseract,
};
use premiere_file::{Premiere, PremiereExportOptions, PremiereImportOptions};
use std::{
    collections::BTreeMap,
    fmt::Debug,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    sync::Mutex,
};

/// Exercise the same guarantees for every direction, without downcasting a format.
fn contract<D: ConversionDiagnostic, E: Debug>(
    input: &Path,
    output: &Path,
    convert: impl Fn(&Path, &Path, ConversionMode) -> Result<ConversionReport<D>, E>,
) -> ConversionReport {
    let parent = output.parent().unwrap();
    let before = files(parent);
    let checked = convert(input, output, ConversionMode::Check)
        .unwrap()
        .into_common();
    assert!(!output.exists());
    assert_eq!(files(parent), before, "Check must not leave files behind");
    let written = convert(input, output, ConversionMode::Write)
        .unwrap()
        .into_common();
    assert_eq!(checked, written);
    assert!(!written.artifacts.is_empty());
    let actual = files(output);
    let mut declared = std::collections::BTreeSet::new();
    for artifact in &written.artifacts {
        assert!(!artifact.path.as_os_str().is_empty());
        assert!(artifact
            .path
            .components()
            .all(|c| matches!(c, Component::Normal(_))));
        assert!(declared.insert(artifact.path.clone()), "duplicate artifact");
    }
    assert_eq!(declared, actual.keys().cloned().collect());
    assert_eq!(
        fs::read(input).unwrap(),
        before[input.strip_prefix(parent).unwrap()]
    );

    // A report is never returned for an existing destination or malformed input.
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        assert!(convert(input, output, mode).is_err());
        assert_eq!(files(output), actual, "existing output was modified");
        let invalid = parent.join("invalid-input");
        fs::write(&invalid, b"not a project").unwrap();
        let destination = parent.join("failed-output");
        assert!(convert(&invalid, &destination, mode).is_err());
        assert!(!destination.exists());
        fs::remove_file(invalid).unwrap();
    }

    #[cfg(unix)]
    for target in [input, &parent.join("nonexistent-target")] {
        let link = parent.join("output-symlink");
        std::os::unix::fs::symlink(target, &link).unwrap();
        for mode in [ConversionMode::Check, ConversionMode::Write] {
            assert!(convert(input, &link, mode).is_err());
            assert_eq!(fs::read_link(&link).unwrap(), target);
        }
        fs::remove_file(link).unwrap();
    }
    written
}

fn import<C: ImportToTesseract>(
    converter: &C,
    input: &Path,
    output: &Path,
    options: &C::Options,
) -> ConversionReport {
    contract(input, output, |input, output, mode| {
        measured_conversion(|progress| {
            converter.import_to_tesseract_with_progress(input, output, options, mode, progress)
        })
    })
}

fn export<C: ExportFromTesseract>(
    converter: &C,
    input: &Path,
    output: &Path,
    options: &C::Options,
) -> ConversionReport {
    contract(input, output, |input, output, mode| {
        measured_conversion(|progress| {
            converter.export_from_tesseract_with_progress(input, output, options, mode, progress)
        })
    })
}

// The same contract exercises four directions in both Check and Write modes.
// It fails if any adapter silently falls back to the trait's unobserved default.
fn measured_conversion<T, E>(
    convert: impl FnOnce(fx_conv::Progress<'_>) -> Result<T, E>,
) -> Result<T, E> {
    let events = Mutex::new(Vec::<fx_conv::ConversionProgress>::new());
    let observe = |event| events.lock().unwrap().push(event);
    let result = convert(fx_conv::Progress::new(&observe));
    if result.is_ok() {
        let events = events.into_inner().unwrap();
        assert!(
            events
                .iter()
                .any(|event| event.completed.is_some_and(|n| n > 0)),
            "conversion emitted no measured work: {events:?}"
        );
        assert!(
            events.iter().any(|event| event
                .completed
                .zip(event.total)
                .is_some_and(|(done, total)| total > 0 && done == total)),
            "no measured phase reached completion: {events:?}"
        );
        let mut previous: Option<fx_conv::ConversionProgress> = None;
        for event in events {
            if let Some((completed, total)) = event.completed.zip(event.total) {
                assert!(completed <= total, "invalid measurement: {event:?}");
            }
            if !event.started {
                let old = previous.expect("measurement without a phase start");
                assert_eq!(event.phase, old.phase, "phase changed without a reset");
                assert_eq!(event.total, old.total);
                assert!(event.completed >= old.completed);
            }
            previous = Some(event);
        }
    }
    result
}

#[test]
fn after_effects_obeys_both_direction_contracts() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.aep");
    fs::write(
        &input,
        include_bytes!(
            "../../../crates/aftereffects_file/tests/fixtures/properties/transform_unseparated.aep"
        ),
    )
    .unwrap();
    let imported = dir.path().join("imported");
    let report = import(
        &AfterEffects,
        &input,
        &imported,
        &AfterEffectsImportOptions::default(),
    );
    assert_eq!(report.artifacts, [Artifact::project("project.tsrct")]);
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.code == "AE-COMPOSITION-SETTINGS"));

    let exported = dir.path().join("exported");
    let report = export(
        &AfterEffects,
        &imported.join("project.tsrct"),
        &exported,
        &AfterEffectsExportOptions { fps: 60.0 },
    );
    assert_eq!(report.artifacts, [Artifact::project("project.aep")]);
    let native = aftereffects_file::structure::read_project(
        &fs::read(exported.join("project.aep")).unwrap(),
    )
    .unwrap();
    let aftereffects_file::structure::ItemKind::Composition(comp) = &native.item(1).unwrap().kind
    else {
        panic!("missing exported composition")
    };
    assert_eq!(comp.layers.len(), 1);
    assert_eq!(comp.frame_rate, 60.0);
}

#[test]
fn premiere_selects_one_project_and_reports_external_export_media() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("media")).unwrap();
    fs::write(
        root.join("media/source.mp4"),
        include_bytes!("../../../crates/premiere_file/tests/fixtures/video-30fps.mp4"),
    )
    .unwrap();
    // Duplicate names must remain selectable by ID, not be imported as a batch.
    // This is a structural contract, not an Adobe-native feature/fidelity claim.
    let first = include_str!("../../../crates/premiere_file/tests/fixtures/one-clip.xml")
        .replace("1270080000000", "254016000000")
        .replace("2540160000000", "254016000000");
    let second = first
        .replace("ObjectID=\"", "ObjectID=\"2")
        .replace("ObjectRef=\"", "ObjectRef=\"2")
        .replace("ObjectUID=\"", "ObjectUID=\"z-second-")
        .replace("ObjectURef=\"", "ObjectURef=\"z-second-");
    let xml = first.replace(
        "</PremiereData>",
        &second.replace("<PremiereData Version=\"3\">", ""),
    );
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(xml.as_bytes()).unwrap();
    let input = root.join("source.prproj");
    fs::write(&input, encoder.finish().unwrap()).unwrap();
    let imported = root.join("imported");
    let targets = Premiere.list_import_targets(&input).unwrap();
    assert_eq!(targets.len(), 2);
    assert!(targets.iter().all(|target| target.name == "Main"));
    assert!(targets
        .iter()
        .any(|target| target.id == "z-second-sequence-1"));
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        let error = Premiere
            .import_to_tesseract(&input, &imported, &PremiereImportOptions::default(), mode)
            .unwrap_err();
        assert!(error.to_string().contains("--sequence"));
        assert!(!imported.exists());
    }
    let report = import(
        &Premiere,
        &input,
        &imported,
        &PremiereImportOptions {
            sequence: Some("z-second-sequence-1".into()),
        },
    );
    assert_eq!(report.artifacts, [Artifact::project("project.tsrct")]);
    let report = export(
        &Premiere,
        &imported.join("project.tsrct"),
        &root.join("exported"),
        &PremiereExportOptions::default(),
    );
    assert_eq!(
        report
            .artifacts
            .iter()
            .filter(|a| a.kind == ArtifactKind::Project)
            .count(),
        1
    );
    assert_eq!(
        report
            .artifacts
            .iter()
            .filter(|a| a.kind == ArtifactKind::Media)
            .count(),
        1
    );
}

#[test]
fn portable_diagnostics_keep_premiere_scope_and_original_text() {
    use fx_conv::DiagnosticKind;
    use premiere_file::{Omission, OmissionKind, OmissionScope};

    for (scope, code) in [
        (OmissionScope::Feature, "PREMIERE-FEATURE"),
        (OmissionScope::Occurrence, "PREMIERE-OCCURRENCE"),
        (OmissionScope::Track, "PREMIERE-TRACK"),
        (OmissionScope::Sequence, "PREMIERE-SEQUENCE"),
    ] {
        let native = Omission {
            scope,
            kind: OmissionKind::Omitted,
            record: "native-record-id".into(),
            reason: "unsupported content".into(),
        };
        let report = ConversionReport {
            diagnostics: vec![native.clone()],
            artifacts: vec![Artifact::project("one.tsrct")],
        }
        .into_common();
        let diagnostic = &report.diagnostics[0];
        assert_eq!(diagnostic.code, code);
        assert_eq!(diagnostic.kind, DiagnosticKind::Warning);
        assert_eq!(
            diagnostic.context.as_deref(),
            Some(format!("{scope} native-record-id").as_str())
        );
        assert_eq!(diagnostic.to_string(), native.to_string());
        assert_eq!(report.artifacts, [Artifact::project("one.tsrct")]);
    }
}

#[test]
fn premiere_diagnostic_kind_is_the_omission_kind_not_inferred_from_text() {
    use fx_conv::DiagnosticKind;
    use premiere_file::{Omission, OmissionKind, OmissionScope};

    for (kind, expected) in [
        (OmissionKind::Omitted, DiagnosticKind::Warning),
        (OmissionKind::Approximated, DiagnosticKind::Approximation),
    ] {
        let native = Omission {
            scope: OmissionScope::Feature,
            kind,
            record: "Crop Edge Feather".into(),
            reason: "Crop Edge Feather is approximated by an FX mask; Premiere feather visuals are not preserved exactly".into(),
        };
        let diagnostic = native.diagnostic();
        assert_eq!(diagnostic.kind, expected, "{kind:?}");
        assert_eq!(diagnostic.code, "PREMIERE-FEATURE");
        assert_eq!(diagnostic.to_string(), native.to_string());
    }
}

fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &path, result);
            } else {
                result.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}
