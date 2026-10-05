use crate::error::IoContext;
use crate::metadata::{invalid, serialize_metadata, METADATA_PATH, PROJECT_PATH};
use crate::{PendingAsset, SaveReport, SaveStrategy, TesseractFileError, TesseractFileMetadata};
use crc32fast::Hasher as Crc32;
use sha2::{Digest, Sha256};
#[cfg(target_os = "macos")]
use std::collections::HashMap;
use std::collections::{BTreeMap, HashSet};
use std::fs::File;
#[cfg(target_os = "macos")]
use std::fs::OpenOptions;
#[cfg(target_os = "macos")]
use std::io::{Cursor, Seek, SeekFrom};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

#[cfg(target_os = "macos")]
const LOCAL_HEADER_SIGNATURE: u32 = 0x0403_4b50;
#[cfg(target_os = "macos")]
const CENTRAL_HEADER_SIGNATURE: u32 = 0x0201_4b50;
#[cfg(target_os = "macos")]
const ZIP64_END_SIGNATURE: u32 = 0x0606_4b50;
#[cfg(target_os = "macos")]
const ZIP64_LOCATOR_SIGNATURE: u32 = 0x0706_4b50;
#[cfg(target_os = "macos")]
const END_SIGNATURE: u32 = 0x0605_4b50;
#[cfg(target_os = "macos")]
const ZIP64_VERSION: u16 = 45;
#[cfg(target_os = "macos")]
const UTF8_FLAG: u16 = 1 << 11;

pub(crate) fn write_new(
    destination: &Path,
    mut metadata: TesseractFileMetadata,
    project_bytes: &[u8],
    mut assets: BTreeMap<String, PendingAsset>,
) -> Result<SaveReport, TesseractFileError> {
    if destination.exists() {
        return Err(invalid(format!(
            "destination already exists: {}",
            destination.display()
        )));
    }
    rewrite_archive(
        None,
        destination,
        &mut metadata,
        project_bytes,
        &mut assets,
        false,
    )
}

pub(crate) fn save(
    source: &Path,
    destination: &Path,
    metadata: &TesseractFileMetadata,
    project_bytes: &[u8],
    pending_assets: &BTreeMap<String, PendingAsset>,
) -> Result<SaveReport, TesseractFileError> {
    // Reused central records keep their stored local-header offsets, so the
    // append route is only valid for archives without prepended bytes.
    #[cfg(target_os = "macos")]
    if ZipArchive::new(File::open(source).at(source)?)?.offset() == 0 {
        if let Some(temporary_path) = clone_to_sibling(source, destination)? {
            let result =
                append_changed_entries(&temporary_path, metadata, project_bytes, pending_assets)
                    .and_then(|report| {
                        crate::TesseractFile::open(&temporary_path)?;
                        publish_clone(&temporary_path, destination)?;
                        Ok(report)
                    });
            if result.is_err() {
                let _ = std::fs::remove_file(&temporary_path);
            }
            return result;
        }
    }

    save_compacted(source, destination, metadata, project_bytes, pending_assets)
}

pub(crate) fn save_compacted(
    source: &Path,
    destination: &Path,
    metadata: &TesseractFileMetadata,
    project_bytes: &[u8],
    pending_assets: &BTreeMap<String, PendingAsset>,
) -> Result<SaveReport, TesseractFileError> {
    let mut metadata = metadata.clone();
    let mut pending_assets = pending_assets.clone();
    rewrite_archive(
        Some(source),
        destination,
        &mut metadata,
        project_bytes,
        &mut pending_assets,
        true,
    )
}

