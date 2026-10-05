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
    schema::PrMediaKind,
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

mod audio_channels;
mod delayed_audio;
mod object_masks;

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

/// Resolve the ordinary, identity-checked original before recovering an absent
/// native PAR override. No prepared replacement or coded dimensions supply PAR.
pub(crate) fn source_pixel_aspect(
    root: &Path,
    media: &PrMedia,
    kind: PrMediaKind,
) -> Result<(crate::schema::records::PixelAspectRatio, &'static str)> {
    let (path, expected_hash, _) = resolve_media(root, media, &mut HashMap::new())?;
    let aspect = match kind {
        PrMediaKind::Still { .. } => {
            let facts = crate::image_media::inspect_image_media(File::open(&path)?)?;
            ensure!(facts.format == crate::image_media::ImageFormat::Png,
                "missing native PAR override: source image pixel aspect is unavailable for this format");
            crate::image_media::inspect_png_pixel_aspect(File::open(&path)?)?
        }
        PrMediaKind::Video { .. } => {
            let file = File::open(&path)?;
            let facts = crate::media::inspect_video_media(
                BufReader::new(file),
                BufReader::new(File::open(&path)?),
                std::fs::metadata(&path)?.len(),
            )?;
            (facts.pixel_aspect, "validated movie metadata")
        }
        _ => {
            return Err(unsupported(
                "missing native PAR override has no physical source pixel-aspect metadata",
            ))
        }
    };
    ensure!(
        hash(&path)? == expected_hash,
        "source changed during missing-PAR metadata inspection"
    );
    Ok(aspect)
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
    // Keep extracted channel files alive until the archive builder is dropped.
    _channel_files: Option<tempfile::TempDir>,
    _raw_audio_files: Option<tempfile::TempDir>,
    _mask_files: tempfile::TempDir,
}

#[derive(Debug)]
struct ResolvedMedia {
    numbered_frames: Vec<NumberedFrame>,
    path: PathBuf,
    hash: String,
    native_path: PathBuf,
    native_hash: String,
    candidate_paths: Vec<PathBuf>,
    /// The archive asset of this file and its admitted container. A linked
    /// AEP is no asset: its pictures package their own media.
    asset: Option<(AssetId, MediaContainer)>,
}

/// Every saved frame is inspected and pinned, including frames outside a trim.
#[derive(Debug)]
struct NumberedFrame {
    path: PathBuf,
    hash: String,
    candidates: Vec<PathBuf>,
    container: MediaContainer,
}

