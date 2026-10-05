//! Browser File/Blob access to the same portable file format.
use crate::error::IoContext;
use crate::metadata::{
    invalid, serialize_metadata, validate_archive_path, METADATA_PATH, PROJECT_PATH,
};
use crate::{
    parse_project, sha256_bytes, validate_project_asset_refs, EditableFxCompositionDocument,
    EntryInfo, Generator, TesseractFileError, TesseractFileMetadata,
};
use async_zip::base::read1::{seek::ZipArchiveReader, ZipOptions};
use async_zip::spec::headers1::Compression;
use futures_lite::io::{AsyncRead, AsyncSeek, BufReader};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::io::{self, Cursor, Read, SeekFrom, Write};
use std::pin::Pin;
use std::task::{Context, Poll};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::Blob;

const CHUNK_BYTES: usize = 64 * 1024;

/// An opened browser file. Assets remain Blob slices of the selected file.
pub struct BrowserTesseractFile {
    metadata: TesseractFileMetadata,
    project: EditableFxCompositionDocument,
    assets: BTreeMap<String, Blob>,
}

impl BrowserTesseractFile {
    pub async fn open(file: Blob) -> Result<Self, TesseractFileError> {
        let archive_bytes = file.size() as u64;
        // These are physical bounds, not product quotas: each central record
        // occupies at least 46 bytes, and Stored entries cannot exceed the file.
        let options = ZipOptions {
            max_cd_num_files: archive_bytes / 46,
            max_cd_num_files_load: archive_bytes / 46,
            max_cd_size_in_bytes: archive_bytes,
            max_uncompressed_size_per_file: archive_bytes,
            max_compressed_size_per_file: archive_bytes,
            ..Default::default()
        };
        let mut zip = ZipArchiveReader::open_with_options(
            BufReader::with_capacity(CHUNK_BYTES, BlobReader::new(file.clone())),
            options,
        )
        .await?;
        let directory_start = zip.ceocdr().cd_offset()?;
        let mut entries = HashMap::new();
        let mut names = HashSet::new();
        let mut ranges = Vec::new();
        for index in 0..zip.cdrs().len() {
            let central = &zip.cdrs()[index];
            let name = std::str::from_utf8(central.insecure_file_name.as_bytes())
                .map_err(|_| invalid("archive entry names must be UTF-8"))?
                .to_owned();
            validate_archive_path(&name)?;
            if !names.insert(name.to_ascii_lowercase()) {
                return Err(invalid(format!(
                    "duplicate or case-colliding entry {name:?}"
                )));
            }
            let size = central.uncompressed_size()?;
            let offset = central.lfh_offset()?;
            let crc32 = central.cdrh.crc;
            let unix_type = (central.cdrh.exter_attr >> 16) & 0o170000;
            if central.cdrh.compression != Compression::Stored
                || central.compressed_size()? != size
                || name.ends_with('/')
                || central.cdrh.exter_attr & 0x10 != 0
                || matches!(unix_type, 0o040000 | 0o120000)
            {
                return Err(invalid(format!(
                    "entry {name:?} must be a regular Stored file"
                )));
            }
            let entry = zip.file(index).await?;
            let header = &entry.lf().lfh;
            let start = offset
                .checked_add(
                    30 + u64::from(header.file_name_length) + u64::from(header.extra_field_length),
                )
                .ok_or_else(|| invalid("entry offset overflow"))?;
            let end = start
                .checked_add(size)
                .ok_or_else(|| invalid("entry size overflow"))?;
            if end > directory_start || end > file.size() as u64 {
                return Err(invalid("entry extends beyond file data"));
            }
            // async_zip's flags type does not expose the encryption bit.
            let flags = read_slice(&file, offset + 6, offset + 8).await.at(&name)?;
            if flags.len() != 2 || u16::from_le_bytes([flags[0], flags[1]]) & 0x41 != 0 {
                return Err(invalid(format!("encrypted entry {name:?} is unsupported")));
            }
            ranges.push((offset, end));
            entries.insert(
                name,
                EntryInfo {
                    data_start: start,
                    size,
                    crc32,
                },
            );
        }
        ranges.sort_unstable();
        if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
            return Err(invalid("overlapping file entries"));
        }
        let metadata_bytes = read_document(&file, &entries, METADATA_PATH, None).await?;
        let metadata: TesseractFileMetadata = serde_json::from_slice(&metadata_bytes)?;
        metadata.validate()?;
        let project_bytes = read_document(
            &file,
            &entries,
            PROJECT_PATH,
            Some(metadata.project.byte_length),
        )
        .await?;
        let project = parse_project(&metadata, &project_bytes, &entries)?;
        let assets = metadata
            .assets
            .iter()
            .map(|(id, descriptor)| {
                let entry = &entries[&descriptor.path];
                let blob = file
                    .slice_with_f64_and_f64_and_content_type(
                        entry.data_start as f64,
                        (entry.data_start + entry.size) as f64,
                        &descriptor.content_type,
                    )
                    .map_err(browser_error)?;
                Ok((id.clone(), blob))
            })
            .collect::<Result<_, TesseractFileError>>()?;
        Ok(Self {
            metadata,
            project,
            assets,
        })
    }

    pub fn metadata(&self) -> &TesseractFileMetadata {
        &self.metadata
    }
    pub fn project(&self) -> &EditableFxCompositionDocument {
        &self.project
    }
    pub fn assets(&self) -> &BTreeMap<String, Blob> {
        &self.assets
    }

    /// Packages a project snapshot with the original assets; does not overwrite the source.
    pub async fn save(
        &self,
        document: &EditableFxCompositionDocument,
        generator: Generator,
    ) -> Result<Blob, TesseractFileError> {
        validate_project_asset_refs(document, &self.metadata, |_| false)?;
        let project = document.to_json_vec()?;
        let mut metadata = self.metadata.clone();
        metadata.modified_at = js_sys::Date::new_0().to_iso_string().into();
        metadata.generator = generator;
        metadata.fx_schema_version = Some(fx_schema::FX_SCHEMA_REVISION);
        metadata.project.byte_length = project.len() as u64;
        metadata.project.sha256 = sha256_bytes(&project);
        metadata.validate()?;
        let output = io::BufWriter::with_capacity(CHUNK_BYTES, BlobWriter(js_sys::Array::new()));
        let mut zip = zip::ZipWriter::new_stream(output);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file(METADATA_PATH, options)?;
        zip.write_all(&serialize_metadata(&metadata)?)
            .at(METADATA_PATH)?;
        zip.start_file(PROJECT_PATH, options)?;
        zip.write_all(&project).at(PROJECT_PATH)?;
        for (id, descriptor) in &metadata.assets {
            zip.start_file(
                &descriptor.path,
                options.large_file(descriptor.byte_length >= u32::MAX as u64),
            )?;
            let blob = &self.assets[id];
            let mut start = 0;
            // Asset bytes are Blob slices of the opened file and were never
            // verified on open; check them against the recorded digest so a
            // save cannot republish corrupt bytes under the original SHA-256.
            let mut digest = Sha256::new();
            while start < descriptor.byte_length {
                let end = (start + CHUNK_BYTES as u64).min(descriptor.byte_length);
                let bytes = read_slice(blob, start, end).await.at(&descriptor.path)?;
                if bytes.len() as u64 != end - start {
                    return Err(invalid("asset file changed while saving"));
                }
                digest.update(&bytes);
                zip.write_all(&bytes).at(&descriptor.path)?;
                start = end;
            }
            if !format!("{:x}", digest.finalize()).eq_ignore_ascii_case(&descriptor.sha256) {
                return Err(invalid(
                    "asset bytes do not match metadata integrity fields",
                ));
            }
        }
        let output = zip
            .finish()?
            .into_inner()
            .into_inner()
            .map_err(|e| e.into_error())
            .at("saved file")?;
        Blob::new_with_blob_sequence(&output.0).map_err(browser_error)
    }
}

