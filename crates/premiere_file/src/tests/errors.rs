use crate::{error::BuildError, ConversionError};

fn contextual(source: BuildError) -> ConversionError {
    BuildError::Context {
        context: "selected sequence".into(),
        source: Box::new(BuildError::Context {
            context: "source clip".into(),
            source: Box::new(source),
        }),
    }
    .into()
}

#[test]
fn error_classification_preserves_context_and_distinguishes_missing_media() {
    let unsupported = contextual(BuildError::Unsupported("unsupported stream".into()));
    assert!(unsupported.is_unsupported());
    assert!(!unsupported.is_missing_media());
    assert!(!unsupported.is_io());
    assert!(unsupported
        .to_string()
        .contains("selected sequence: source clip"));

    let missing = contextual(BuildError::MissingMedia("source.mov".into()));
    assert!(missing.is_missing_media());
    assert!(!missing.is_unsupported());
    assert!(!missing.is_io());
}

#[test]
fn direct_video_import_omits_missing_media_but_retains_read_failure_causes() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.prproj");
    let output = root.path().join("output");
    crate::test_support::write_prproj(&source, include_str!("../../tests/fixtures/one-clip.xml"));
    let missing = crate::premiere_to_tesseract(&source, &output, None, true).unwrap_err();
    // The unavailable occurrence is diagnosed; without any healthy sibling,
    // publication still rejects an empty converted timeline.
    assert!(missing.is_unsupported(), "{missing}");
    assert!(missing.to_string().contains("no output published"));
    assert!(missing.to_string().contains("missing media"));
    assert!(!missing.is_io());
    assert!(!output.exists());

    // A directory at the selected file path forces a real read error without
    // permissions that an elevated test runner could bypass.
    std::fs::create_dir_all(root.path().join("media/source.mp4")).unwrap();
    let unreadable = crate::premiere_to_tesseract(&source, &output, None, true).unwrap_err();
    assert!(unreadable.is_io(), "{unreadable}");
    assert!(!unreadable.is_missing_media());
    assert!(!unreadable.is_unsupported());
    let mut cause: &dyn std::error::Error = &unreadable;
    while let Some(source) = cause.source() {
        cause = source;
    }
    assert!(cause.downcast_ref::<std::io::Error>().is_some(), "{cause}");
    assert!(!output.exists());
}

#[test]
fn error_classification_finds_io_through_format_and_context_sources() {
    let error = contextual(BuildError::Premiere(crate::format::FormatError::Io(
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "read denied"),
    )));
    assert!(error.is_io());
    assert!(!error.is_unsupported());
    assert!(!error.is_missing_media());
    assert!(std::error::Error::source(&error).is_some());

    let removed = contextual(BuildError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "source removed after resolution",
    )));
    assert!(removed.is_io());
    assert!(!removed.is_missing_media());
}
