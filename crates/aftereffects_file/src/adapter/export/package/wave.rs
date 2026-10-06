//! Restrict WAVE_EXTENSIBLE export to PCM24 envelopes that can be represented
//! by the already-supported canonical RIFF/WAVE source profile. Audio bytes are
//! never decoded, resampled, trimmed, or rewritten.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};

use super::{AepConversionError, InterpretError};

const PCM_SUBTYPE: [u8; 16] = [1, 0, 0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113];

/// Returns true only after writing a byte-preserving PCM24 data-chunk copy.
/// Other WAVE encodings use the existing native-source admission path.
pub(super) fn normalize_extensible_pcm24(
    input: &Path,
    output: &Path,
    file_length: u64,
) -> Result<bool, InterpretError> {
    let mut source =
        File::open(input).map_err(|error| io_error("open verified WAVE source", input, error))?;
    let mut header = [0_u8; 60];
    if file_length < header.len() as u64 {
        return Err(InterpretError::Fatal(AepConversionError::Input(
            "RIFF/WAVE extensible header is truncated",
        )));
    }
    source
        .read_exact(&mut header)
        .map_err(|error| io_error("read verified WAVE header", input, error))?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" || &header[12..16] != b"fmt " {
        return Ok(false);
    }
    let declared = u64::from(u32::from_le_bytes(
        header[4..8].try_into().expect("4-byte RIFF field"),
    )) + 8;
    if declared != file_length {
        return Err(InterpretError::Fatal(AepConversionError::Input(
            "RIFF size does not match the archive asset",
        )));
    }
    let fmt_length = u32::from_le_bytes(header[16..20].try_into().expect("4-byte RIFF field"));
    let encoding = u16::from_le_bytes(header[20..22].try_into().expect("2-byte WAVE field"));
    if encoding != 0xfffe {
        return Ok(false);
    }
    if fmt_length != 40 {
        return Err(InterpretError::Unsupported(
            "WAVE_EXTENSIBLE format length is outside the supported PCM24 profile",
        ));
    }
    let channels = u16::from_le_bytes(header[22..24].try_into().expect("2-byte WAVE field"));
    let sample_rate = u32::from_le_bytes(header[24..28].try_into().expect("4-byte WAVE field"));
    let byte_rate = u32::from_le_bytes(header[28..32].try_into().expect("4-byte WAVE field"));
    let align = u16::from_le_bytes(header[32..34].try_into().expect("2-byte WAVE field"));
    let bits = u16::from_le_bytes(header[34..36].try_into().expect("2-byte WAVE field"));
    let cb_size = u16::from_le_bytes(header[36..38].try_into().expect("2-byte WAVE field"));
    let valid_bits = u16::from_le_bytes(header[38..40].try_into().expect("2-byte WAVE field"));
    let mask = u32::from_le_bytes(header[40..44].try_into().expect("4-byte WAVE field"));
    if !matches!((channels, mask), (1, 4) | (2, 3))
        || sample_rate == 0
        || bits != 24
        || valid_bits != 24
        || cb_size != 22
        || header[44..60] != PCM_SUBTYPE
    {
        return Err(InterpretError::Unsupported(
            "WAVE_EXTENSIBLE is not the bounded mono/stereo PCM24 speaker-layout profile",
        ));
    }
    if align != channels * 3 || byte_rate != sample_rate.checked_mul(u32::from(align)).unwrap_or(0)
    {
        return Err(InterpretError::Fatal(AepConversionError::Input(
            "WAVE_EXTENSIBLE sample rate and block metadata disagree",
        )));
    }
    let output_length = file_length - 24;
    let riff_size = u32::try_from(output_length - 8).map_err(|_| {
        InterpretError::Unsupported("normalized RIFF exceeds its native 32-bit length field")
    })?;
    let mut target = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| io_error("create normalized WAVE", output, error))?;
    header[4..8].copy_from_slice(&riff_size.to_le_bytes());
    header[16..20].copy_from_slice(&16_u32.to_le_bytes());
    header[20..22].copy_from_slice(&1_u16.to_le_bytes());
    target
        .write_all(&header[..36])
        .map_err(|error| io_error("write normalized WAVE header", output, error))?;
    let copied = io::copy(&mut source, &mut target)
        .map_err(|error| io_error("copy normalized WAVE payload", output, error))?;
    if copied != file_length - 60 {
        return Err(InterpretError::Fatal(AepConversionError::SourceChanged(
            input.to_owned(),
        )));
    }
    target
        .flush()
        .map_err(|error| io_error("flush normalized WAVE", output, error))?;
    if fs::metadata(output)
        .map_err(|error| io_error("stat normalized WAVE", output, error))?
        .len()
        != output_length
    {
        return Err(InterpretError::Fatal(AepConversionError::Input(
            "normalized WAVE length differs from the verified source",
        )));
    }
    Ok(true)
}

fn io_error(operation: &'static str, path: &Path, error: io::Error) -> InterpretError {
    InterpretError::Fatal(AepConversionError::io(operation, path, error))
}