fn rewrite_archive(
    source: Option<&Path>,
    destination: &Path,
    metadata: &mut TesseractFileMetadata,
    project_bytes: &[u8],
    pending_assets: &mut BTreeMap<String, PendingAsset>,
    replace: bool,
) -> Result<SaveReport, TesseractFileError> {
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).at(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).at(parent)?;
    let mut writer = ZipWriter::new(temporary.as_file_mut());
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o644);

    let pending_paths: HashSet<&str> = pending_assets
        .values()
        .map(|asset| asset.descriptor.path.as_str())
        .collect();
    let active_paths: HashSet<&str> = metadata
        .assets
        .values()
        .map(|asset| asset.path.as_str())
        .collect();
    let mut reused_assets = 0_usize;

    if let Some(source) = source {
        let source_file = File::open(source).at(source)?;
        let mut archive = ZipArchive::new(source_file)?;
        for index in 0..archive.len() {
            let file = archive.by_index_raw(index)?;
            let name = file.name().to_string();
            if active_paths.contains(name.as_str()) && !pending_paths.contains(name.as_str()) {
                writer.raw_copy_file(file)?;
                reused_assets += 1;
            }
        }
    }

    writer.start_file(
        PROJECT_PATH,
        options.large_file(needs_zip64(project_bytes.len() as u64)),
    )?;
    writer.write_all(project_bytes).at(destination)?;

    for (asset_id, pending) in pending_assets.iter_mut() {
        // zip aborts a non-ZIP64 entry once it passes 4 GiB, so size it up front.
        let length = std::fs::metadata(&pending.source)
            .at(&pending.source)?
            .len()
            .max(pending.descriptor.byte_length);
        writer.start_file(
            &pending.descriptor.path,
            options.large_file(needs_zip64(length)),
        )?;
        let integrity = copy_pending_asset(pending, &mut writer, destination)?;
        if !pending.integrity_ready {
            pending.descriptor.byte_length = integrity.byte_length;
            pending.descriptor.sha256 = integrity.sha256;
            pending.crc32 = integrity.crc32;
            pending.integrity_ready = true;
            metadata
                .assets
                .insert(asset_id.clone(), pending.descriptor.clone());
        }
    }
    metadata.validate()?;
    let metadata_bytes = serialize_metadata(metadata)?;

    writer.start_file(
        METADATA_PATH,
        options.large_file(needs_zip64(metadata_bytes.len() as u64)),
    )?;
    writer.write_all(&metadata_bytes).at(destination)?;
    writer.finish()?;
    temporary.as_file_mut().sync_all().at(temporary.path())?;
    let bytes_written = temporary.as_file().metadata().at(temporary.path())?.len();

    publish_named(temporary, destination, replace)?;
    Ok(SaveReport {
        strategy: SaveStrategy::RawCopyRewrite,
        bytes_written,
        reused_assets,
    })
}

#[cfg(target_os = "macos")]
fn clone_to_sibling(
    source: &Path,
    destination: &Path,
) -> Result<Option<PathBuf>, TesseractFileError> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).at(parent)?;
    let reservation = tempfile::Builder::new()
        .prefix(".tesseract-save-")
        .suffix(".tmp")
        .tempfile_in(parent)
        .at(parent)?;
    let temporary_path = reservation.path().to_path_buf();
    reservation.close().at(&temporary_path)?;

    let source_c = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| invalid("source path contains NUL"))?;
    let destination_c = CString::new(temporary_path.as_os_str().as_bytes())
        .map_err(|_| invalid("destination path contains NUL"))?;
    // SAFETY: both C strings are NUL-terminated and remain alive for the call.
    let result = unsafe { libc::clonefile(source_c.as_ptr(), destination_c.as_ptr(), 0) };
    if result == 0 {
        Ok(Some(temporary_path))
    } else {
        let _ = std::fs::remove_file(&temporary_path);
        Ok(None)
    }
}

