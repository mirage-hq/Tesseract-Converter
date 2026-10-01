//! Validate one sequence and its media before writing a Tesseract archive.
//! Preparation reads source files; writing rechecks their hashes before publication.
use crate::{
    approximate,
    audio_media::inspect_audio_media,
    error::{ensure, unsupported, BuildError, Result},
    format::{MediaId, PrMedia, PrSequence, PrVideoItem},
    hash::hash,
    linked_compositions::LinkedCompositions,
    media::{admitted_container, unsupported_media_reason, MediaContainer, MediaFacts},
    media_metadata::ColourDescription,
    omit,
    schema::{after_effects::LINKED_AUDIO_REASON, PrMediaKind},
    Omission, OmissionScope,
};
use aftereffects_file::LinkedMedia;
use fx_conv::{
    InspectedMedia as PreflightMedia, MediaKind, MediaPreflight, MediaRemediation, MediaStatus,
    ValidatedMediaMap,
};
use fx_schema::AssetId;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    sync::Arc,
};
use tesseract_file::TesseractFileBuilder;

// A cache lives for one verification pass only. Later passes re-read bytes,
// while repeated occurrences and aliases share a digest within the same pass.
fn media_hash(path: &Path, digests: &mut HashMap<PathBuf, String>) -> Result<String> {
    if let Some(digest) = digests.get(path) {
        return Ok(digest.clone());
    }
    let digest = hash(path)?;
    digests.insert(path.to_owned(), digest.clone());
    Ok(digest)
}

/// Resolve one media record to the source file that Premiere links.
///
/// Premiere links media through its current absolute path first. It uses
/// paths relative to the project only when that path is missing. After a
/// package moves, only its package-local RelativePath stays live. Adobe Save As
/// keeps earlier relative hints beside its updated paths. Thus a candidate can
/// be missing, but all live candidates must identify the same bytes. The
/// resolved path is the package-local candidate, else the first live relative
/// candidate, else the first live absolute alias.
///
/// A live package-local RelativePath identifies the media by itself: other
/// relative hints that are missing are stale history, such as a `../` hint to
/// the folder the media was imported from, and are ignored. Otherwise a
/// missing hint needs a live absolute alias with the same bytes, or the media
/// is missing and its placements are omitted; without that alias, a record
/// whose relative hints are all live but outside the package is rejected.
fn resolve_media(
    root: &Path,
    clip: &PrMedia,
    digests: &mut HashMap<PathBuf, String>,
) -> Result<(PathBuf, String, Vec<PathBuf>)> {
    let mut package_local = None;
    let mut expected_hash: Option<(&str, String)> = None;
    let mut resolved_candidates = Vec::with_capacity(clip.relative_paths.len());
    let mut missing_relative = Vec::new();
    for candidate in &clip.relative_paths {
        let relative = Path::new(candidate);
        ensure!(
            !relative.is_absolute(),
            "absolute native media candidate is unsupported: {candidate:?}"
        );
        let canonical = match root.join(relative).canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing_relative.push(candidate);
                continue;
            }
            Err(error) => {
                return Err(BuildError::IoAt {
                    context: format!("cannot resolve native media candidate {candidate:?}"),
                    source: error,
                });
            }
        };
        let candidate_hash = media_hash(&canonical, digests)?;
        if let Some((first, hash)) = &expected_hash {
            ensure!(
                *hash == candidate_hash,
                "native media candidates identify different bytes: {first:?} and {candidate:?}"
            );
        } else {
            expected_hash = Some((candidate, candidate_hash.clone()));
        }
        if clip.relative_path.as_ref() == Some(candidate) {
            ensure!(
                canonical.starts_with(root),
                "package-local media candidate escapes the source package"
            );
            package_local = Some(canonical.clone());
        }
        resolved_candidates.push(canonical);
    }
    let mut absolute_identity_verified = false;
    for (field, candidate) in &clip.absolute_paths {
        let field = field.tag();
        let canonical = match Path::new(candidate).canonicalize() {
            Ok(path) => path,
            // Absolute locations become stale when a self-contained package moves.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(BuildError::IoAt {
                    context: format!("cannot resolve native {field} alias {candidate:?}"),
                    source: error,
                });
            }
        };
        let candidate_hash = media_hash(&canonical, digests)?;
        // Save As outside the package can make every relative hint stale. Then
        // the first live alias sets the expected bytes, because Premiere links it.
        let (_, expected) = expected_hash.get_or_insert_with(|| (field, candidate_hash.clone()));
        ensure!(
            *expected == candidate_hash,
            "native media candidates identify different bytes: {field} {candidate:?}"
        );
        absolute_identity_verified = true;
        resolved_candidates.push(canonical);
    }
    let package_local_live = package_local.is_some();
    let (Some(media), Some((_, hash))) = (
        package_local.or_else(|| resolved_candidates.first().cloned()),
        expected_hash,
    ) else {
        return Err(BuildError::MissingMedia(
            "native media candidates are missing; identity cannot be verified".into(),
        ));
    };
    // verify_media repeats these checks before publication.
    if !package_local_live && !absolute_identity_verified {
        if !missing_relative.is_empty() {
            return Err(BuildError::MissingMedia(format!(
                "native media candidate is missing; identity cannot be verified without a matching absolute alias: {missing_relative:?}"
            )));
        }
        return Err(unsupported(
            "native media candidate is external-only; identity cannot be verified without a matching absolute alias",
        ));
    }
    Ok((media, hash, resolved_candidates))
}

