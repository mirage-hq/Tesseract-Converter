//! Fresh Premiere writer for the supported set of video and audio occurrences.
//!
use super::{invalid, Result};
use crate::schema::{
    records::MediaPathField, MediaId, PrColorMatte, PrMedia, PrMediaKind, PrProjectFile,
    PrSequence, TICKS,
};
use flate2::{Compression, GzBuilder};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::OpenOptions,
    io::Write,
    path::Path,
};

mod adjustment;
mod after_effects;
mod color_matte;
mod encode;
mod graph;
mod media;
mod project;
mod sequence;
mod still;
mod time_remap;
mod tracks;

fn valid_xml_text(value: &str) -> bool {
    value.chars().all(|c| matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}'))
}

// Writer-local borrowed media. Only validation constructs these values, so graph
// encoding does not need optional fields or panicking path accessors.
enum BoundMedia<'a> {
    File {
        media: &'a PrMedia,
        relative_path: &'a str,
        absolute_path: &'a str,
    },
    /// Generator media: no file, one colour.
    ColorMatte {
        media: &'a PrMedia,
        matte: PrColorMatte,
    },
    /// The Black Video generator media of an adjustment layer: no file.
    Adjustment { media: &'a PrMedia },
}

impl<'a> BoundMedia<'a> {
    fn media(&self) -> &'a PrMedia {
        match self {
            Self::File { media, .. }
            | Self::ColorMatte { media, .. }
            | Self::Adjustment { media } => media,
        }
    }
}

fn valid_media_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.contains(['/', '\\'])
        && valid_xml_text(name)
        && Path::new(name).file_name().and_then(|x| x.to_str()) == Some(name)
}

// Generated hybrid packages give each AEP its own dependency directory. Keep
// this exception narrower than ordinary media paths: the fixed ASCII layout
// has no traversal, device names, case aliases or ambiguous separators.
fn scoped_after_effects_path(path: &str, name: &str) -> bool {
    if name != "compositions.aep" {
        return false;
    }
    path.strip_prefix("./media/ae-")
        .and_then(|path| path.strip_suffix("/compositions.aep"))
        .is_some_and(|index| {
            index.len() == 4 && index.bytes().all(|byte| byte.is_ascii_digit()) && index != "0000"
        })
}