async fn read_document(
    file: &Blob,
    entries: &HashMap<String, EntryInfo>,
    name: &str,
    expected_size: Option<u64>,
) -> Result<Vec<u8>, TesseractFileError> {
    let entry = entries
        .get(name)
        .ok_or_else(|| invalid(format!("missing {name}")))?;
    if expected_size.is_some_and(|size| entry.size != size) {
        return Err(invalid(format!(
            "{name} length does not match its descriptor"
        )));
    }
    let bytes = read_slice(file, entry.data_start, entry.data_start + entry.size)
        .await
        .at(name)?;
    if bytes.len() as u64 != entry.size || crc32fast::hash(&bytes) != entry.crc32 {
        return Err(invalid(format!("{name} is truncated or corrupt")));
    }
    Ok(bytes)
}

fn browser_error(error: JsValue) -> TesseractFileError {
    invalid(format!("browser file operation failed: {error:?}"))
}
async fn read_slice(blob: &Blob, start: u64, end: u64) -> io::Result<Vec<u8>> {
    let slice = blob
        .slice_with_f64_and_f64(start as f64, end as f64)
        .map_err(|e| io::Error::other(format!("{e:?}")))?;
    let buffer = JsFuture::from(slice.array_buffer())
        .await
        .map_err(|e| io::Error::other(format!("{e:?}")))?;
    let array = js_sys::Uint8Array::new(&buffer);
    let len = usize::try_from(array.length()).map_err(io::Error::other)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(io::Error::other)?;
    bytes.resize(len, 0);
    array.copy_to(&mut bytes);
    Ok(bytes)
}