pub(crate) fn asset_ids_in_order(
    sequence: &PrSequence,
    media: &BTreeMap<MediaId, PrMedia>,
) -> BTreeMap<MediaId, AssetId> {
    sequence
        .media_in_order()
        .into_iter()
        // Generator media (Color Mattes) become layers without an asset, and
        // a linked composition becomes editable layers with assets of its own.
        .filter(|id| {
            !media.get(*id).is_some_and(|media| {
                media.is_generator() || media.after_effects_composition().is_some()
            })
        })
        .enumerate()
        .map(|(index, id)| {
            let kind = if media.get(id).is_some_and(PrMedia::is_still) {
                "image"
            } else {
                "video"
            };
            (
                id.clone(),
                AssetId::from_trusted(format!("premiere-{kind}-{}", index + 1)),
            )
        })
        .collect()
}

/// One validated sequence with resolved media paths, asset IDs, and source hashes.
#[derive(Debug)]
pub(crate) struct PendingTesseractFile {
    source: PathBuf,
    project_media: Arc<BTreeMap<MediaId, PrMedia>>,
    media: BTreeMap<MediaId, ResolvedMedia>,
    /// Media of the linked compositions' pictures, packaged by `builder`; it
    /// backs that archive's asset files until this value is dropped, and its
    /// local sources are checked with the other media.
    linked_media: LinkedMedia,
    builder: Option<TesseractFileBuilder>,
}

#[derive(Debug)]
struct ResolvedMedia {
    path: PathBuf,
    hash: String,
    native_path: PathBuf,
    native_hash: String,
    candidate_paths: Vec<PathBuf>,
    /// The archive asset of this file and its admitted container. A linked
    /// AEP is no asset: its pictures package their own media.
    asset: Option<(AssetId, MediaContainer)>,
}

/// The resolved source file of one media record, shared by all of its placements.
struct InspectedMedia {
    path: PathBuf,
    hash: String,
    native_path: PathBuf,
    native_hash: String,
    candidate_paths: Vec<PathBuf>,
    /// The admitted container, which decides the packaged asset kind.
    container: MediaContainer,
    /// A still feature reported per placement, or a video approximation
    /// reported once when this media is retained for packaging.
    note: Option<MediaNote>,
    /// Video colour that passes through, reported once for a packaged file.
    colour: Option<ColourDescription>,
    /// Admitted video codec family, when this source has a picture stream.
    codec: Option<String>,
}