#[cfg(target_os = "macos")]
fn append_changed_entries(
    path: &Path,
    metadata: &TesseractFileMetadata,
    project_bytes: &[u8],
    pending_assets: &BTreeMap<String, PendingAsset>,
) -> Result<SaveReport, TesseractFileError> {
    // Serialize and size-check before mutating the cloned destination. The
    // metadata has already been finalized by prepare_save_metadata.
    let metadata_bytes = serialize_metadata(metadata)?;
    let (central_start, existing_records) = read_central_records(path)?;
    let pending_paths: HashSet<&str> = pending_assets
        .values()
        .map(|asset| asset.descriptor.path.as_str())
        .collect();
    let records_by_name: HashMap<&str, &CentralRecord> = existing_records
        .iter()
        .map(|record| (record.name.as_str(), record))
        .collect();
    let mut central_records = Vec::with_capacity(metadata.assets.len() + 2);
    let mut reused_assets = 0_usize;
    for descriptor in metadata.assets.values() {
        let path = descriptor.path.as_str();
        if pending_paths.contains(path) {
            continue;
        }
        let record = records_by_name
            .get(path)
            .ok_or_else(|| invalid(format!("missing central record for {path:?}")))?;
        central_records.push(record.bytes.clone());
        reused_assets += 1;
    }

    let mut output = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .at(path)?;
    output.set_len(central_start).at(path)?;
    output.seek(SeekFrom::Start(central_start)).at(path)?;

    central_records.push(write_stored_bytes(
        &mut output,
        PROJECT_PATH,
        project_bytes,
        path,
    )?);
    for pending in pending_assets.values() {
        central_records.push(write_stored_pending(&mut output, pending, path)?);
    }
    central_records.push(write_stored_bytes(
        &mut output,
        METADATA_PATH,
        &metadata_bytes,
        path,
    )?);

    let new_central_start = output.stream_position().at(path)?;
    for record in &central_records {
        output.write_all(record).at(path)?;
    }
    let central_end = output.stream_position().at(path)?;
    write_zip64_footer(
        &mut output,
        new_central_start,
        central_end - new_central_start,
        central_records.len() as u64,
        path,
    )?;
    let end = output.stream_position().at(path)?;
    output.set_len(end).at(path)?;
    output.sync_all().at(path)?;

    Ok(SaveReport {
        strategy: SaveStrategy::ReflinkAppend,
        bytes_written: end - central_start,
        reused_assets,
    })
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct CentralRecord {
    name: String,
    bytes: Vec<u8>,
}

#[cfg(target_os = "macos")]
fn read_central_records(path: &Path) -> Result<(u64, Vec<CentralRecord>), TesseractFileError> {
    let archive = ZipArchive::new(File::open(path).at(path)?)?;
    let central_start = archive.central_directory_start();
    let count = archive.len();
    drop(archive);

    let mut file = File::open(path).at(path)?;
    file.seek(SeekFrom::Start(central_start)).at(path)?;
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        let mut fixed = [0_u8; 46];
        file.read_exact(&mut fixed).at(path)?;
        if u32::from_le_bytes(fixed[0..4].try_into().expect("fixed four-byte signature"))
            != CENTRAL_HEADER_SIGNATURE
        {
            return Err(invalid("invalid central-directory record signature"));
        }
        let name_len = u16::from_le_bytes(fixed[28..30].try_into().expect("fixed name length"));
        let extra_len = u16::from_le_bytes(fixed[30..32].try_into().expect("fixed extra length"));
        let comment_len =
            u16::from_le_bytes(fixed[32..34].try_into().expect("fixed comment length"));
        let variable_len =
            usize::from(name_len) + usize::from(extra_len) + usize::from(comment_len);
        let mut variable = vec![0_u8; variable_len];
        file.read_exact(&mut variable).at(path)?;
        let name = std::str::from_utf8(&variable[..usize::from(name_len)])
            .map_err(|_| invalid("central-directory name must be UTF-8"))?
            .to_string();
        let mut bytes = fixed.to_vec();
        bytes.extend_from_slice(&variable);
        records.push(CentralRecord { name, bytes });
    }
    Ok((central_start, records))
}

#[cfg(target_os = "macos")]
fn write_stored_bytes(
    output: &mut File,
    name: &str,
    bytes: &[u8],
    path: &Path,
) -> Result<Vec<u8>, TesseractFileError> {
    let mut crc32 = Crc32::new();
    crc32.update(bytes);
    let mut source = Cursor::new(bytes);
    write_stored_entry(
        output,
        name,
        bytes.len() as u64,
        crc32.finalize(),
        &mut source,
        None,
        path,
    )
}

#[cfg(target_os = "macos")]
fn write_stored_pending(
    output: &mut File,
    pending: &PendingAsset,
    path: &Path,
) -> Result<Vec<u8>, TesseractFileError> {
    let mut source = File::open(&pending.source).at(&pending.source)?;
    write_stored_entry(
        output,
        &pending.descriptor.path,
        pending.descriptor.byte_length,
        pending.crc32,
        &mut source,
        Some(&pending.descriptor.sha256),
        path,
    )
}

