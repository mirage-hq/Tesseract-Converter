use fx_schema::EditableFxCompositionDocument;
use serde_json::json;
use std::io::Read;
use std::path::{Path, PathBuf};
use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};

const ASSET_BYTES: &[u8] = b"synthetic archive compatibility asset, not a playable video";

struct EntryLayout {
    name: String,
    size: u64,
    compressed_size: u64,
    header_start: u64,
    data_start: u64,
}

struct Fixture {
    directory: tempfile::TempDir,
    path: PathBuf,
    original: Vec<u8>,
    legacy: Vec<u8>,
    project: Vec<u8>,
    asset_data_start: usize,
}

fn u16_at(bytes: &[u8], offset: usize) -> usize {
    usize::from(u16::from_le_bytes(
        bytes[offset..offset + 2].try_into().unwrap(),
    ))
}

fn fixture() -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let asset_path = directory.path().join("synthetic.mp4");
    std::fs::write(&asset_path, ASSET_BYTES).unwrap();
    let path = directory.path().join("legacy.tsrct");
    let document = EditableFxCompositionDocument::from_json_value(json!({
        "$schema": "https://jerboa.dev/schemas/fx-composition/editable/v1/document.schema.json",
        "formatVersion": 1,
        "dimensions": { "width": 32, "height": 32 },
        "duration": 1,
        "composition": { "id": "zip64-compat", "name": "ZIP archive I/O", "layers": [] }
    }))
    .unwrap();
    let file = TesseractFileBuilder::new(document)
        .add_asset("synthetic", &asset_path, AssetKind::Video)
        .unwrap()
        .write(&path)
        .unwrap();
    let project = file.project_json_bytes().to_vec();
    let asset_name = file.metadata().assets["synthetic"].path.clone();
    drop(file);

    // Only inspect the pristine archive with zip. Once patched, all layout
    // information must come from these independently captured original entries.
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let central_start = usize::try_from(archive.central_directory_start()).unwrap();
    let entries: Vec<_> = (0..archive.len())
        .map(|index| {
            let entry = archive.by_index(index).unwrap();
            EntryLayout {
                name: entry.name().to_owned(),
                size: entry.size(),
                compressed_size: entry.compressed_size(),
                header_start: entry.header_start(),
                data_start: entry.data_start(),
            }
        })
        .collect();
    drop(archive);
    let asset_data_start = usize::try_from(
        entries
            .iter()
            .find(|entry| entry.name == asset_name)
            .unwrap()
            .data_start,
    )
    .unwrap();
    let source = std::fs::read(&path).unwrap();
    let mut original = source[..central_start].to_vec();
    let mut legacy = original.clone();
    let count = entries.len();
    let mut central = central_start;
    for entry in entries {
        assert_eq!(&source[central..central + 4], b"PK\x01\x02");
        let name_len = u16_at(&source, central + 28);
        let extra_len = u16_at(&source, central + 30);
        let comment_len = u16_at(&source, central + 32);
        let extra_start = 46 + name_len;
        assert_eq!(
            &source[central + 46..central + extra_start],
            entry.name.as_bytes()
        );
        let mut record = source[central..central + extra_start].to_vec();
        record[6..8].copy_from_slice(&45_u16.to_le_bytes());
        record[30..32].copy_from_slice(&u16::try_from(extra_len + 28).unwrap().to_le_bytes());
        record.extend_from_slice(&1_u16.to_le_bytes());
        record.extend_from_slice(&24_u16.to_le_bytes());
        for value in [entry.size, entry.compressed_size, entry.header_start] {
            record.extend_from_slice(&value.to_le_bytes());
        }
        record.extend_from_slice(
            &source[central + extra_start..central + extra_start + extra_len + comment_len],
        );
        for offset in [20, 24, 42] {
            record[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        }
        original.extend_from_slice(&record);
        for (offset, value) in [
            (20, entry.compressed_size),
            (24, entry.size),
            (42, entry.header_start),
        ] {
            record[offset..offset + 4]
                .copy_from_slice(&u32::try_from(value).unwrap().to_le_bytes());
        }
        let stale = u64::try_from(source.len()).unwrap() + 10_000;
        for index in 0..3 {
            let start = extra_start + 4 + index * 8;
            record[start..start + 8].copy_from_slice(&stale.to_le_bytes());
        }
        legacy.extend_from_slice(&record);
        central += 46 + name_len + extra_len + comment_len;
    }
    for bytes in [&mut original, &mut legacy] {
        let central_size = u32::try_from(bytes.len() - central_start).unwrap();
        bytes.extend_from_slice(b"PK\x05\x06");
        bytes.extend_from_slice(&[0; 4]);
        for _ in 0..2 {
            bytes.extend_from_slice(&u16::try_from(count).unwrap().to_le_bytes());
        }
        bytes.extend_from_slice(&central_size.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(central_start).unwrap().to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
    }
    // Local headers, payloads, CRCs, and metadata SHA values are untouched.
    assert_eq!(&legacy[..central_start], &original[..central_start]);
    std::fs::write(&path, &legacy).unwrap();
    Fixture {
        directory,
        path,
        original,
        legacy,
        project,
        asset_data_start,
    }
}

fn assert_contents(file: &TesseractFile, project: &[u8]) {
    assert_eq!(file.project_json_bytes(), project);
    assert_eq!(
        file.asset("synthetic")
            .unwrap()
            .read_verified_bytes(1024)
            .unwrap(),
        ASSET_BYTES
    );
}

fn assert_raw_zip_payloads(path: &Path) {
    // Deliberately bypass TesseractFile's compatibility reader. Reading every
    // entry to EOF also checks the ZIP CRC, rather than just directory parsing.
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    assert_eq!(archive.len(), 3);
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        assert_eq!(u64::try_from(bytes.len()).unwrap(), entry.size());
    }
}

