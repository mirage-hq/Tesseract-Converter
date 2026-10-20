//! Read-only compatibility for redundant, stale ZIP64 central-directory values.
//!
//! Some legacy saves updated the complete classic fields but retained an old
//! 24-byte ZIP64 extra. Classic fields are authoritative unless they contain
//! sentinels. Do not rewrite the source or relax payload/integrity validation.

use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use zip::ZipArchive;

use crate::{error::IoContext, metadata::invalid, TesseractFileError};

const CENTRAL_HEADER_SIZE: usize = 46;
const ZIP64_VALUES_SIZE: usize = 24;

#[derive(Debug)]
struct Patch {
    offset: u64,
    bytes: [u8; ZIP64_VALUES_SIZE],
}

/// Streaming metadata view; file payloads and source bytes are never changed.
#[derive(Debug)]
pub(super) struct ArchiveReader<R> {
    inner: R,
    position: u64,
    patches: Vec<Patch>,
}

impl<R: Read + Seek> ArchiveReader<R> {
    fn new(mut inner: R) -> io::Result<Self> {
        let position = inner.stream_position()?;
        Ok(Self {
            inner,
            position,
            patches: Vec::new(),
        })
    }
}

impl<R: Read> Read for ArchiveReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        let start = self.position;
        // Supported targets have at most 64-bit usize; the read length fits u64.
        let end = start.checked_add(count as u64).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "ZIP read position overflow")
        })?;
        let first = self.patches.partition_point(|patch| {
            patch.offset.saturating_add(ZIP64_VALUES_SIZE as u64) <= start
        });
        for patch in self.patches[first..]
            .iter()
            .take_while(|patch| patch.offset < end)
        {
            let overlap_start = start.max(patch.offset);
            let overlap_end = end.min(patch.offset + ZIP64_VALUES_SIZE as u64);
            let destination = usize::try_from(overlap_start - start)
                .expect("intersection lies within the read buffer");
            let source = usize::try_from(overlap_start - patch.offset)
                .expect("intersection lies within the 24-byte patch");
            let length = usize::try_from(overlap_end - overlap_start)
                .expect("intersection is at most 24 bytes");
            buffer[destination..destination + length]
                .copy_from_slice(&patch.bytes[source..source + length]);
        }
        self.position = end;
        Ok(count)
    }
}

impl<R: Seek> Seek for ArchiveReader<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.position = self.inner.seek(position)?;
        Ok(self.position)
    }
}

/// Apply bounded legacy metadata corrections before resolving local headers.
pub(super) fn open_archive<R: Read + Seek>(
    reader: R,
    path: &Path,
) -> Result<ZipArchive<ArchiveReader<R>>, TesseractFileError> {
    // zip's constructor reads only the central directory, not local headers.
    // Its completed read extent bounds our scan of those validated records.
    let archive = ZipArchive::new(ArchiveReader::new(reader).at(path)?)?;
    let start = archive.central_directory_start();
    let count = archive.len();
    let mut reader = archive.into_inner();
    let end = reader.position;
    if start > end {
        return Err(invalid("invalid central-directory read extent"));
    }
    reader.seek(SeekFrom::Start(start)).at(path)?;
    let mut patches = Vec::new();
    for _ in 0..count {
        let record_start = reader.position;
        if end.saturating_sub(record_start) < CENTRAL_HEADER_SIZE as u64 {
            return Err(invalid("truncated central-directory header"));
        }
        let mut header = [0; CENTRAL_HEADER_SIZE];
        reader.read_exact(&mut header).at(path)?;
        let length = record_length(&header)?;
        let record_end = record_start
            .checked_add(
                u64::try_from(length)
                    .map_err(|_| invalid("central-directory record length overflow"))?,
            )
            .ok_or_else(|| invalid("central-directory position overflow"))?;
        if record_end > end {
            return Err(invalid("central-directory record exceeds its read extent"));
        }
        let mut record = header.to_vec();
        record.resize(length, 0);
        reader
            .read_exact(&mut record[CENTRAL_HEADER_SIZE..])
            .at(path)?;
        for mut patch in record_patches(&record)? {
            patch.offset = record_start
                .checked_add(patch.offset)
                .ok_or_else(|| invalid("ZIP64 patch position overflow"))?;
            patches.push(patch);
        }
    }
    if reader.position != end {
        return Err(invalid(
            "central-directory entry count does not match its extent",
        ));
    }
    reader.patches = patches;
    reader.seek(SeekFrom::Start(0)).at(path)?;
    Ok(ZipArchive::new(reader)?)
}

/// Retained records keep their local-header positions during prefix reuse.
/// Canonicalize their redundant values before publishing the new directory.
#[cfg(any(target_os = "macos", test))]
pub(super) fn normalize_central_record(record: &mut [u8]) -> Result<(), TesseractFileError> {
    for patch in record_patches(record)? {
        let offset = usize::try_from(patch.offset)
            .expect("patch offset lies within the in-memory central record");
        record[offset..offset + ZIP64_VALUES_SIZE].copy_from_slice(&patch.bytes);
    }
    Ok(())
}

fn record_length(header: &[u8]) -> Result<usize, TesseractFileError> {
    if header.len() < CENTRAL_HEADER_SIZE || &header[..4] != b"PK\x01\x02" {
        return Err(invalid("invalid central-directory record signature"));
    }
    Ok(CENTRAL_HEADER_SIZE
        + usize::from(word(header, 28))
        + usize::from(word(header, 30))
        + usize::from(word(header, 32)))
}

