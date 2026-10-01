//! Conservative SWF classification, not a Flash renderer or decoder.
//!
//! Only a single opaque video covering the stage, placed once with an identity
//! transform and one sequential video frame per timeline frame is a candidate.
//! All other display-list/script semantics require an independent renderer.

use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

/// Finding embedded video does not prove that extracting it reproduces the SWF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwfClassification {
    /// A decoder still has to prove codec availability and timing preservation.
    EmbeddedVideoCandidate,
    /// Nontrivial Flash rendering semantics cannot be supplied by a video demuxer.
    ExternalRenderRequired { reason: String },
    /// The classifier cannot assess this encoding; this is not an automatic candidate.
    Unassessed { reason: String },
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn u16le(reader: &mut impl Read) -> io::Result<u16> {
    let mut bytes = [0; 2];
    reader.read_exact(&mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}
fn u32le(reader: &mut impl Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
fn byte(reader: &mut impl Read) -> io::Result<u8> {
    let mut bytes = [0];
    reader.read_exact(&mut bytes)?;
    Ok(bytes[0])
}
struct Bits<'a, R> {
    reader: &'a mut R,
    byte: u8,
    left: u8,
}
impl<'a, R: Read> Bits<'a, R> {
    fn new(reader: &'a mut R) -> Self {
        Self {
            reader,
            byte: 0,
            left: 0,
        }
    }
    fn unsigned(&mut self, count: u32) -> io::Result<u32> {
        let mut value = 0;
        for _ in 0..count {
            if self.left == 0 {
                self.byte = byte(self.reader)?;
                self.left = 8;
            }
            self.left -= 1;
            value = (value << 1) | u32::from((self.byte >> self.left) & 1);
        }
        Ok(value)
    }
    fn signed(&mut self, count: u32) -> io::Result<i32> {
        if count == 0 {
            return Ok(0);
        }
        let value = self.unsigned(count)? << (32 - count);
        Ok(i32::from_ne_bytes(value.to_ne_bytes()) >> (32 - count))
    }
}

/// Reads FWS/CWS incrementally with native length bounds, without buffering
/// compressed scripts or imposing a project/media size quota. ZWS is unassessed.
pub fn classify_swf(path: &Path) -> io::Result<SwfClassification> {
    let mut file = File::open(path)?;
    let mut header = [0; 8];
    file.read_exact(&mut header)?;
    let length = u32::from_le_bytes(header[4..8].try_into().map_err(|_| invalid("SWF header"))?);
    let body_length = length
        .checked_sub(8)
        .ok_or_else(|| invalid("invalid SWF length"))?;
    match &header[..3] {
        b"FWS" => classify_body(file, body_length),
        b"CWS" => classify_body(flate2::read::ZlibDecoder::new(file), body_length),
        b"ZWS" => Ok(SwfClassification::Unassessed {
            reason: "LZMA SWF classification is unavailable; do not assume embedded-video extraction preserves its display".into(),
        }),
        _ => Err(invalid("invalid SWF signature")),
    }
}

fn classify_body(reader: impl Read, length: u32) -> io::Result<SwfClassification> {
    let mut body = reader.take(u64::from(length));
    let rect = {
        let mut bits = Bits::new(&mut body);
        let count = bits.unsigned(5)?;
        [
            bits.signed(count)?,
            bits.signed(count)?,
            bits.signed(count)?,
            bits.signed(count)?,
        ]
    };
    let rate = u16le(&mut body)?;
    let timeline_frames = u16le(&mut body)?;
    if rate == 0 || timeline_frames == 0 {
        return Err(invalid("SWF has no timed frames"));
    }
    let mut unsupported = None;
    let mut video = None;
    let mut placed = false;
    let mut video_frames = 0u32;
    let mut shown = 0u32;
    let mut pending_frame = false;
    loop {
        let tag = u16le(&mut body)?;
        let code = tag >> 6;
        let short_length = tag & 63;
        let tag_length = if short_length == 63 {
            u32le(&mut body)?
        } else {
            u32::from(short_length)
        };
        if u64::from(tag_length) > body.limit() {
            return Err(invalid("SWF tag exceeds declared length"));
        }
        let mut payload = body.by_ref().take(u64::from(tag_length));
        match code {
            0 => {
                if tag_length != 0 {
                    return Err(invalid("invalid SWF End tag"));
                }
            }
            1 => {
                if tag_length != 0 {
                    return Err(invalid("invalid SWF ShowFrame tag"));
                }
                if !pending_frame || !placed {
                    unsupported.get_or_insert_with(|| {
                        "timeline holds or shows content other than one placed video frame".into()
                    });
                }
                pending_frame = false;
                shown = shown
                    .checked_add(1)
                    .ok_or_else(|| invalid("SWF frame overflow"))?;
            }
            60 => {
                if tag_length != 10 {
                    return Err(invalid("invalid SWF DefineVideoStream"));
                }
                let id = u16le(&mut payload)?;
                let frames = u16le(&mut payload)?;
                let width = u16le(&mut payload)?;
                let height = u16le(&mut payload)?;
                let flags = byte(&mut payload)?;
                let codec = byte(&mut payload)?;
                if video.is_some()
                    || width == 0
                    || height == 0
                    || frames != timeline_frames
                    || rect != [0, i32::from(width) * 20, 0, i32::from(height) * 20]
                    || flags != 0
                    || !matches!(codec, 2 | 4)
                {
                    unsupported.get_or_insert_with(|| "video is not a single opaque full-stage stream with unmodified display semantics".into());
                }
                video = Some(id);
            }
            61 => {
                let id = u16le(&mut payload)?;
                let frame = u16le(&mut payload)?;
                if video != Some(id) || u32::from(frame) != video_frames || pending_frame {
                    unsupported.get_or_insert_with(|| {
                        "video frame IDs/order differ from the SWF timeline".into()
                    });
                }
                video_frames = video_frames
                    .checked_add(1)
                    .ok_or_else(|| invalid("SWF video frame overflow"))?;
                pending_frame = true;
            }
            26 => {
                let flags = byte(&mut payload)?;
                let depth = u16le(&mut payload)?;
                if flags != 6 || depth != 1 || placed {
                    unsupported.get_or_insert_with(|| {
                        "SWF display-list placement, transforms or actions require rendering".into()
                    });
                } else {
                    let id = u16le(&mut payload)?;
                    let mut bits = Bits::new(&mut payload);
                    let identity = if bits.unsigned(1)? != 0 || bits.unsigned(1)? != 0 {
                        false
                    } else {
                        let count = bits.unsigned(5)?;
                        bits.signed(count)? == 0 && bits.signed(count)? == 0
                    };
                    if video != Some(id) || !identity || payload.limit() != 0 {
                        unsupported.get_or_insert_with(|| {
                            "SWF video has a transform or additional placement state".into()
                        });
                    }
                    placed = true;
                }
            }
            // Background is covered only by the strictly full-stage opaque profile.
            9 if tag_length == 3 => {}
            // Non-rendering metadata can be skipped without buffering it.
            77 => {}
            _ => {
                unsupported.get_or_insert_with(|| format!("SWF tag {code} requires external Flash rendering or unimplemented semantic assessment"));
            }
        }
        io::copy(&mut payload, &mut io::sink())?;
        if payload.limit() != 0 {
            return Err(invalid("truncated SWF tag"));
        }
        if code == 0 {
            break;
        }
    }
    io::copy(&mut body, &mut io::sink())?;
    if body.limit() != 0 {
        return Err(invalid("truncated SWF body"));
    }
    if body.into_inner().read(&mut [0])? != 0 {
        return Err(invalid("SWF exceeds declared length"));
    }
    if video.is_none()
        || !placed
        || video_frames != u32::from(timeline_frames)
        || shown != u32::from(timeline_frames)
        || pending_frame
    {
        unsupported.get_or_insert_with(|| {
            "SWF does not contain one video frame for every displayed timeline frame".into()
        });
    }
    Ok(match unsupported {
        Some(reason) => SwfClassification::ExternalRenderRequired { reason },
        None => SwfClassification::EmbeddedVideoCandidate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tag(out: &mut Vec<u8>, code: u16, payload: &[u8]) {
        out.extend_from_slice(&((code << 6) | u16::try_from(payload.len()).unwrap()).to_le_bytes());
        out.extend_from_slice(payload);
    }
    fn movie() -> Vec<u8> {
        // RECT: five-bit width=6; [0,20,0,20] twips => a 1x1 stage.
        let mut bit_string = String::from("00110");
        for value in [0, 20, 0, 20] {
            bit_string.push_str(&format!("{value:06b}"));
        }
        while bit_string.len() % 8 != 0 {
            bit_string.push('0');
        }
        let mut body: Vec<_> = bit_string
            .as_bytes()
            .chunks(8)
            .map(|bits| u8::from_str_radix(std::str::from_utf8(bits).unwrap(), 2).unwrap())
            .collect();
        body.extend_from_slice(&256u16.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes());
        tag(&mut body, 60, &[1, 0, 1, 0, 1, 0, 1, 0, 0, 2]);
        tag(&mut body, 26, &[6, 1, 0, 1, 0, 0]);
        tag(&mut body, 61, &[1, 0, 0, 0, 1]);
        tag(&mut body, 1, &[]);
        tag(&mut body, 0, &[]);
        body
    }
    fn classify(body: &[u8], compressed: bool) -> io::Result<SwfClassification> {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(if compressed { b"CWS\x08" } else { b"FWS\x08" })
            .unwrap();
        file.write_all(&u32::try_from(body.len() + 8).unwrap().to_le_bytes())
            .unwrap();
        if compressed {
            let mut encoder =
                flate2::write::ZlibEncoder::new(&mut file, flate2::Compression::default());
            encoder.write_all(body).unwrap();
            encoder.finish().unwrap();
        } else {
            file.write_all(body).unwrap();
        }
        classify_swf(file.path())
    }
    #[test]
    fn swf_classification_distinguishes_embedded_video_from_script_rendering() {
        for compressed in [false, true] {
            let mut body = movie();
            assert_eq!(
                classify(&body, compressed).unwrap(),
                SwfClassification::EmbeddedVideoCandidate
            );
            body.truncate(body.len() - 2);
            tag(&mut body, 12, &[0]); // DoAction cannot be supplied by a video decoder.
            tag(&mut body, 0, &[]);
            assert!(matches!(
                classify(&body, compressed).unwrap(),
                SwfClassification::ExternalRenderRequired { .. }
            ));
        }
    }
    #[test]
    fn swf_truncated_tags_are_not_transcode_candidates() {
        let mut body = movie();
        body.pop();
        assert!(classify(&body, false).is_err());
    }
}