#[test]
fn redundant_zip64_values_use_complete_32_bit_fields_without_changing_source() {
    let fixture = fixture();
    let file = TesseractFile::open(&fixture.path).unwrap();
    assert_contents(&file, &fixture.project);
    assert_eq!(std::fs::read(&fixture.path).unwrap(), fixture.legacy);
}

#[test]
fn saving_legacy_redundant_zip64_writes_raw_zip_readable_entries() {
    let fixture = fixture();
    let mut file = TesseractFile::open(&fixture.path).unwrap();
    file.save().unwrap();
    assert_raw_zip_payloads(&fixture.path);
    assert_contents(
        &TesseractFile::open(&fixture.path).unwrap(),
        &fixture.project,
    );
}

#[test]
fn optimizing_legacy_redundant_zip64_writes_raw_zip_readable_entries() {
    let fixture = fixture();
    let mut file = TesseractFile::open(&fixture.path).unwrap();
    file.optimize().unwrap();
    assert_raw_zip_payloads(&fixture.path);
    assert_contents(
        &TesseractFile::open(&fixture.path).unwrap(),
        &fixture.project,
    );
}

#[test]
fn saving_legacy_redundant_zip64_as_preserves_source_and_repairs_destination() {
    let fixture = fixture();
    let destination = fixture.directory.path().join("saved-as.tsrct");
    let mut file = TesseractFile::open(&fixture.path).unwrap();
    file.save_as(&destination).unwrap();
    assert_eq!(std::fs::read(&fixture.path).unwrap(), fixture.legacy);
    assert_raw_zip_payloads(&destination);
    assert_contents(
        &TesseractFile::open(&destination).unwrap(),
        &fixture.project,
    );
}

#[test]
fn genuine_sentinel_zip64_archive_opens_without_changing_source() {
    let fixture = fixture();
    std::fs::write(&fixture.path, &fixture.original).unwrap();
    let file = TesseractFile::open(&fixture.path).unwrap();
    assert_contents(&file, &fixture.project);
    assert_raw_zip_payloads(&fixture.path);
    assert_eq!(std::fs::read(&fixture.path).unwrap(), fixture.original);
}

#[test]
fn redundant_zip64_compatibility_does_not_hide_asset_integrity_mismatch() {
    let fixture = fixture();
    let mut corrupt = fixture.legacy.clone();
    corrupt[fixture.asset_data_start] ^= 1;
    std::fs::write(&fixture.path, &corrupt).unwrap();
    let file = TesseractFile::open(&fixture.path).unwrap();
    assert!(file
        .asset("synthetic")
        .unwrap()
        .read_verified_bytes(1024)
        .is_err());
    assert_eq!(std::fs::read(&fixture.path).unwrap(), corrupt);
}