/// The resolved source file of one media record, shared by all of its placements.
struct InspectedMedia {
    delayed_audio: Option<Box<crate::audio_media::DelayedAudioClock>>,
    /// Unsupported embedded sound must not remove an independently admitted picture.
    audio_omission: Option<String>,
    picture_clock: Option<Box<Result<crate::media::PictureClock>>>,
    numbered_frames: Vec<NumberedFrame>,
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
    PresentationOrigin {
        origin: crate::media::PresentationOrigin,
        timescale: u32,
        declared_tail: bool,
    },
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
    /// A missing direct video or unsupported codec may drop its placements,
    /// unlike a failed identity, source-clock or container validation.
    UnavailableVideo(String),
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

/// Apply a sound-only inspection loss at every placement, including nested
/// copies, before channel extraction. Picture identity and clocks stay intact.
fn omit_unreadable_sounds(
    sequence: &mut PrSequence,
    inspected: &BTreeMap<MediaId, MediaInspection>,
    omissions: &mut Vec<Omission>,
) {
    sequence.audio.retain(|clip| {
        if let Some(MediaInspection::Ready(media)) = inspected.get(&clip.media) {
            if let Some(reason) = &media.audio_omission {
                omit(omissions, OmissionScope::Occurrence, clip.record(), reason);
                return false;
            }
        }
        true
    });
    for track in &mut sequence.video_tracks {
        for nest in &mut track.nests {
            omit_unreadable_sounds(&mut nest.sequence, inspected, omissions);
        }
    }
}

#[derive(Clone, Copy)]
enum MediaUse<'a> {
    Loaded(&'a PrSequence),
    Native(&'a crate::media::VideoUse),
    Whole,
}

impl MediaUse<'_> {
    fn uses_audio(self, id: &MediaId) -> bool {
        match self {
            Self::Loaded(sequence) => sequence.uses_audio_media(id),
            Self::Native(usage) => usage.uses_audio,
            Self::Whole => true,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VideoAdmission {
    Strict,
    OmitUnsupported,
}

struct MediaSelection<'a> {
    usage: MediaUse<'a>,
    admission: VideoAdmission,
}

/// Resolve and inspect one media record `id`. Import may omit a missing direct
/// video or unsupported codec; all other video admission failures stay fatal.
fn inspect_media(
    root: &Path,
    id: &MediaId,
    media: &PrMedia,
    selection: MediaSelection<'_>,
    digests: &mut HashMap<PathBuf, String>,
    linked: &mut LinkedCompositions<'_>,
    media_map: Option<&ValidatedMediaMap>,
) -> Result<MediaInspection> {
    let MediaSelection {
        usage: selection,
        admission,
    } = selection;
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
        Some(
            PrMediaKind::Video { .. }
            | PrMediaKind::Still { .. }
            | PrMediaKind::NumberedStills { .. },
        )
        | None => {}
    }
    let (native_path, native_hash, candidate_paths) = match resolve_media(root, media, digests) {
        Ok(resolved) => resolved,
        Err(error @ BuildError::MissingMedia(_)) => {
            let is_video = media
                .video
                .as_ref()
                .is_some_and(|video| matches!(video.kind, PrMediaKind::Video { .. }));
            if is_video {
                return if admission == VideoAdmission::OmitUnsupported {
                    Ok(MediaInspection::UnavailableVideo(error.to_string()))
                } else {
                    Err(error)
                };
            }
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
    if let Some(native) = media.video.as_ref() {
        if let PrMediaKind::NumberedStills { alpha } = native.kind {
            if path != native_path {
                return Ok(MediaInspection::Omitted("numbered-image replacement must preserve the complete saved frame set; single-file replacement is unsupported".into()));
            }
            let frames =
                match inspect_numbered_frames(&path, &candidate_paths, native, alpha, digests) {
                    Ok(frames) => frames,
                    Err(error) => {
                        return unsupported_media_reason(error).map(MediaInspection::Omitted)
                    }
                };
            let container = frames[0].container;
            return Ok(MediaInspection::Ready(InspectedMedia {
                delayed_audio: None,
                audio_omission: None,
                picture_clock: None,
                numbered_frames: frames,
                path, hash: native_hash.clone(), native_path, native_hash, candidate_paths,
                container, note: Some(MediaNote::Approximation("numbered images retain original bytes and saved numeric order as editable sequence-sampled image holds; other output rates, start phases and sub-frame timing are approximate or unproved; independent Premiere fidelity is unverified")), colour: None, codec: None,
            }));
        }
    }
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
    let interpreted = media.video.as_ref().is_some_and(|video| {
        !matches!(
            video.interpretation,
            crate::schema::SourceInterpretation::Original
        )
    });
    let loaded_use = match selection {
        MediaUse::Loaded(sequence) => selected_video_use(sequence, id),
        _ => None,
    };
    let usage = match selection {
        MediaUse::Native(usage) => Some(usage),
        _ => loaded_use.as_ref(),
    }
    .filter(|_| !interpreted);
    let mut picture_clock = None;
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
            usage,
        ) {
            Ok(facts) => facts,
            Err(error @ BuildError::UnsupportedVideoCodec(_))
                if is_video && admission == VideoAdmission::OmitUnsupported =>
            {
                return Ok(MediaInspection::UnavailableVideo(error.to_string()));
            }
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
            MediaFacts::Video(video) if interpreted => {
                picture_clock = Some(Box::new(
                    crate::media::InterpretedPictureClock::bind(native, video)
                        .map(crate::media::PictureClock::Interpreted),
                ));
                Ok(false)
            }
            MediaFacts::Video(video) => {
                let selected_only = video.validate_source_for_use(native, usage)?;
                if let Some(origin) = video.timing.presentation_origin() {
                    picture_clock = Some(Box::new(Ok(
                        crate::media::PictureClock::PresentationOrigin(origin),
                    )));
                    note = Some(MediaNote::PresentationOrigin {
                        origin,
                        timescale: video.timing.timescale,
                        declared_tail: video.timing.declared_media_end.is_some(),
                    });
                }
                Ok(selected_only)
            }
            MediaFacts::Still(_) | MediaFacts::UnsupportedVideo(_) => {
                facts.validate_source(native).map(|()| false)
            }
        }?;
        if selected_only {
            let declared_tail = matches!(
                &facts,
                MediaFacts::Video(video) if video.timing.declared_media_end.is_some()
            );
            note.get_or_insert(MediaNote::Approximation(if declared_tail {
                "declared media-header tail has no samples; original sample timestamps, bytes and first affine MP4 edit retained for proved selected picture-only unit intervals; the uncertain final sample and whole-source playback remain unsupported; native intermediate frame selection is unverified"
            } else {
                "original sample timestamps and first affine MP4 edit retained for proved selected unit source intervals; whole-source endpoint and later edit playback are unsupported"
            }));
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
            if matches!(
                video.codec,
                crate::schema::VideoCodec::ProRes {
                    profile: crate::schema::video_codec::ProResProfile::P4444,
                    ..
                }
            ) {
                note.get_or_insert(MediaNote::Approximation(
                    "ProRes 4444 picture and alpha packets retained unchanged for native FFmpeg playback; WebCodecs playback is unavailable and native RGB/alpha fidelity is unverified",
                ));
            }
            if video.timing.legacy_signed_ctts {
                note.get_or_insert(MediaNote::Approximation(
                    "legacy CTTSv0 signed offsets retained exactly as the shared parser/player evaluates them after physical PTS/count/origin checks; whole-source admission requires the validated full-duration edit; other edits require selected unit interior intervals; native intermediate frame selection remains unverified",
                ));
            }
            // Import admitted the codec (`media::inspect_media`), so the
            // family name exists; native-only playback is diagnosed above.
            codec = video.codec.tesseract_name().map(str::to_owned);
            if matches!(
                video.timing.clock,
                crate::media::SampleClock::Irregular { .. }
            ) {
                // A complete physical clock needs no selected-use proof. The
                // editable playback maps parent time to source time; the decoder
                // then selects the nearest-earlier retained PTS, including for
                // reverse and remapped requests. Do not replace that lookup with
                // the native average period or require omitted siblings to prove
                // clocks that only partial-source admission depends on.
                let whole_source = !video.timing.partial_timeline && !selected_only && !interpreted;
                if usage.is_none() && !whole_source {
                    return Ok(MediaInspection::Omitted(
                        "irregular source without a validated whole-source clock requires unit forward playback without Time Remapping, proved through every containing nest".into(),
                    ));
                }
                note.get_or_insert(MediaNote::Approximation(
                    "irregular source presentation timestamps retained with original bytes after exact native sample-count and bounded endpoint checks; editable retimes map source times onto nearest-earlier sample starts under millisecond rounding; native intermediate frame selection remains unverified",
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
    let mut audio_omission = None;
    let mut delayed_audio = None;
    'sound: {
        if let Some(native) = media.audio.as_ref().filter(|_| selection.uses_audio(id)) {
            let input = File::open(&path)?;
            let size = input.metadata()?.len();
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            let inspected_audio = (|| {
                if matches!(
                    container,
                    MediaContainer::Mp4 | MediaContainer::Mov | MediaContainer::M4a
                ) {
                    delayed_audio = crate::audio_media::inspect_delayed_audio(
                        BufReader::new(File::open(&path)?),
                        size,
                        native,
                    )?
                    .map(Box::new);
                    if delayed_audio.is_some() {
                        return Ok(Some(native.clone()));
                    }
                }
                inspect_audio_media(BufReader::new(input), size, extension)
            })();
            let facts = match inspected_audio {
                Ok(Some(facts)) => facts,
                Ok(None) => {
                    return Ok(MediaInspection::Omitted(
                        "native audio stream is missing from the file".into(),
                    ));
                }
                Err(error) => {
                    let reason = unsupported_media_reason(error)?;
                    if !is_video {
                        return Ok(MediaInspection::Omitted(reason));
                    }
                    audio_omission = Some(format!("embedded sound not imported: {reason}"));
                    break 'sound;
                }
            };
            if delayed_audio.is_some() {
                break 'sound;
            }
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
                Ok(crate::audio_media::AudioDurationMatch::RoundedUpToSample) => {
                    note.get_or_insert(MediaNote::Approximation(
                    "native fractional audio duration and measured whole-sample duration share the same sample ceiling; source clocks and selected ranges are unchanged",
                ));
                }
                Ok(crate::audio_media::AudioDurationMatch::PaddedToPicture) => {
                    note.get_or_insert(MediaNote::Approximation(
                    "embedded AAC ends up to one frame before the picture; Premiere's picture-length audio duration was admitted and the tail plays as silence",
                ));
                }
                Err(error) => return Ok(MediaInspection::Omitted(error.to_string())),
            }
        }
    }
    Ok(MediaInspection::Ready(InspectedMedia {
        audio_omission,
        delayed_audio,
        picture_clock,
        numbered_frames: Vec::new(),
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

/// Compare complete live aliases, not just their identical first PNGs.
fn inspect_numbered_frames(
    path: &Path,
    candidates: &[PathBuf],
    native: &crate::schema::PrVideoStream,
    alpha: bool,
    digests: &mut HashMap<PathBuf, String>,
) -> Result<Vec<NumberedFrame>> {
    let paths = crate::numbered_images::paths(path, native)?;
    let aliases = candidates
        .iter()
        .map(|candidate| crate::numbered_images::paths(candidate, native))
        .collect::<Result<Vec<_>>>()?;
    paths
        .into_iter()
        .enumerate()
        .map(|(index, path)| {
            let hash = media_hash(&path, digests)?;
            tesseract_file::validate_asset_source_name(&path)?;
            let image = crate::image_media::inspect_image_media(File::open(&path)?)?;
            ensure!(
                (image.width, image.height) == (native.width, native.height),
                "numbered-image frame {} dimensions differ from native dimensions",
                path.display()
            );
            if let Some(reason) =
                image.declaration_mismatch(path.extension().and_then(|value| value.to_str()), alpha)
            {
                return Err(unsupported(format!(
                    "numbered-image frame {}: {reason}",
                    path.display()
                )));
            }
            ensure!(
                !image.icc_profile,
                "numbered-image frame {} embeds an unverified ICC colour profile",
                path.display()
            );
            let candidates = aliases
                .iter()
                .map(|paths| paths[index].clone())
                .collect::<Vec<_>>();
            for candidate in &candidates {
                ensure!(
                    media_hash(candidate, digests)? == hash,
                    "numbered-image aliases identify different frame bytes: {}",
                    candidate.display()
                );
            }
            Ok(NumberedFrame {
                path,
                hash,
                candidates,
                container: MediaContainer::Image(image.format),
            })
        })
        .collect()
}

fn preflight_failure(error: &BuildError) -> (MediaStatus, Option<String>) {
    let status = match error {
        BuildError::MissingMedia(_) => MediaStatus::Missing,
        BuildError::Io(_) | BuildError::IoAt { .. } => MediaStatus::Unreadable,
        BuildError::Mp4(_) | BuildError::Audio(_) => MediaStatus::InvalidMedia,
        BuildError::Unsupported(_) | BuildError::UnsupportedVideoCodec(_) => {
            MediaStatus::RequiresTranscode
        }
        _ => MediaStatus::Unassessed,
    };
    (status, Some(error.to_string()))
}

/// Inspect one loaded Premiere sequence under the import omission policy.
fn inspect_import_sequence(
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
        // Conversion packages exactly what this loaded sequence places.
        |_| MediaUse::Loaded(parsed),
        VideoAdmission::OmitUnsupported,
        media_map,
    )
}

pub(crate) fn inspect_native_premiere_media(
    source: &Path,
    target: &str,
    media_map: Option<&ValidatedMediaMap>,
) -> Result<MediaPreflight> {
    // A media takes the model's clocks only when the native inventory proves
    // that each of its uses survives there. Failed loads retain strict inspection.
    let parsed = crate::format::PrProjectFile::load_import(source, Some(target))
        .ok()
        .map(|(project, _)| project);
    let sequence = parsed
        .as_ref()
        .and_then(|project| project.single_sequence());
    inspect_native_media(
        source,
        target,
        sequence,
        VideoAdmission::Strict,
        media_map,
        None,
    )
}

/// Native inventory; a media is inspected for its selected use in `parsed`
/// only when every native picture and sound use of it survives there.
#[cfg(all(test, feature = "ffmpeg-library"))]
pub(crate) fn inspect_native_premiere_media_for_sequence(
    source: &Path,
    parsed: &PrSequence,
    media_map: Option<&ValidatedMediaMap>,
) -> Result<MediaPreflight> {
    inspect_native_media(
        source,
        parsed.id.as_deref().unwrap_or_default(),
        Some(parsed),
        VideoAdmission::Strict,
        media_map,
        None,
    )
}

/// Validate native uses before import, allowing only unavailable direct videos
/// to be omitted. Unresolved selected-use clocks retain whole-source admission.
pub(crate) fn inspect_native_premiere_media_for_import(
    source: &Path,
    parsed: &PrSequence,
    media_map: Option<&ValidatedMediaMap>,
    relink: Option<&crate::ValidatedMediaRelink>,
) -> Result<MediaPreflight> {
    inspect_native_media(
        source,
        parsed.id.as_deref().unwrap_or_default(),
        Some(parsed),
        VideoAdmission::OmitUnsupported,
        media_map,
        relink,
    )
}

pub(crate) fn inspect_native_premiere_media_with_relink(
    source: &Path,
    target: &str,
    relink: &crate::ValidatedMediaRelink,
) -> Result<MediaPreflight> {
    let (project, _) = crate::format::PrProjectFile::load_import_with_media_relink(
        source,
        Some(target),
        Some(relink),
    )?;
    inspect_native_media(
        source,
        target,
        project.single_sequence(),
        VideoAdmission::Strict,
        None,
        Some(relink),
    )
}

fn inspect_native_media(
    source: &Path,
    target: &str,
    sequence: Option<&PrSequence>,
    admission: VideoAdmission,
    media_map: Option<&ValidatedMediaMap>,
    relink: Option<&crate::ValidatedMediaRelink>,
) -> Result<MediaPreflight> {
    let native = if relink.is_some() {
        crate::format::PrProjectFile::native_media_scope_with_media_relink(source, target, relink)?
    } else {
        crate::format::PrProjectFile::native_media_scope(source, target)?
    };
    let direct: BTreeMap<_, _> = native
        .media
        .keys()
        .filter_map(|id| native.direct_video_use(target, id).map(|usage| (id, usage)))
        .collect();
    let mut report = inspect_media_references(
        source,
        target.to_owned(),
        &native.media,
        native.media.keys().collect(),
        |id| {
            sequence
                .filter(|sequence| native.covers(sequence, id))
                .map(MediaUse::Loaded)
                .or_else(|| direct.get(id).map(MediaUse::Native))
                .unwrap_or(MediaUse::Whole)
        },
        admission,
        media_map,
    )?;
    report.unassessed.extend(native.unassessed);
    Ok(report)
}

#[cfg(all(test, feature = "ffmpeg-library"))]
pub(crate) fn require_video_admission(preflight: &MediaPreflight, source: &Path) -> Result<()> {
    require_video_admission_for(preflight, source, VideoAdmission::Strict)
}

pub(crate) fn require_import_video_admission(
    preflight: &MediaPreflight,
    source: &Path,
) -> Result<()> {
    // Import inspection propagates every direct-video error before this gate.
    // Its omitted native sources are safe to drop; linked footage stays strict.
    require_video_admission_for(preflight, source, VideoAdmission::OmitUnsupported)
}

fn require_video_admission_for(
    preflight: &MediaPreflight,
    source: &Path,
    admission: VideoAdmission,
) -> Result<()> {
    if let Some(blocked) = preflight.media.iter().find(|media| {
        // Linked AEP inventory retains its native owner. Only genuinely absent
        // linked footage takes AE's existing contextual omission policy. Import
        // has already rejected unsafe direct-video errors before this gate.
        let missing_linked = media.owner != source && media.status == MediaStatus::Missing;
        let omitted_native = admission == VideoAdmission::OmitUnsupported && media.owner == source;
        media.kind == MediaKind::Video
            && media.status != MediaStatus::Supported
            && !missing_linked
            && !omitted_native
    }) {
        return Err(unsupported(format!(
            "video media {} ({:?}) failed admission: {}",
            blocked.id,
            blocked.name,
            blocked.reason.as_deref().unwrap_or("unassessed media")
        )));
    }
    Ok(())
}

/// Inspect the media records `ids`. `selected` returns the loaded sequence
/// whose placements are the whole use of one media record; without it, the
/// record is inspected as a whole source whose embedded sound may play.
fn inspect_media_references<'a>(
    source: &Path,
    target: String,
    project_media: &BTreeMap<MediaId, PrMedia>,
    ids: Vec<&MediaId>,
    selected: impl Fn(&MediaId) -> MediaUse<'a>,
    admission: VideoAdmission,
    media_map: Option<&ValidatedMediaMap>,
) -> Result<MediaPreflight> {
    let root = source
        .parent()
        .ok_or_else(|| unsupported("project parent missing"))?;
    let owner = source.to_owned();
    let mut digests = HashMap::new();
    let mut linked = LinkedCompositions::new(media_map);
    let mut report = MediaPreflight {
        format: "premiere".to_owned(),
        target: target.clone(),
        media: Vec::new(),
        unassessed: Vec::new(),
    };

    for id in ids {
        let media = &project_media[id];
        let sequence = selected(id);
        if media.is_generator()
            || media.video.as_ref().is_some_and(|video| {
                matches!(
                    video.kind,
                    PrMediaKind::Still { .. } | PrMediaKind::NumberedStills { .. }
                )
            })
        {
            continue;
        }
        if media.after_effects_composition().is_some() {
            match inspect_media(
                root,
                id,
                media,
                MediaSelection {
                    usage: sequence,
                    admission,
                },
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
            MediaSelection {
                usage: sequence,
                admission,
            },
            &mut digests,
            &mut linked,
            media_map,
        );
        let mut codec = None;
        let (mut status, mut reason) = match inspected {
            Ok(MediaInspection::Ready(inspected)) => {
                codec = inspected.codec;
                (MediaStatus::Supported, inspected.audio_omission)
            }
            Ok(MediaInspection::Omitted(reason))
                if kind == MediaKind::Video && admission == VideoAdmission::OmitUnsupported =>
            {
                return Err(unsupported(format!(
                    "video media {id} ({:?}) failed admission: {reason}",
                    media.name
                )));
            }
            Ok(MediaInspection::Omitted(reason) | MediaInspection::UnavailableVideo(reason)) => {
                (MediaStatus::RequiresTranscode, Some(reason))
            }
            Ok(_) => (
                MediaStatus::Unassessed,
                Some("native media was not inspected as a file".to_owned()),
            ),
            Err(error)
                if kind == MediaKind::Video && admission == VideoAdmission::OmitUnsupported =>
            {
                return Err(BuildError::Context {
                    context: format!("video media {id} ({:?}) failed admission", media.name),
                    source: Box::new(error),
                });
            }
            Err(error) => preflight_failure(&error),
        };
        if let Err(error) = resolved {
            if admission == VideoAdmission::OmitUnsupported
                && kind == MediaKind::Video
                && !matches!(error, BuildError::MissingMedia(_))
            {
                return Err(BuildError::Context {
                    context: format!("video media {id} ({:?}) failed admission", media.name),
                    source: Box::new(error),
                });
            }
            (status, reason) = preflight_failure(&error);
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
                    if admission == VideoAdmission::OmitUnsupported && kind == MediaKind::Video {
                        return Err(BuildError::Context {
                            context: format!(
                                "video media {id} ({:?}) failed admission",
                                media.name
                            ),
                            source: Box::new(error.into()),
                        });
                    }
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
    convert_premiere_sequence_with_progress(
        source,
        parsed,
        project_media,
        omissions,
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
        Some(media_map),
        progress,
    )
}

/// Convert a sequence with command-scoped progress observations.
pub(crate) fn convert_premiere_sequence_with_progress(
    source: &Path,
    parsed: PrSequence,
    project_media: Arc<BTreeMap<MediaId, PrMedia>>,
    omissions: &mut Vec<Omission>,
    progress: fx_conv::Progress<'_>,
) -> Result<Option<PendingTesseractFile>> {
    convert_premiere_sequence_with_options(source, parsed, project_media, omissions, None, progress)
}

fn convert_premiere_sequence_with_options(
    source: &Path,
    mut parsed: PrSequence,
    mut project_media: Arc<BTreeMap<MediaId, PrMedia>>,
    omissions: &mut Vec<Omission>,
    media_map: Option<&ValidatedMediaMap>,
    progress: fx_conv::Progress<'_>,
) -> Result<Option<PendingTesseractFile>> {
    let root = source
        .parent()
        .ok_or_else(|| unsupported("project parent missing"))?;
    let preflight = inspect_import_sequence(source, &parsed, &project_media, media_map)?;
    require_import_video_admission(&preflight, source)?;
    let mut digests = HashMap::new();
    let mut linked = LinkedCompositions::new(media_map);
    // One omitted media source applies to every placement of that source.
    let mut inspected: BTreeMap<MediaId, MediaInspection> = parsed
        .media_in_order()
        .into_iter()
        .map(|id| {
            Ok((
                id.clone(),
                inspect_media(
                    root,
                    id,
                    &project_media[id],
                    MediaSelection {
                        usage: MediaUse::Loaded(&parsed),
                        admission: VideoAdmission::OmitUnsupported,
                    },
                    &mut digests,
                    &mut linked,
                    media_map,
                )?,
            ))
        })
        .collect::<Result<_>>()?;
    omit_unreadable_sounds(&mut parsed, &inspected, omissions);
    let raw_audio_files =
        delayed_audio::prepare(&mut parsed, &mut project_media, &mut inspected, omissions)?;
    let channel_files =
        audio_channels::prepare(&mut parsed, &mut project_media, &mut inspected, omissions)?;
    let mask_files = object_masks::prepare(
        source,
        &mut parsed,
        &mut project_media,
        &mut inspected,
        omissions,
    )?;
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
                MediaInspection::Omitted(reason) | MediaInspection::UnavailableVideo(reason) => {
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
            MediaInspection::Ready(_)
            | MediaInspection::Synthetic
            | MediaInspection::Linked { .. } => return true,
            MediaInspection::Omitted(reason) | MediaInspection::UnavailableVideo(reason) => reason,
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
            MediaInspection::Omitted(reason) | MediaInspection::UnavailableVideo(reason) => {
                Some(reason.clone())
            }
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
    let mut picture_clocks = crate::media::PictureClocks::new();
    for (id, inspected) in inspected {
        let resolved = match inspected {
            MediaInspection::Ready(inspected) => {
                if let Some(clock) = inspected.picture_clock {
                    picture_clocks.insert(id.clone(), *clock);
                }
                let Some(asset_id) = asset_ids.get(&id) else {
                    continue;
                };
                match inspected.note {
                    Some(MediaNote::Approximation(note)) => {
                        approximate(
                            omissions,
                            format!("{id} ({:?})", project_media[&id].name),
                            note,
                        );
                    }
                    Some(MediaNote::PresentationOrigin {
                        origin,
                        timescale,
                        declared_tail,
                    }) => {
                        let tail = if declared_tail {
                            "declared media-header tail has no samples; "
                        } else {
                            ""
                        };
                        approximate(
                            omissions,
                            format!("{id} ({:?})", project_media[&id].name),
                            format!("{tail}displayed video zero origin is translated by {}/{timescale} seconds into the decoder source clock, rounded forward to {} ms in editable source trim/playback; original bytes, sample identities and timestamps are retained; selected picture-only unit intervals stay inside proved coverage; final-sample/whole-source playback and native intermediate frame parity remain unverified", origin.units, origin.offset_millis),
                        );
                    }
                    _ => {}
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
                    numbered_frames: inspected.numbered_frames,
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
                numbered_frames: Vec::new(),
                native_path: path.clone(),
                native_hash: hash.clone(),
                path,
                hash,
                candidate_paths,
                asset: None,
            },
            MediaInspection::Linked { .. }
            | MediaInspection::Synthetic
            | MediaInspection::Omitted(_)
            | MediaInspection::UnavailableVideo(_) => continue,
        };
        media.insert(id, resolved);
    }
    let document = crate::convert::sequence_document_with_progress(
        &parsed,
        &project_media,
        &asset_ids,
        &picture_clocks,
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
        if resolved.numbered_frames.is_empty() {
            builder =
                builder.add_asset(asset_id.as_str(), &resolved.path, container.asset_kind())?;
        } else {
            for (index, frame) in resolved.numbered_frames.iter().enumerate() {
                builder = builder.add_asset(
                    crate::numbered_images::frame_asset(asset_id, index).as_str(),
                    &frame.path,
                    frame.container.asset_kind(),
                )?;
            }
        }
    }
    let (builder, linked_media) = linked.package(builder)?;
    builder.validate()?;
    let output = PendingTesseractFile {
        source: source.to_owned(),
        project_media,
        media,
        linked_media,
        builder: Some(builder),
        _channel_files: channel_files,
        _raw_audio_files: raw_audio_files,
        _mask_files: mask_files,
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
            for frame in &resolved.numbered_frames {
                for candidate in &frame.candidates {
                    ensure!(
                        media_hash(candidate, &mut digests)? == frame.hash,
                        "numbered-image frame changed during the Tesseract build: {}",
                        candidate.display()
                    );
                }
            }
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
            if !resolved.numbered_frames.is_empty() {
                for (index, frame) in resolved.numbered_frames.iter().enumerate() {
                    let asset = written
                        .asset(crate::numbered_images::frame_asset(asset_id, index).as_str())?;
                    ensure!(
                        asset.descriptor().sha256 == frame.hash,
                        "packaged numbered-image frame bytes changed"
                    );
                }
                continue;
            }
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
    use std::fs;

    #[cfg(feature = "ffmpeg-library")]
    fn ready_delayed_source(
        root: &Path,
    ) -> (
        Arc<BTreeMap<MediaId, PrMedia>>,
        BTreeMap<MediaId, MediaInspection>,
    ) {
        use crate::schema::{AudioChannels, PrAudioStream, TICKS};
        let bytes = crate::tests::support::delayed_aac_bytes();
        fs::write(root.join("sound.m4a"), bytes).unwrap();
        let mut media = video_media();
        let id = MediaId("source".into());
        let source = media.get_mut(&id).unwrap();
        source.video = None;
        source.relative_path = Some("sound.m4a".into());
        source.relative_paths = vec!["sound.m4a".into()];
        source.audio = Some(PrAudioStream {
            prepared_clock: None,
            intrinsic_ticks: TICKS / 5,
            channels: AudioChannels::Stereo,
            sample_rate: 48_000,
        });
        let inspected = inspect_media(
            root,
            &id,
            source,
            MediaSelection {
                usage: MediaUse::Whole,
                admission: VideoAdmission::Strict,
            },
            &mut HashMap::new(),
            &mut LinkedCompositions::new(None),
            None,
        )
        .unwrap();
        assert!(
            matches!(&inspected, MediaInspection::Ready(facts) if facts.delayed_audio.is_some())
        );
        (Arc::new(media), BTreeMap::from([(id, inspected)]))
    }

    #[cfg(feature = "ffmpeg-library")]
    #[test]
    fn unused_ready_delayed_audio_does_not_prepare_or_block_picture_import() {
        use crate::schema::TICKS;
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let (mut media, mut inspected) = ready_delayed_source(&root);
        // Exercise the preparation boundary after the last sound use has gone.
        // A missing decoder input must be irrelevant to the retained picture.
        let id = MediaId("source".into());
        let MediaInspection::Ready(facts) = inspected.get_mut(&id).unwrap() else {
            panic!("ready source")
        };
        facts.path = root.join("unavailable-audio.m4a");
        let picture = include_bytes!("../tests/fixtures/video-30fps.mp4");
        fs::write(root.join("picture.mp4"), picture).unwrap();
        let source = Arc::make_mut(&mut media).get_mut(&id).unwrap();
        source.video = video_media().remove(&id).unwrap().video;
        source.video.as_mut().unwrap().intrinsic_ticks = TICKS;
        source.relative_path = Some("picture.mp4".into());
        source.relative_paths = vec!["picture.mp4".into()];
        let mut sequence = video_sequence();
        sequence.video_tracks[0].clip_mut(0).end_ticks = TICKS / 5;
        sequence.video_tracks[0].clip_mut(0).out_ticks = TICKS / 5;
        sequence.timeline_end_ticks = TICKS / 5;
        let original_media = Arc::clone(&media);
        let mut omissions = Vec::new();
        assert!(
            delayed_audio::prepare(&mut sequence, &mut media, &mut inspected, &mut omissions)
                .unwrap()
                .is_none()
        );
        assert!(Arc::ptr_eq(&media, &original_media));
        assert_eq!(media.len(), 1);
        assert_eq!(inspected.len(), 1);
        assert!(omissions.is_empty());
        let pending =
            convert_premiere_sequence(&root.join("input.prproj"), sequence, media, &mut omissions)
                .unwrap()
                .unwrap();
        let path = root.join("picture.tsrct");
        pending.write_to_staging(&path).unwrap();
        let archive = tesseract_file::TesseractFile::open(path).unwrap();
        assert_eq!(archive.metadata().assets.len(), 1);
        let doc = archive.project_json().unwrap();
        let layers = doc["composition"]["layers"].as_array().unwrap();
        assert!(!layers.iter().any(|layer| layer["type"] == "Audio"));
        let video = layers
            .iter()
            .find(|layer| layer["type"] == "Video")
            .unwrap();
        assert_eq!(
            archive
                .asset(video["source"]["assetId"].as_str().unwrap())
                .unwrap()
                .read_verified_bytes(picture.len() as u64)
                .unwrap(),
            picture
        );
        assert!(!omissions.iter().any(|o| o.reason.contains("prepared once")));
    }

    #[test]
    #[cfg(feature = "ffmpeg-library")]
    fn nested_delayed_audio_requires_decode_and_shares_one_preparation() {
        use crate::schema::{PrAudioOccurrence, TICKS};
        use crate::tests::support::nest_of;
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let (mut media, mut inspected) = ready_delayed_source(&root);
        let id = MediaId("source".into());
        let mut child = video_sequence();
        child.video_tracks.clear();
        child.audio = (0..2)
            .map(|index| PrAudioOccurrence {
                id: None,
                media: id.clone(),
                source_channel: None,
                preserve_audio_pitch: false,
                playback_rate: 1.0,
                start_ticks: index * TICKS / 5,
                end_ticks: (index + 1) * TICKS / 5,
                in_ticks: 0,
                out_ticks: TICKS / 5,
                volume: fx_schema::LinearGain::UNITY,
                volume_keys: None,
                fade_in: None,
                fade_out: None,
            })
            .collect();
        let mut sequence = video_sequence();
        sequence.video_tracks[0].items.clear();
        sequence.video_tracks[0]
            .nests
            .push(nest_of(child, 0..2 * TICKS / 5, 0));
        // Demand is only two nests deep; neither containing sequence has audio.
        let mut outer = video_sequence();
        outer.video_tracks[0].items.clear();
        outer.video_tracks[0]
            .nests
            .push(nest_of(sequence, 0..2 * TICKS / 5, 0));
        let MediaInspection::Ready(facts) = inspected.get_mut(&id).unwrap() else {
            panic!("ready source")
        };
        let valid_path = facts.path.clone();
        facts.path = root.join("unavailable-audio.m4a");
        let mut omissions = Vec::new();
        assert!(
            delayed_audio::prepare(&mut outer, &mut media, &mut inspected, &mut omissions).is_err()
        );
        assert_eq!(media.len(), 1);
        let MediaInspection::Ready(facts) = inspected.get_mut(&id).unwrap() else {
            panic!("ready source")
        };
        facts.path = valid_path;
        let prepared =
            delayed_audio::prepare(&mut outer, &mut media, &mut inspected, &mut omissions)
                .unwrap()
                .unwrap();
        assert_eq!(fs::read_dir(prepared.path()).unwrap().count(), 1);
        assert_eq!(media.len(), 2);
        assert_eq!(inspected.len(), 2);
        let sounds = &outer.video_tracks[0].nests[0].sequence.video_tracks[0].nests[0]
            .sequence
            .audio;
        assert_eq!(sounds.len(), 2);
        assert_eq!(sounds[0].media, sounds[1].media);
        assert_ne!(sounds[0].media, id);
        assert!(media[&sounds[0].media]
            .audio
            .as_ref()
            .unwrap()
            .prepared_clock
            .is_some());
        assert_eq!(
            omissions
                .iter()
                .filter(|o| o.reason.contains("prepared once"))
                .count(),
            1
        );
    }

    #[test]
    fn video_removed_after_resolution_keeps_its_typed_missing_media_error() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let source = root.join("source.mp4");
        fs::write(&source, b"original source bytes").unwrap();
        let media = video_media();
        let (id, native) = media.first_key_value().unwrap();
        let id = id.clone();
        let mut native = native.clone();
        native.relative_path = Some("source.mp4".into());
        native.relative_paths = vec!["source.mp4".into()];
        let mut digests = HashMap::new();
        resolve_media(&root, &native, &mut digests).unwrap();
        fs::remove_file(&source).unwrap();
        let inspected = inspect_media(
            &root,
            &id,
            &native,
            MediaSelection {
                usage: MediaUse::Whole,
                admission: VideoAdmission::Strict,
            },
            &mut digests,
            &mut LinkedCompositions::new(None),
            None,
        );
        assert!(matches!(inspected, Err(BuildError::MissingMedia(_))));
    }

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