#[cfg(target_os = "macos")]
fn write_stored_entry(
    output: &mut File,
    name: &str,
    size: u64,
    crc32: u32,
    source: &mut impl Read,
    expected_sha256: Option<&str>,
    path: &Path,
) -> Result<Vec<u8>, TesseractFileError> {
    let name_bytes = name.as_bytes();
    let name_len =
        u16::try_from(name_bytes.len()).map_err(|_| invalid("entry name is too long"))?;
    let local_offset = output.stream_position().at(path)?;
    let local_extra = zip64_extra(size, local_offset, false);

    write_u32(output, LOCAL_HEADER_SIGNATURE, path)?;
    write_u16(output, ZIP64_VERSION, path)?;
    write_u16(output, UTF8_FLAG, path)?;
    write_u16(output, 0, path)?;
    write_u16(output, 0, path)?;
    write_u16(output, 0, path)?;
    write_u32(output, crc32, path)?;
    write_u32(output, u32::MAX, path)?;
    write_u32(output, u32::MAX, path)?;
    write_u16(output, name_len, path)?;
    write_u16(output, local_extra.len() as u16, path)?;
    output.write_all(name_bytes).at(path)?;
    output.write_all(&local_extra).at(path)?;

    let mut remaining = size;
    let mut observed_sha256 = Sha256::new();
    let mut observed_crc32 = Crc32::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    while remaining > 0 {
        let requested = usize::try_from(remaining.min(buffer.len() as u64))
            .expect("bounded by the in-memory buffer length");
        let read = source.read(&mut buffer[..requested]).at(path)?;
        if read == 0 {
            return Err(invalid(format!(
                "entry {name:?} became shorter while saving"
            )));
        }
        output.write_all(&buffer[..read]).at(path)?;
        observed_sha256.update(&buffer[..read]);
        observed_crc32.update(&buffer[..read]);
        remaining -= read as u64;
    }
    if source.read(&mut [0_u8; 1]).at(path)? != 0 {
        return Err(invalid(format!("entry {name:?} grew while saving")));
    }
    if observed_crc32.finalize() != crc32 {
        return Err(invalid(format!("entry {name:?} changed while saving")));
    }
    if let Some(expected) = expected_sha256 {
        if !format!("{:x}", observed_sha256.finalize()).eq_ignore_ascii_case(expected) {
            return Err(invalid(format!("entry {name:?} changed while saving")));
        }
    }

    let central_extra = zip64_extra(size, local_offset, true);
    let mut central = Vec::with_capacity(46 + name_bytes.len() + central_extra.len());
    write_u32_vec(&mut central, CENTRAL_HEADER_SIGNATURE);
    write_u16_vec(&mut central, (3 << 8) | ZIP64_VERSION);
    write_u16_vec(&mut central, ZIP64_VERSION);
    write_u16_vec(&mut central, UTF8_FLAG);
    write_u16_vec(&mut central, 0);
    write_u16_vec(&mut central, 0);
    write_u16_vec(&mut central, 0);
    write_u32_vec(&mut central, crc32);
    write_u32_vec(&mut central, u32::MAX);
    write_u32_vec(&mut central, u32::MAX);
    write_u16_vec(&mut central, name_len);
    write_u16_vec(&mut central, central_extra.len() as u16);
    write_u16_vec(&mut central, 0);
    write_u16_vec(&mut central, 0);
    write_u16_vec(&mut central, 0);
    write_u32_vec(&mut central, 0o100644 << 16);
    write_u32_vec(&mut central, u32::MAX);
    central.extend_from_slice(name_bytes);
    central.extend_from_slice(&central_extra);
    Ok(central)
}

#[cfg(target_os = "macos")]
fn zip64_extra(size: u64, local_offset: u64, include_offset: bool) -> Vec<u8> {
    let payload_len = if include_offset { 24_u16 } else { 16_u16 };
    let mut extra = Vec::with_capacity(usize::from(payload_len) + 4);
    write_u16_vec(&mut extra, 0x0001);
    write_u16_vec(&mut extra, payload_len);
    extra.extend_from_slice(&size.to_le_bytes());
    extra.extend_from_slice(&size.to_le_bytes());
    if include_offset {
        extra.extend_from_slice(&local_offset.to_le_bytes());
    }
    extra
}