#[derive(Clone, Copy)]
enum MediaNote {
    Feature(&'static str),
    Approximation(&'static str),
}

enum MediaInspection {
    Ready(InspectedMedia),
    /// Generator media with no file to inspect or package.
    Synthetic,
    /// A composition in a resolved AEP, which each video placement imports
    /// as editable content ([`LinkedCompositions`]).
    Linked {
        path: PathBuf,
        hash: String,
        candidate_paths: Vec<PathBuf>,
    },
    Omitted(String),
}

/// Prove source intervals after clipping every containing unit nest. Hidden
/// placements stay included: toggling visibility later must retain valid media.
fn selected_video_use(sequence: &PrSequence, id: &MediaId) -> Option<crate::media::VideoUse> {
    use std::ops::Range;
    fn collect(
        sequence: &PrSequence,
        id: &MediaId,
        visible: Range<i64>,
        ranges: &mut Vec<Range<i64>>,
    ) -> Option<()> {
        for clip in sequence
            .video_occurrences()
            .filter(|clip| &clip.media == id)
        {
            let start = clip.start_ticks.max(visible.start);
            let end = clip.end_ticks.min(visible.end);
            if start >= end {
                continue;
            }
            if clip.playback_rate != 1.0
                || clip.time_remap.is_some()
                || clip.out_ticks.checked_sub(clip.in_ticks)?
                    != clip.end_ticks.checked_sub(clip.start_ticks)?
            {
                return None;
            }
            let source_start = clip
                .in_ticks
                .checked_add(start.checked_sub(clip.start_ticks)?)?;
            let source_end = clip
                .in_ticks
                .checked_add(end.checked_sub(clip.start_ticks)?)?;
            ranges.push(source_start..source_end);
        }
        for nest in sequence.nest_occurrences() {
            let start = nest.start_ticks.max(visible.start);
            let end = nest.end_ticks.min(visible.end);
            if start >= end {
                continue;
            }
            if nest.out_ticks.checked_sub(nest.in_ticks)?
                != nest.end_ticks.checked_sub(nest.start_ticks)?
            {
                return None;
            }
            let inner_start = nest
                .in_ticks
                .checked_add(start.checked_sub(nest.start_ticks)?)?;
            let inner_end = nest
                .in_ticks
                .checked_add(end.checked_sub(nest.start_ticks)?)?;
            collect(&nest.sequence, id, inner_start..inner_end, ranges)?;
        }
        Some(())
    }
    let mut ranges = Vec::new();
    collect(sequence, id, 0..sequence.end_ticks(), &mut ranges)?;
    if ranges.is_empty() {
        return None;
    }
    Some(crate::media::VideoUse {
        ranges,
        uses_audio: sequence.uses_audio_media(id),
    })
}

/// Resolve and inspect one media record `id`. Unsupported video file formats
/// stop conversion; missing files and unsupported still/audio content can omit placements.
fn inspect_media(
    root: &Path,
    id: &MediaId,
    media: &PrMedia,
    sequence: Option<&PrSequence>,
    digests: &mut HashMap<PathBuf, String>,
    linked: &mut LinkedCompositions<'_, '_>,
    media_map: Option<&ValidatedMediaMap>,
) -> Result<MediaInspection> {
    match media.video.as_ref().map(|video| video.kind) {
        Some(PrMediaKind::ColorMatte(_) | PrMediaKind::Adjustment) => {
            return Ok(MediaInspection::Synthetic);
        }
        Some(PrMediaKind::AfterEffectsComposition(_)) => {
            // The AEP resolves as any package file does; it is never decoded.
            let (path, hash, candidate_paths) = match resolve_media(root, media, digests) {
                Ok(resolved) => resolved,
                Err(error @ BuildError::MissingMedia(_)) => {
                    return Ok(MediaInspection::Omitted(format!(
                        "linked After Effects project: {error}"
                    )));
                }
                Err(error) => return Err(error),
            };
            return Ok(match linked.link(id, media, &path, &hash)? {
                Ok(()) => MediaInspection::Linked {
                    path,
                    hash,
                    candidate_paths,
                },
                Err(reason) => MediaInspection::Omitted(reason),
            });
        }
        Some(PrMediaKind::Video { .. } | PrMediaKind::Still { .. }) | None => {}
    }
    let (native_path, native_hash, candidate_paths) = match resolve_media(root, media, digests) {
        Ok(resolved) => resolved,
        Err(error @ BuildError::MissingMedia(_)) => {
            return Ok(MediaInspection::Omitted(error.to_string()));
        }
        Err(error) => return Err(error),
    };
    let path = media_map
        .map(|map| map.replacement_for(&native_path))
        .transpose()?
        .flatten()
        .unwrap_or(&native_path)
        .to_owned();
    let hash = media_hash(&path, digests)?;
    let extension = path.extension().and_then(|x| x.to_str());
    let is_video = media
        .video
        .as_ref()
        .is_some_and(|video| matches!(video.kind, PrMediaKind::Video { .. }));
    if is_video {
        crate::video_format::validate_video_file_name(&path)?;
    }
    let Some(container) = admitted_container(media, &path) else {
        let error = unsupported(if media.is_still() {
            "still media must be a PNG or JPEG file"
        } else {
            "supported media: MP4/MOV video and WAV/MP3/M4A audio"
        });
        return if is_video {
            Err(error)
        } else {
            Ok(MediaInspection::Omitted(error.to_string()))
        };
    };
    // The archive stores the source file name. Reject unusable video names;
    // still and audio media retain their existing omission policy.
    if let Err(error) = tesseract_file::validate_asset_source_name(&path) {
        return if is_video {
            Err(error.into())
        } else {
            Ok(MediaInspection::Omitted(error.to_string()))
        };
    }
    let usage = sequence.and_then(|sequence| selected_video_use(sequence, id));
    // A media keeps one note: the first one below that applies.
    let mut note = None;
    let mut colour = None;
    let mut codec = None;
    let mut picture = None;
    if let Some(native) = &media.video {
        let input = File::open(&path)?;
        let size = input.metadata()?.len();
        let facts = match crate::media::inspect_media(
            native.kind,
            BufReader::new(input),
            File::open(&path)?,
            size,
            usage.as_ref(),
        ) {
            Ok(facts) => facts,
            Err(error) if is_video => return Err(error),
            Err(error) => return unsupported_media_reason(error).map(MediaInspection::Omitted),
        };
        let (width, height) = facts.dimensions();
        if (width, height) != (native.width, native.height) {
            let error = unsupported(format!(
                "source dimensions {width}x{height} differ from native dimensions {}x{}",
                native.width, native.height
            ));
            return if is_video {
                Err(error)
            } else {
                Ok(MediaInspection::Omitted(error.to_string()))
            };
        }
        // Video admission is strict. A still with incompatible source facts
        // retains the existing safe-omission behavior.
        picture = crate::audio_media::PictureClock::of(&facts);
        let selected_only = match &facts {
            MediaFacts::Video(video) => video.validate_source_for_use(native, usage.as_ref()),
            MediaFacts::Still(_) => facts.validate_source(native).map(|()| false),
        }?;
        if selected_only {
            note.get_or_insert(MediaNote::Approximation(
                "original sample timestamps and first affine MP4 edit retained for proved selected unit source intervals; whole-source endpoint and later edit playback are unsupported",
            ));
        }
        if let (MediaFacts::Still(image), PrMediaKind::Still { alpha }) = (&facts, native.kind) {
            if let Some(reason) = image.declaration_mismatch(extension, alpha) {
                return Ok(MediaInspection::Omitted(unsupported(reason).to_string()));
            }
            if let Some(colour) = image.colour_note() {
                note.get_or_insert(MediaNote::Feature(colour));
            }
        }
        if let MediaFacts::Video(video) = &facts {
            colour = video.colour;
            if video.timing.legacy_signed_ctts {
                note.get_or_insert(MediaNote::Approximation(
                    "legacy CTTSv0 signed offsets retained exactly as the shared parser/player evaluates them after physical PTS/count/origin checks; admission beyond established exact CFR requires selected unit interior intervals; native intermediate frame selection remains unverified",
                ));
            }
            codec = Some(
                match video.codec {
                    crate::schema::VideoCodec::H264 => "h264",
                    crate::schema::VideoCodec::HevcMain => "hevc",
                }
                .to_owned(),
            );
            if matches!(
                video.timing.clock,
                crate::media::SampleClock::Irregular { .. }
            ) {
                if usage.is_none() {
                    return Ok(MediaInspection::Omitted(
                        "irregular source requires unit forward playback without Time Remapping, proved through every containing nest".into(),
                    ));
                }
                note.get_or_insert(MediaNote::Approximation(
                    "irregular source presentation timestamps retained with original bytes and unit playback after native sample-count and rounded endpoint checks; intermediate native frame-clock interpretation remains unverified",
                ));
            } else if matches!(
                video.timing.clock,
                crate::media::SampleClock::Quantized { .. }
            ) || native.frame_rate.supported().is_none()
            {
                note.get_or_insert(MediaNote::Approximation(
                    "source sample clock admitted by matching sample count and rounded duration; intermediate source frame selection may differ from Premiere",
                ));
            }
        }
    }
    // Pictures import muted. An embedded stream that this timeline never
    // places as sound must not determine whether its picture can import.
    if let Some(native) = media
        .audio
        .as_ref()
        .filter(|_| sequence.is_none_or(|sequence| sequence.uses_audio_media(id)))
    {
        let input = File::open(&path)?;
        let size = input.metadata()?.len();
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let facts = match inspect_audio_media(BufReader::new(input), size, extension) {
            Ok(Some(facts)) => facts,
            Ok(None) => {
                return Ok(MediaInspection::Omitted(
                    "native audio stream is missing from the file".into(),
                ));
            }
            Err(error) => return unsupported_media_reason(error).map(MediaInspection::Omitted),
        };
        // An audio-only native record of a movie still has the picture
        // Premiere measures; a file without a supported picture has none.
        let picture = picture.or_else(|| {
            let video = crate::media::inspect_video_media(
                File::open(&path).ok()?,
                File::open(&path).ok()?,
                size,
            )
            .ok()?;
            crate::audio_media::PictureClock::of(&MediaFacts::Video(video))
        });
        match crate::audio_media::validate_source(&facts, native, picture) {
            Ok(crate::audio_media::AudioDurationMatch::Exact) => {}
            Ok(crate::audio_media::AudioDurationMatch::PaddedToPicture) => {
                note.get_or_insert(MediaNote::Approximation(
                    "embedded AAC ends up to one frame before the picture; Premiere's picture-length audio duration was admitted and the tail plays as silence",
                ));
            }
            Err(error) => return Ok(MediaInspection::Omitted(error.to_string())),
        }
    }
    Ok(MediaInspection::Ready(InspectedMedia {
        path,
        hash,
        native_path,
        native_hash,
        candidate_paths,
        container,
        note,
        colour,
        codec,
    }))
}

fn preflight_failure(error: &BuildError) -> (MediaStatus, Option<String>) {
    let status = match error {
        BuildError::MissingMedia(_) => MediaStatus::Missing,
        BuildError::Io(_) | BuildError::IoAt { .. } => MediaStatus::Unreadable,
        BuildError::Mp4(_) | BuildError::Audio(_) => MediaStatus::InvalidMedia,
        BuildError::Unsupported(_) => MediaStatus::RequiresTranscode,
        _ => MediaStatus::Unassessed,
    };
    (status, Some(error.to_string()))
}

/// Inspect one loaded Premiere sequence without creating output files.
pub(crate) fn inspect_premiere_sequence(
    source: &Path,
    parsed: &PrSequence,
    project_media: &BTreeMap<MediaId, PrMedia>,
    media_map: Option<&ValidatedMediaMap>,
) -> Result<MediaPreflight> {
    inspect_media_references(
        source,
        parsed.id.clone().unwrap_or_default(),
        project_media,
        parsed.media_in_order(),
        Some(parsed),
        media_map,
    )
}

pub(crate) fn inspect_native_premiere_media(
    source: &Path,
    target: &str,
    media_map: Option<&ValidatedMediaMap>,
) -> Result<MediaPreflight> {
    // The model supplies clocks only when the native inventory proves that
    // every physical placement survives. Failed loads retain strict inspection.
    let parsed = crate::format::PrProjectFile::load_import(source, Some(target))
        .ok()
        .map(|(project, _)| project);
    let sequence = parsed
        .as_ref()
        .and_then(|project| project.single_sequence());
    inspect_native_media(source, target, sequence, media_map)
}

/// Native inventory with proved complete selected picture/sound placement context.
pub(crate) fn inspect_native_premiere_media_for_sequence(
    source: &Path,
    parsed: &PrSequence,
    media_map: Option<&ValidatedMediaMap>,
) -> Result<MediaPreflight> {
    inspect_native_media(
        source,
        parsed.id.as_deref().unwrap_or_default(),
        Some(parsed),
        media_map,
    )
}

fn inspect_native_media(
    source: &Path,
    target: &str,
    sequence: Option<&PrSequence>,
    media_map: Option<&ValidatedMediaMap>,
) -> Result<MediaPreflight> {
    let native = crate::format::PrProjectFile::native_media_scope(source, target)?;
    let sequence = sequence.filter(|sequence| native.covers(sequence));
    let mut report = inspect_media_references(
        source,
        target.to_owned(),
        &native.media,
        native.media.keys().collect(),
        sequence,
        media_map,
    )?;
    report.unassessed.extend(native.unassessed);
    Ok(report)
}

pub(crate) fn require_video_admission(preflight: &MediaPreflight) -> Result<()> {
    if let Some(blocked) = preflight
        .media
        .iter()
        .find(|media| media.kind == MediaKind::Video && media.status != MediaStatus::Supported)
    {
        return Err(unsupported(format!(
            "video media {} ({:?}) failed admission: {}",
            blocked.id,
            blocked.name,
            blocked.reason.as_deref().unwrap_or("unassessed media")
        )));
    }
    Ok(())
}

fn inspect_media_references(
    source: &Path,
    target: String,
    project_media: &BTreeMap<MediaId, PrMedia>,
    ids: Vec<&MediaId>,
    sequence: Option<&PrSequence>,
    media_map: Option<&ValidatedMediaMap>,
) -> Result<MediaPreflight> {
    let root = source
        .parent()
        .ok_or_else(|| unsupported("project parent missing"))?;
    let owner = source.to_owned();
    let mut digests = HashMap::new();
    let mut linked = LinkedCompositions::new(None, media_map);
    let mut report = MediaPreflight {
        format: "premiere".to_owned(),
        target: target.clone(),
        media: Vec::new(),
        unassessed: Vec::new(),
    };

    for id in ids {
        let media = &project_media[id];
        if media.is_generator()
            || media
                .video
                .as_ref()
                .is_some_and(|video| matches!(video.kind, PrMediaKind::Still { .. }))
        {
            continue;
        }
        if media.after_effects_composition().is_some() {
            match inspect_media(
                root,
                id,
                media,
                sequence,
                &mut digests,
                &mut linked,
                media_map,
            ) {
                Ok(MediaInspection::Linked { .. }) => match linked.inspect_media(id, media_map) {
                    Ok(mut linked_report) => {
                        for inspected in &mut linked_report.media {
                            inspected.references.push(format!("premiere:{target}:{id}"));
                        }
                        report.media.append(&mut linked_report.media);
                        report.unassessed.extend(
                            linked_report
                                .unassessed
                                .into_iter()
                                .map(|reason| format!("{id}: {reason}")),
                        );
                    }
                    Err(error) => report.unassessed.push(format!("{id}: {error}")),
                },
                Ok(MediaInspection::Omitted(reason)) => {
                    report.unassessed.push(format!("{id}: {reason}"));
                }
                Ok(_) => report
                    .unassessed
                    .push(format!("{id}: linked composition was not resolved")),
                Err(error) => report.unassessed.push(format!("{id}: {error}")),
            }
            continue;
        }

        let authored = media
            .relative_path
            .as_ref()
            .map(PathBuf::from)
            .or_else(|| media.absolute_paths.first().map(|(_, path)| path.clone()));
        let resolved = resolve_media(root, media, &mut digests);
        let (original, selected) = match &resolved {
            Ok((original, _, _)) => {
                let selected = media_map
                    .map(|map| map.replacement_for(original))
                    .transpose()?
                    .flatten()
                    .unwrap_or(original)
                    .to_owned();
                (Some(original.clone()), Some(selected))
            }
            Err(_) => (None, None),
        };
        let kind = if media
            .video
            .as_ref()
            .is_some_and(|video| matches!(video.kind, PrMediaKind::Video { .. }))
        {
            MediaKind::Video
        } else {
            MediaKind::Audio
        };
        let inspected = inspect_media(
            root,
            id,
            media,
            sequence,
            &mut digests,
            &mut linked,
            media_map,
        );
        let mut codec = None;
        let (mut status, mut reason) = match inspected {
            Ok(MediaInspection::Ready(inspected)) => {
                codec = inspected.codec;
                (MediaStatus::Supported, None)
            }
            Ok(MediaInspection::Omitted(reason)) => (MediaStatus::RequiresTranscode, Some(reason)),
            Ok(_) => (
                MediaStatus::Unassessed,
                Some("native media was not inspected as a file".to_owned()),
            ),
            Err(error) => preflight_failure(&error),
        };
        if let Err(error) = &resolved {
            (status, reason) = preflight_failure(error);
        }
        let is_swf = selected
            .as_deref()
            .or(original.as_deref())
            .and_then(Path::extension)
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("swf"));
        let swf_remediation = if is_swf {
            let path = selected.as_deref().or(original.as_deref());
            match path.map(fx_conv::classify_swf).transpose() {
                Ok(Some(fx_conv::SwfClassification::EmbeddedVideoCandidate)) => {
                    status = MediaStatus::RequiresTranscode;
                    Some(MediaRemediation::TranscodeCandidate)
                }
                Ok(Some(fx_conv::SwfClassification::ExternalRenderRequired {
                    reason: swf_reason,
                })) => {
                    status = MediaStatus::RequiresTranscode;
                    reason = Some(swf_reason);
                    Some(MediaRemediation::ExternalRenderRequired)
                }
                Ok(Some(fx_conv::SwfClassification::Unassessed { reason: swf_reason })) => {
                    status = MediaStatus::Unassessed;
                    reason = Some(swf_reason);
                    Some(MediaRemediation::Unknown)
                }
                Ok(None) => Some(MediaRemediation::Unknown),
                Err(error) => {
                    status = if matches!(
                        error.kind(),
                        std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof
                    ) {
                        MediaStatus::InvalidMedia
                    } else {
                        MediaStatus::Unreadable
                    };
                    reason = Some(error.to_string());
                    Some(MediaRemediation::Unknown)
                }
            }
        } else {
            None
        };
        let remediation = swf_remediation.unwrap_or(match status {
            MediaStatus::Supported => MediaRemediation::None,
            MediaStatus::RequiresTranscode => MediaRemediation::TranscodeCandidate,
            _ => MediaRemediation::Unknown,
        });
        let container = selected
            .as_deref()
            .or(original.as_deref())
            .and_then(Path::extension)
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        report.media.push(PreflightMedia {
            owner: owner.clone(),
            id: id.to_string(),
            name: media.name.clone(),
            kind,
            authored,
            original,
            selected,
            references: vec![target.clone()],
            status,
            container,
            codec,
            reason,
            remediation,
        });
    }
    Ok(report)
}

/// Convert one loaded Premiere sequence without creating output files.
///
/// Returns `None`, and records the omission, when no occurrence can convert.
/// Every other failure is an error for the whole batch.
#[cfg(test)]
pub(crate) fn convert_premiere_sequence(
    source: &Path,
    parsed: PrSequence,
    project_media: Arc<BTreeMap<MediaId, PrMedia>>,
    omissions: &mut Vec<Omission>,
) -> Result<Option<PendingTesseractFile>> {
    convert_premiere_sequence_with_links(
        source,
        parsed,
        project_media,
        omissions,
        None,
        fx_conv::Progress::default(),
    )
}

/// [`convert_premiere_sequence`] with source-bound prepared media substitutions.
pub(crate) fn convert_premiere_sequence_with_media_map(
    source: &Path,
    parsed: PrSequence,
    project_media: Arc<BTreeMap<MediaId, PrMedia>>,
    omissions: &mut Vec<Omission>,
    media_map: &ValidatedMediaMap,
    progress: fx_conv::Progress<'_>,
) -> Result<Option<PendingTesseractFile>> {
    convert_premiere_sequence_with_options(
        source,
        parsed,
        project_media,
        omissions,
        None,
        Some(media_map),
        progress,
    )
}

/// [`convert_premiere_sequence`], with the pictures of its linked
/// compositions imported by `resolver` instead of the built-in import. The
/// resolver's assets are packaged as it returns them: their lifetime and
/// bytes are its caller's to keep until the archive is written.
pub(crate) fn convert_premiere_sequence_with_links<'a>(
    source: &Path,
    parsed: PrSequence,
    project_media: Arc<BTreeMap<MediaId, PrMedia>>,
    omissions: &mut Vec<Omission>,
    resolver: Option<&'a mut crate::LinkedCompositionResolver<'a>>,
    progress: fx_conv::Progress<'_>,
) -> Result<Option<PendingTesseractFile>> {
    convert_premiere_sequence_with_options(
        source,
        parsed,
        project_media,
        omissions,
        resolver,
        None,
        progress,
    )
}