type PendingRead = Pin<Box<dyn Future<Output = io::Result<Vec<u8>>>>>;

struct BlobReader {
    blob: Blob,
    position: u64,
    buffer: Cursor<Vec<u8>>,
    pending: Option<PendingRead>,
}
impl BlobReader {
    fn new(blob: Blob) -> Self {
        Self {
            blob,
            position: 0,
            buffer: Cursor::new(Vec::new()),
            pending: None,
        }
    }
}
impl AsyncRead for BlobReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if out.is_empty() || self.position >= self.blob.size() as u64 {
            return Poll::Ready(Ok(0));
        }
        if self.buffer.position() == self.buffer.get_ref().len() as u64 {
            if self.pending.is_none() {
                let blob = self.blob.clone();
                let start = self.position;
                let end = (start + out.len().min(CHUNK_BYTES) as u64).min(blob.size() as u64);
                self.pending = Some(Box::pin(async move { read_slice(&blob, start, end).await }));
            }
            let bytes = std::task::ready!(self.pending.as_mut().unwrap().as_mut().poll(cx));
            self.pending = None;
            self.buffer = Cursor::new(bytes?);
        }
        let read = Read::read(&mut self.buffer, out)?;
        self.position += read as u64;
        Poll::Ready(Ok(read))
    }
}
impl AsyncSeek for BlobReader {
    fn poll_seek(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        position: SeekFrom,
    ) -> Poll<io::Result<u64>> {
        let next = match position {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::Current(n) => self.position.checked_add_signed(n),
            SeekFrom::End(n) => (self.blob.size() as u64).checked_add_signed(n),
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid file seek"))?;
        self.position = next;
        self.pending = None;
        self.buffer = Cursor::new(Vec::new());
        Poll::Ready(Ok(next))
    }
}
struct BlobWriter(js_sys::Array);
impl Write for BlobWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        // Copy out of WASM: Blob parts must survive subsequent memory growth.
        let part = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
        let blob = Blob::new_with_u8_array_sequence(&part)
            .map_err(|e| io::Error::other(format!("{e:?}")))?;
        self.0.push(&blob);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