#[cfg(target_os = "macos")]
fn write_zip64_footer(
    output: &mut File,
    central_start: u64,
    central_size: u64,
    entries: u64,
    path: &Path,
) -> Result<(), TesseractFileError> {
    let zip64_end_offset = output.stream_position().at(path)?;
    write_u32(output, ZIP64_END_SIGNATURE, path)?;
    output.write_all(&44_u64.to_le_bytes()).at(path)?;
    write_u16(output, ZIP64_VERSION, path)?;
    write_u16(output, ZIP64_VERSION, path)?;
    write_u32(output, 0, path)?;
    write_u32(output, 0, path)?;
    output.write_all(&entries.to_le_bytes()).at(path)?;
    output.write_all(&entries.to_le_bytes()).at(path)?;
    output.write_all(&central_size.to_le_bytes()).at(path)?;
    output.write_all(&central_start.to_le_bytes()).at(path)?;

    write_u32(output, ZIP64_LOCATOR_SIGNATURE, path)?;
    write_u32(output, 0, path)?;
    output.write_all(&zip64_end_offset.to_le_bytes()).at(path)?;
    write_u32(output, 1, path)?;

    write_u32(output, END_SIGNATURE, path)?;
    write_u16(output, 0, path)?;
    write_u16(output, 0, path)?;
    write_u16(output, u16::MAX, path)?;
    write_u16(output, u16::MAX, path)?;
    write_u32(output, u32::MAX, path)?;
    write_u32(output, u32::MAX, path)?;
    write_u16(output, 0, path)
}

#[cfg(target_os = "macos")]
fn publish_clone(temporary: &Path, destination: &Path) -> Result<(), TesseractFileError> {
    std::fs::rename(temporary, destination).at(destination)?;
    sync_parent(destination)
}

struct ObservedIntegrity {
    byte_length: u64,
    sha256: String,
    crc32: u32,
}

/// Whether a Stored entry of `length` bytes needs ZIP64 size fields.
fn needs_zip64(length: u64) -> bool {
    length >= u64::from(u32::MAX)
}

fn copy_pending_asset(
    pending: &PendingAsset,
    destination: &mut impl Write,
    output_path: &Path,
) -> Result<ObservedIntegrity, TesseractFileError> {
    let mut source = File::open(&pending.source).at(&pending.source)?;
    let mut sha256 = Sha256::new();
    let mut crc32 = Crc32::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = source.read(&mut buffer).at(&pending.source)?;
        if read == 0 {
            break;
        }
        total += read as u64;
        sha256.update(&buffer[..read]);
        crc32.update(&buffer[..read]);
        destination.write_all(&buffer[..read]).at(output_path)?;
    }
    let observed = ObservedIntegrity {
        byte_length: total,
        sha256: format!("{:x}", sha256.finalize()),
        crc32: crc32.finalize(),
    };
    if total != pending.descriptor.byte_length
        || (pending.integrity_ready
            && (observed.sha256 != pending.descriptor.sha256 || observed.crc32 != pending.crc32))
    {
        return Err(invalid(format!(
            "asset source {} changed while saving",
            pending.source.display()
        )));
    }
    Ok(observed)
}

fn publish_named(
    temporary: tempfile::NamedTempFile,
    destination: &Path,
    replace: bool,
) -> Result<(), TesseractFileError> {
    let result = if replace {
        temporary.persist(destination)
    } else {
        temporary.persist_noclobber(destination)
    };
    result.map_err(|error| TesseractFileError::Io {
        path: PathBuf::from(destination),
        source: error.error,
    })?;
    sync_parent(destination)
}

#[cfg(unix)]
fn sync_parent(destination: &Path) -> Result<(), TesseractFileError> {
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .at(parent)?;
    Ok(())
}

/// Windows has no directory fsync, so a save is durable once the file's own
/// data is flushed; the rename itself is not separately made durable.
#[cfg(not(unix))]
fn sync_parent(_destination: &Path) -> Result<(), TesseractFileError> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn write_u16(output: &mut File, value: u16, path: &Path) -> Result<(), TesseractFileError> {
    output.write_all(&value.to_le_bytes()).at(path)
}

#[cfg(target_os = "macos")]
fn write_u32(output: &mut File, value: u32, path: &Path) -> Result<(), TesseractFileError> {
    output.write_all(&value.to_le_bytes()).at(path)
}

#[cfg(target_os = "macos")]
fn write_u16_vec(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

#[cfg(target_os = "macos")]
fn write_u32_vec(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}