fn convert_premiere_sequence_with_options<'a>(
    source: &Path,
    mut parsed: PrSequence,
    project_media: Arc<BTreeMap<MediaId, PrMedia>>,
    omissions: &mut Vec<Omission>,
    resolver: Option<&'a mut crate::LinkedCompositionResolver<'a>>,
    media_map: Option<&'a ValidatedMediaMap>,
    progress: fx_conv::Progress<'_>,
) -> Result<Option<PendingTesseractFile>> {
    let root = source
        .parent()
        .ok_or_else(|| unsupported("project parent missing"))?;
    let preflight = inspect_premiere_sequence(source, &parsed, &project_media, media_map)?;
    require_video_admission(&preflight)?;
    let mut digests = HashMap::new();
    let mut linked = LinkedCompositions::new(resolver, media_map);
    // One omitted media source applies to every placement of that source.
    let inspected: BTreeMap<MediaId, MediaInspection> = parsed
        .media_in_order()
        .into_iter()
        .map(|id| {
            Ok((
                id.clone(),
                inspect_media(
                    root,
                    id,
                    &project_media[id],
                    Some(&parsed),
                    &mut digests,
                    &mut linked,
                    media_map,
                )?,
            ))
        })
        .collect::<Result<_>>()?;
    for track in &mut parsed.video_tracks {
        track.items.retain(|item| {
            // Graphics use synthetic generator media; there is no file to inspect.
            let PrVideoItem::Media(clip) = item else {
                return true;
            };
            match &inspected[&clip.media] {
                MediaInspection::Ready(media) => {
                    if let Some(MediaNote::Feature(note)) = media.note {
                        omit(omissions, OmissionScope::Feature, clip.record(), note);
                    }
                    true
                }
                MediaInspection::Synthetic | MediaInspection::Linked { .. } => true,
                MediaInspection::Omitted(reason) => {
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        clip.record(),
                        reason.clone(),
                    );
                    false
                }
            }
        });
    }
    parsed.audio.retain(|clip| {
        let reason = match &inspected[&clip.media] {
            MediaInspection::Ready(_) | MediaInspection::Synthetic => return true,
            // The reader omits these; a sound must never double a muted picture.
            MediaInspection::Linked { .. } => LINKED_AUDIO_REASON,
            MediaInspection::Omitted(reason) => reason,
        };
        omit(omissions, OmissionScope::Occurrence, clip.record(), reason);
        false
    });
    // Nested placements follow the same decision for their media.
    parsed.retain_nested_media(
        &|id| match &inspected[id] {
            MediaInspection::Ready(_)
            | MediaInspection::Synthetic
            | MediaInspection::Linked { .. } => None,
            MediaInspection::Omitted(reason) => Some(reason.clone()),
        },
        omissions,
    );
    if parsed.video_items().next().is_none()
        && parsed.nest_occurrences().next().is_none()
        && parsed.audio.is_empty()
    {
        omit(
            omissions,
            OmissionScope::Sequence,
            format!(
                "{} ({:?})",
                parsed.id.as_deref().unwrap_or_default(),
                parsed.name
            ),
            "no convertible video or audio occurrences",
        );
        return Ok(None);
    }
    let asset_ids = asset_ids_in_order(&parsed, &project_media);
    // Every retained occurrence has inspected media; media with no retained
    // occurrence is neither packaged nor rechecked.
    let placed: BTreeSet<MediaId> = parsed.media_in_order().into_iter().cloned().collect();
    let mut media = BTreeMap::new();
    for (id, inspected) in inspected {
        let resolved = match inspected {
            MediaInspection::Ready(inspected) => {
                let Some(asset_id) = asset_ids.get(&id) else {
                    continue;
                };
                if let Some(MediaNote::Approximation(note)) = inspected.note {
                    approximate(
                        omissions,
                        format!("{id} ({:?})", project_media[&id].name),
                        note,
                    );
                }
                if let Some(colour) = inspected.colour {
                    // The media record's identity keeps two same-named files apart
                    // (`push_omission` drops repeated omissions); the name reads.
                    approximate(
                        omissions,
                        format!("{id} ({:?})", project_media[&id].name),
                        colour.passthrough_warning(),
                    );
                }
                ResolvedMedia {
                    path: inspected.path,
                    hash: inspected.hash,
                    native_path: inspected.native_path,
                    native_hash: inspected.native_hash,
                    candidate_paths: inspected.candidate_paths,
                    asset: Some((asset_id.clone(), inspected.container)),
                }
            }
            MediaInspection::Linked {
                path,
                hash,
                candidate_paths,
            } if placed.contains(&id) => ResolvedMedia {
                native_path: path.clone(),
                native_hash: hash.clone(),
                path,
                hash,
                candidate_paths,
                asset: None,
            },
            MediaInspection::Linked { .. }
            | MediaInspection::Synthetic
            | MediaInspection::Omitted(_) => continue,
        };
        media.insert(id, resolved);
    }
    let document = crate::convert::sequence_document_with_progress(
        &parsed,
        &project_media,
        &asset_ids,
        &mut linked,
        omissions,
        progress,
    )?;
    let mut builder = TesseractFileBuilder::try_new(document)?;
    for id in parsed.media_in_order() {
        // Generator media has no file to package, and a linked AEP is not
        // packaged: its pictures' media is.
        let Some(resolved) = media.get(id) else {
            continue;
        };
        let Some((asset_id, container)) = &resolved.asset else {
            continue;
        };
        // The container decides the asset kind, as export admission does, so
        // a sound-only MP4 or MOV source is packaged as the Video asset it is.
        builder = builder.add_asset(asset_id.as_str(), &resolved.path, container.asset_kind())?;
    }
    let (builder, linked_media) = linked.package(builder)?;
    builder.validate()?;
    let output = PendingTesseractFile {
        source: source.to_owned(),
        project_media,
        media,
        linked_media,
        builder: Some(builder),
    };
    // Check mode stops here, so media must still match the bytes we inspected.
    output.verify_media()?;
    Ok(Some(output))
}