fn record_patches(record: &[u8]) -> Result<Vec<Patch>, TesseractFileError> {
    if record_length(record)? != record.len() {
        return Err(invalid("central-directory record length mismatch"));
    }
    let sizes = [dword(record, 24), dword(record, 20), dword(record, 42)];
    // Leave genuine (including mixed) ZIP64 and multi-disk layouts untouched.
    if sizes.contains(&u32::MAX) || word(record, 34) != 0 {
        return Ok(Vec::new());
    }
    let mut values = [0; ZIP64_VALUES_SIZE];
    for (index, value) in sizes.into_iter().enumerate() {
        values[index * 8..index * 8 + 8].copy_from_slice(&u64::from(value).to_le_bytes());
    }
    let extra_start = CENTRAL_HEADER_SIZE + usize::from(word(record, 28));
    let extra_end = extra_start + usize::from(word(record, 30));
    let mut offset = extra_start;
    let mut patches = Vec::new();
    while offset < extra_end {
        if extra_end - offset < 4 {
            return Err(invalid("truncated ZIP extra-field header"));
        }
        let kind = word(record, offset);
        let length = usize::from(word(record, offset + 2));
        let data_start = offset + 4;
        let data_end = data_start + length;
        if data_end > extra_end {
            return Err(invalid("ZIP extra field exceeds central-directory record"));
        }
        // Only the full three-value legacy layout is unambiguous. Other
        // layouts remain subject to the ZIP library's normal interpretation.
        if kind == 1 && length == ZIP64_VALUES_SIZE && record[data_start..data_end] != values {
            patches.push(Patch {
                offset: u64::try_from(data_start)
                    .map_err(|_| invalid("ZIP64 patch offset overflow"))?,
                bytes: values,
            });
        }
        offset = data_end;
    }
    Ok(patches)
}

fn word(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        bytes[offset..offset + 2]
            .try_into()
            .expect("fixed-width field in a checked central-directory record"),
    )
}

fn dword(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("fixed-width field in a checked central-directory record"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[derive(Debug)]
    struct ShortReads(Cursor<Vec<u8>>);

    impl Read for ShortReads {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            let length = bytes.len().min(3);
            self.0.read(&mut bytes[..length])
        }
    }

    impl Seek for ShortReads {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.0.seek(position)
        }
    }

    #[test]
    fn zip64_overlay_handles_short_reads_seeks_crossings_and_eof() {
        let mut reader = ArchiveReader::new(ShortReads(Cursor::new(vec![0; 80]))).unwrap();
        reader.patches = vec![
            Patch {
                offset: 5,
                bytes: [1; 24],
            },
            Patch {
                offset: 35,
                bytes: [2; 24],
            },
        ];
        let mut expected = vec![0; 80];
        expected[5..29].fill(1);
        expected[35..59].fill(2);
        let mut full = ArchiveReader::new(Cursor::new(vec![0; 80])).unwrap();
        full.patches = vec![
            Patch {
                offset: 5,
                bytes: [1; 24],
            },
            Patch {
                offset: 35,
                bytes: [2; 24],
            },
        ];
        let mut whole = [0; 80];
        assert_eq!(full.read(&mut whole).unwrap(), 80);
        assert_eq!(whole.as_slice(), expected);
        let mut actual = Vec::new();
        reader.read_to_end(&mut actual).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(reader.read(&mut [0; 8]).unwrap(), 0);
        reader.seek(SeekFrom::Start(17)).unwrap();
        let mut interior = [0; 30];
        reader.read_exact(&mut interior).unwrap();
        assert_eq!(&interior, &expected[17..47]);
        reader.seek(SeekFrom::End(-2)).unwrap();
        let mut tail = [7; 8];
        assert_eq!(reader.read(&mut tail).unwrap(), 2);
        assert_eq!(tail, [0, 0, 7, 7, 7, 7, 7, 7]);
    }

    fn record() -> Vec<u8> {
        let mut record = vec![0; 46];
        record[..4].copy_from_slice(b"PK\x01\x02");
        record[20..24].copy_from_slice(&7_u32.to_le_bytes());
        record[24..28].copy_from_slice(&7_u32.to_le_bytes());
        record[42..46].copy_from_slice(&9_u32.to_le_bytes());
        record[30..32].copy_from_slice(&28_u16.to_le_bytes());
        record.extend_from_slice(&1_u16.to_le_bytes());
        record.extend_from_slice(&24_u16.to_le_bytes());
        record.extend_from_slice(&[0; 24]);
        record
    }

    #[test]
    fn zip64_normalization_leaves_sentinels_and_other_layouts_unchanged() {
        for sentinel in [20, 24, 42] {
            let mut bytes = record();
            bytes[sentinel..sentinel + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            let original = bytes.clone();
            normalize_central_record(&mut bytes).unwrap();
            assert_eq!(bytes, original);
        }
        let mut longer = record();
        longer[30..32].copy_from_slice(&32_u16.to_le_bytes());
        longer[48..50].copy_from_slice(&28_u16.to_le_bytes());
        longer.extend_from_slice(&[0; 4]);
        let original = longer.clone();
        normalize_central_record(&mut longer).unwrap();
        assert_eq!(longer, original);
    }

    #[test]
    fn zip64_normalization_rejects_truncated_records_and_extras() {
        let mut bytes = record();
        bytes.pop();
        assert!(normalize_central_record(&mut bytes).is_err());
        let mut bytes = record();
        bytes[48..50].copy_from_slice(&25_u16.to_le_bytes());
        assert!(normalize_central_record(&mut bytes).is_err());
    }
}
