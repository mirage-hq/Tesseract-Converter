use std::{collections::BTreeMap, io::Cursor, path::Path};

use super::{
    metadata::{validate_archive_path, METADATA_SCHEMA_URL, PROJECT_PATH},
    read_document_snapshot, AssetDescriptor, AssetKind, Generator, ProjectDescriptor,
    TesseractFileMetadata,
};

#[test]
fn document_snapshot_preserves_exact_bytes() {
    let bytes = b"{\"name\":\"project\"}";
    assert_eq!(
        read_document_snapshot(
            &mut Cursor::new(bytes),
            bytes.len() as u64,
            Path::new("project.json"),
        )
        .unwrap(),
        bytes,
    );
}

#[test]
fn document_snapshot_rejects_growth_and_truncation() {
    let path = Path::new("project.json");
    let error = read_document_snapshot(&mut Cursor::new(b"abc"), 2, path).unwrap_err();
    assert!(error.to_string().contains("length changed"));
    assert!(read_document_snapshot(&mut Cursor::new(b"a"), 2, path).is_err());
}

#[test]
fn metadata_accepts_inventories_beyond_previous_policy_ceilings() {
    const PREVIOUS_TOTAL_ASSET_LIMIT: u64 = 64 * 1024 * 1024 * 1024;
    let assets = (0..4_097)
        .map(|index| {
            let asset_id = format!("asset-{index}");
            (
                asset_id.clone(),
                AssetDescriptor {
                    path: format!("assets/{asset_id}"),
                    kind: AssetKind::Other,
                    content_type: "application/octet-stream".to_owned(),
                    byte_length: 16 * 1024 * 1024,
                    sha256: "0".repeat(64),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let total_bytes = assets
        .values()
        .map(|descriptor| descriptor.byte_length)
        .sum::<u64>();
    assert!(total_bytes > PREVIOUS_TOTAL_ASSET_LIMIT);

    metadata(assets.clone()).validate().unwrap();

    let mut overflow = assets;
    overflow.values_mut().next().unwrap().byte_length = u64::MAX;
    assert!(metadata(overflow)
        .validate()
        .unwrap_err()
        .to_string()
        .contains("overflow"));
}

#[test]
fn archive_path_uses_zip_name_width() {
    let prefix = "assets/";
    let maximum = format!(
        "{prefix}{}",
        "a".repeat(usize::from(u16::MAX) - prefix.len())
    );
    validate_archive_path(&maximum).unwrap();

    let too_long = format!("{maximum}a");
    assert!(validate_archive_path(&too_long).is_err());
}

#[test]
fn metadata_reads_archives_without_fx_schema_version() {
    let metadata = metadata(BTreeMap::new());
    let mut json = serde_json::to_value(&metadata).unwrap();
    json.as_object_mut().unwrap().remove("fxSchemaVersion");

    let legacy: TesseractFileMetadata = serde_json::from_value(json).unwrap();
    assert_eq!(legacy.fx_schema_version, None);
    legacy.validate().unwrap();
}

#[test]
fn metadata_does_not_gate_on_a_newer_fx_schema_version() {
    let mut metadata = metadata(BTreeMap::new());
    metadata.fx_schema_version = Some(u8::MAX);
    let decoded: TesseractFileMetadata =
        serde_json::from_value(serde_json::to_value(metadata).unwrap()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.fx_schema_version, Some(u8::MAX));
}

#[test]
fn fx_schema_version_notice_is_informational_for_differences_and_missing_versions() {
    let current = fx_schema::FX_SCHEMA_REVISION;
    let mut metadata = metadata(BTreeMap::new());
    assert_eq!(metadata.fx_schema_version_notice("converter"), None);

    metadata.fx_schema_version = Some(current - 1);
    let notice = metadata.fx_schema_version_notice("converter").unwrap();
    assert!(notice.starts_with("info: FX schema versions:"));
    assert!(notice.contains(&format!("Tesseract file fxSchemaVersion={}", current - 1)));
    assert!(notice.contains(&format!("converter fxSchemaVersion={current}")));
    assert!(notice.contains("may be relevant"));

    metadata.fx_schema_version = None;
    let missing = metadata.fx_schema_version_notice("converter").unwrap();
    assert!(missing.contains("Tesseract file fxSchemaVersion=unknown (missing)"));
    assert!(missing.contains(&format!("converter fxSchemaVersion={current}")));
}

fn metadata(assets: BTreeMap<String, AssetDescriptor>) -> TesseractFileMetadata {
    TesseractFileMetadata {
        schema: METADATA_SCHEMA_URL.to_owned(),
        format: "tesseract".to_owned(),
        format_version: 2,
        fx_schema_version: Some(fx_schema::FX_SCHEMA_REVISION),
        document_id: "00000000-0000-0000-0000-000000000001".to_owned(),
        created_at: "2026-01-01T00:00:00Z".to_owned(),
        modified_at: "2026-01-01T00:00:00Z".to_owned(),
        generator: Generator {
            name: "test".to_owned(),
            version: "1".to_owned(),
            engine_version: "1".to_owned(),
            git_revision: None,
        },
        project: ProjectDescriptor {
            path: PROJECT_PATH.to_owned(),
            content_type: "application/vnd.tesseract.fx-composition+json".to_owned(),
            byte_length: 0,
            sha256: "0".repeat(64),
        },
        assets,
        fonts: BTreeMap::new(),
    }
}