impl PendingTesseractFile {
    fn verify_media(&self) -> Result<()> {
        let root = self
            .source
            .parent()
            .ok_or_else(|| unsupported("project parent missing"))?;
        let mut digests = HashMap::new();
        // A linked AEP is rechecked with the other media, as a record without an asset.
        for (id, resolved) in &self.media {
            let (path, digest, _) = resolve_media(root, &self.project_media[id], &mut digests)?;
            ensure!(
                path == resolved.native_path && digest == resolved.native_hash,
                "native media identity changed during the Tesseract build"
            );
            for candidate in &resolved.candidate_paths {
                ensure!(
                    media_hash(candidate, &mut digests)? == resolved.native_hash,
                    "native media candidate changed during the Tesseract build"
                );
            }
            ensure!(
                media_hash(&resolved.path, &mut digests)? == resolved.hash,
                "selected media changed during the Tesseract build"
            );
        }
        self.linked_media.verify_sources()?;
        Ok(())
    }

    /// Write into batch-owned staging. The batch removes staged files on failure
    /// and publishes only after all source and media checks succeed.
    pub(crate) fn write_to_staging(mut self, output: &Path) -> Result<()> {
        let written = self
            .builder
            .take()
            .expect("validated builder is available once")
            .write(output)?;
        for resolved in self.media.values() {
            let Some((asset_id, _)) = &resolved.asset else {
                continue;
            };
            // The archive writer hashes the bytes as it copies them. Compare
            // that digest with the inspected source, without rereading the payload.
            let asset = written.asset(asset_id.as_str())?;
            ensure!(
                asset.descriptor().sha256 == resolved.hash,
                "packaged source bytes changed"
            );
        }
        self.linked_media.verify_packaged(&written)?;
        self.verify_media()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::support::{video_media, video_sequence};

    #[test]
    fn shared_placements_have_one_dense_asset_id_in_first_appearance_order() {
        let mut sequence = video_sequence();
        let original = sequence.video_tracks[0].clip(0).clone();
        for id in ["source", "second", "source", "third"] {
            let mut clip = original.clone();
            clip.media = MediaId(id.into());
            sequence.video_tracks[0]
                .items
                .push(PrVideoItem::Media(clip));
        }
        let ids = asset_ids_in_order(&sequence, &video_media());
        assert_eq!(ids.len(), 3);
        assert_eq!(ids[&MediaId("source".into())].as_str(), "premiere-video-1");
        assert_eq!(ids[&MediaId("second".into())].as_str(), "premiere-video-2");
        assert_eq!(ids[&MediaId("third".into())].as_str(), "premiere-video-3");
    }
}