fn validate_project(
    project: &PrProjectFile,
) -> Result<(&PrSequence, BTreeMap<&MediaId, BoundMedia<'_>>)> {
    let spec = project
        .single_sequence()
        .ok_or_else(|| invalid("writer requires exactly one sequence"))?;
    if spec.name.is_empty() || spec.name.chars().count() > 255 || !valid_xml_text(&spec.name) {
        return Err(invalid(
            "sequence name must contain 1 to 255 valid XML characters",
        ));
    }
    tracks::nested::validate_sequences(spec)?;
    project.validate()?;
    fn check_stroke(sequence: &PrSequence) -> Result<()> {
        for track in &sequence.video_tracks {
            if track
                .items
                .iter()
                .filter_map(crate::PrVideoItem::media)
                .any(|clip| clip.stroke.is_some())
            {
                return Err(invalid("measured Film Impact Stroke is import-only"));
            }
            for nest in &track.nests {
                check_stroke(&nest.sequence)?;
            }
        }
        Ok(())
    }
    check_stroke(spec)?;
    let mut bound = BTreeMap::new();
    let mut relative_paths = BTreeSet::new();
    for id in spec.media_in_order() {
        let media = &project.media[id];
        if let Some(video) = &media.video {
            let generator = match video.kind {
                PrMediaKind::NumberedStills { .. } => {
                    return Err(invalid(
                        "numbered stills must export as editable individual still placements",
                    ))
                }
                PrMediaKind::ColorMatte(matte) => Some(BoundMedia::ColorMatte { media, matte }),
                PrMediaKind::Adjustment => Some(BoundMedia::Adjustment { media }),
                PrMediaKind::Video { .. }
                | PrMediaKind::Still { .. }
                | PrMediaKind::AfterEffectsComposition(_) => None,
            };
            if let Some(generator) = generator {
                if !valid_media_name(&media.name) {
                    return Err(invalid(
                        "media name must be one safe path component of at most 255 bytes",
                    ));
                }
                if (video.width, video.height) != (spec.width, spec.height) {
                    return Err(invalid("media dimensions must match sequence dimensions"));
                }
                bound.insert(id, generator);
                continue;
            }
        }
        let relative_path = media
            .relative_path
            .as_deref()
            .ok_or_else(|| invalid("writer requires one preferred relative media path"))?;
        let absolute_path = media
            .absolute_paths
            .iter()
            .find(|(field, _)| *field == MediaPathField::FilePath)
            .or_else(|| media.absolute_paths.first())
            .map(|(_, path)| Path::new(path))
            .ok_or_else(|| invalid("writer requires one absolute media path"))?;
        if !valid_media_name(&media.name) {
            return Err(invalid(
                "media name must be one safe path component of at most 255 bytes",
            ));
        }
        let is_after_effects = media.after_effects_composition().is_some();
        if is_after_effects {
            if media.audio.is_some()
                || ![Path::new(&media.name), absolute_path]
                    .into_iter()
                    .all(|path| {
                        path.extension()
                            .and_then(|value| value.to_str())
                            .is_some_and(|value| value.eq_ignore_ascii_case("aep"))
                    })
            {
                return Err(invalid(
                    "linked After Effects media requires an AEP file and no audio stream",
                ));
            }
        } else if crate::media::admitted_container(media, Path::new(&media.name)).is_none() {
            return Err(invalid(if media.is_still() {
                "writer supports PNG/JPEG still media only"
            } else {
                "writer supports MP4/MOV video and WAV/MP3/M4A audio"
            }));
        }
        if media.video.as_ref().is_some_and(|video| {
            matches!(
                video.kind,
                crate::schema::PrMediaKind::Video { codec: None, .. }
            )
        }) {
            return Err(invalid("writer requires the inspected video codec"));
        }
        if !absolute_path.is_absolute() {
            return Err(invalid("media absolute path must be absolute"));
        }
        let Some(absolute_path) = absolute_path
            .to_str()
            .filter(|path| path.len() <= 4096 && valid_xml_text(path))
        else {
            return Err(invalid(
                "media path must be UTF-8 with valid XML characters and at most 4096 bytes",
            ));
        };
        if relative_path != format!("./media/{}", media.name)
            && !(is_after_effects && scoped_after_effects_path(relative_path, &media.name))
        {
            return Err(invalid(if is_after_effects {
                "linked AEP relative path must be ./media/<the exact media name> or ./media/ae-NNNN/compositions.aep (0001–9999)"
            } else {
                "media relative path must be ./media/<the exact media name>"
            }));
        }
        if !relative_paths.insert(relative_path) {
            return Err(invalid(
                "media relative path must be unique across source identities",
            ));
        }
        if media
            .video
            .as_ref()
            .is_some_and(|video| video.width == 0 || video.height == 0)
        {
            return Err(invalid("media dimensions must be positive"));
        }
        if media.audio.as_ref().is_some_and(|audio| {
            audio.sample_rate == 0 || TICKS % i64::from(audio.sample_rate) != 0
        }) {
            return Err(invalid("audio sample rate has no exact tick period"));
        }
        bound.insert(
            id,
            BoundMedia::File {
                media,
                relative_path,
                absolute_path,
            },
        );
    }
    Ok((spec, bound))
}

/// Build decompressed Premiere XML directly from current video occurrences.
pub(super) fn project_xml(project: &PrProjectFile) -> Result<String> {
    let (sequence, media) = validate_project(project)?;
    encode::encode(&graph::build(sequence, &media)?)
}

/// Temporary validated Premiere XML used by check or immediate writing.
#[derive(Debug)]
pub(crate) struct PremiereProjectXml {
    xml: String,
}

impl PremiereProjectXml {
    /// Encode and validate a Premiere model without touching disk.
    pub(crate) fn new(project: &PrProjectFile) -> Result<Self> {
        Ok(Self {
            xml: project_xml(project)?,
        })
    }

    /// Write exactly the prepared graph to a new gzip-framed project.
    pub(crate) fn write_new(&self, path: &Path) -> Result<[u8; 32]> {
        if path.extension().and_then(|x| x.to_str()) != Some("prproj") {
            return Err(invalid("output project must end in .prproj"));
        }
        let output = OpenOptions::new().write(true).create_new(true).open(path)?;
        let mut encoder = GzBuilder::new()
            .mtime(0)
            .operating_system(10)
            .write(crate::hash::DigestWriter::new(output), Compression::new(6));
        encoder.write_all(self.xml.as_bytes())?;
        let (output, digest) = encoder.finish()?.into_parts();
        output.sync_all()?;
        Ok(digest)
    }
}
