//! Recover saved coverage into ordinary numbered PNG assets. The native
//! sidecar remains the hash-bound source; temporary PNGs live through publication.
use super::{InspectedMedia, MediaInspection, NumberedFrame};
use crate::{
    approximate,
    error::{ensure, unsupported, BuildError, Result},
    format::object_mask::Tracker,
    hash::hash,
    image_media::ImageFormat,
    media::MediaContainer,
    omit,
    schema::{
        records::MediaPathField, MediaId, PrMedia, PrMediaKind, PrSequence, PrVideoItem,
        PrVideoOccurrence, RasterMask, SourceInterpretation, VideoOrientation,
    },
    Omission, OmissionScope,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub(super) fn prepare(
    source: &Path,
    sequence: &mut PrSequence,
    media: &mut Arc<BTreeMap<MediaId, PrMedia>>,
    inspected: &mut BTreeMap<MediaId, MediaInspection>,
    omissions: &mut Vec<Omission>,
) -> Result<tempfile::TempDir> {
    let directory = tempfile::tempdir()?;
    // Observed in the supplied native package only: XML has no mask-directory
    // locator. Do not scan other folders or choose an unreferenced UUID.
    let mut sidecar_name = source
        .file_stem()
        .ok_or_else(|| unsupported("Object Mask project name is missing"))?
        .to_os_string();
    sidecar_name.push(" Masks");
    let sidecars = source.with_file_name(sidecar_name);
    let media = Arc::make_mut(media);
    for track in &mut sequence.video_tracks {
        let mut kept = Vec::with_capacity(track.items.len());
        for mut item in std::mem::take(&mut track.items) {
            let PrVideoItem::Media(clip) = &mut item else {
                // Graphic masks are not physical source-frame rasters.
                kept.push(item);
                continue;
            };
            // Derived recovery follows the same Ready gate as derived audio.
            // Omitted media must not spend decode/PNG/storage work first.
            if !matches!(inspected.get(&clip.media), Some(MediaInspection::Ready(_))) {
                kept.push(item);
                continue;
            }
            let Some(RasterMask::Saved(tracker)) = clip
                .opacity_mask
                .as_ref()
                .and_then(|mask| mask.raster.as_ref())
            else {
                kept.push(item);
                continue;
            };
            let result = recover(clip, tracker, &sidecars, directory.path(), media);
            match result {
                Ok((id, native, resolved)) => {
                    media.insert(id.clone(), native);
                    inspected.insert(id.clone(), MediaInspection::Ready(resolved));
                    if let Some(mask) = &mut clip.opacity_mask {
                        mask.raster = Some(RasterMask::Prepared(id));
                    }
                    approximate(omissions, clip.record(), "Object Mask saved selection/propagation is recovered as an ordinary editable supplied raster matte sequence; coherent timing/source edits must update the stage and its matte together; slipping or retiming only the video child does not update coverage; placement and opacity remain editable, but the AI controller/User Interactions and AI re-propagation are not retained; source picture is unchanged; native raster cadence and millisecond/PTS picture sampling can select adjacent source frames differently (observed at 30fps samples 2/193/194 for the supplied native source); this residual is not a fidelity pass");
                    kept.push(item);
                }
                Err(BuildError::Unsupported(reason)) => {
                    omit(omissions, OmissionScope::Occurrence, clip.record(), format!("Object Mask could not recover its referenced supplied matte: {reason}; owning masked occurrence is omitted, never shown unmasked"));
                }
                Err(error) => return Err(error),
            }
        }
        track.items = kept;
        // Sampling under containing clocks remains outside numbered-images'
        // current admission. Remove only the masked inner occurrence safely.
        for nest in &mut track.nests {
            omit_nested(&mut nest.sequence, omissions);
        }
    }
    Ok(directory)
}

fn omit_nested(sequence: &mut PrSequence, omissions: &mut Vec<Omission>) {
    for track in &mut sequence.video_tracks {
        track.items.retain(|item| {
            let PrVideoItem::Media(clip) = item else { return true };
            if clip.opacity_mask.as_ref().is_some_and(|mask| mask.raster.is_some()) {
                omit(omissions, OmissionScope::Occurrence, clip.record(), "Object Mask under a containing sequence clock is not yet supported by numbered-image sampling; masked occurrence omitted");
                false
            } else { true }
        });
        for nest in &mut track.nests {
            omit_nested(&mut nest.sequence, omissions);
        }
    }
}

fn recover(
    clip: &PrVideoOccurrence,
    tracker: &Tracker,
    sidecars: &Path,
    directory: &Path,
    media: &BTreeMap<MediaId, PrMedia>,
) -> Result<(MediaId, PrMedia, InspectedMedia)> {
    let original = media
        .get(&clip.media)
        .ok_or_else(|| unsupported("Object Mask source is missing"))?;
    let stream = original
        .video
        .as_ref()
        .ok_or_else(|| unsupported("Object Mask source has no picture stream"))?;
    ensure!(
        matches!(stream.kind, PrMediaKind::Video { .. })
            && stream.orientation == VideoOrientation::Identity
            && stream.interpretation == SourceInterpretation::Original,
        "Object Mask requires an unrotated, uninterpreted physical video source"
    );
    ensure!(clip.playback_rate == 1.0 && clip.time_remap.is_none() && clip.frame_blending.is_none()
        && clip.crop.is_default() && clip.linear_wipe.is_none() && clip.track_matte.is_none()
        && clip.active_transforms == 0
        && clip.source_effects.as_ref().is_none_or(|effects| effects.active_transforms == 0)
        && clip.stroke.is_none(),
        "Object Mask saved coverage requires unit forward playback without additional masks, Transform/time owners or frame blending");
    tracker.validate_source(stream.frame_rate, stream.intrinsic_ticks)?;
    let referenced = tracker.propagation_path(sidecars);
    let native_path = referenced.canonicalize().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            unsupported(format!(
                "referenced Object Mask sidecar {} is missing",
                referenced.display()
            ))
        } else {
            BuildError::IoAt {
                context: format!(
                    "cannot resolve Object Mask sidecar {}",
                    referenced.display()
                ),
                source: error,
            }
        }
    })?;
    let mut file = std::fs::File::open(&native_path)?;
    let size = usize::try_from(file.metadata()?.len())
        .map_err(|_| unsupported("Object Mask sidecar length exceeds address space"))?;
    let mut bytes = crate::format::object_mask::zeroed(size)?;
    std::io::Read::read_exact(&mut file, &mut bytes)?;
    let native_hash = format!("{:x}", Sha256::digest(&bytes));
    let raster = tracker.propagation(&bytes, [stream.width, stream.height])?;
    ensure!(
        hash(&native_path)? == native_hash,
        "Object Mask sidecar changed while reading"
    );
    let id = MediaId(format!(
        "object-mask-{}-{}",
        tracker.propagation,
        media.len()
    ));
    ensure!(
        !media.contains_key(&id),
        "Object Mask media identity collision"
    );
    let mut numbered_frames = Vec::with_capacity(raster.frames.len());
    for (index, frame) in raster.frames.iter().enumerate() {
        let coverage = frame.coverage()?;
        let mut pixels = crate::format::object_mask::zeroed(coverage.len() * 4)?;
        for (pixel, alpha) in pixels.chunks_exact_mut(4).zip(coverage) {
            pixel.copy_from_slice(&[255, 255, 255, alpha]);
        }
        let path = directory.join(format!("{}-{index:06}.png", id.as_str()));
        image::save_buffer_with_format(
            &path,
            &pixels,
            stream.width,
            stream.height,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .map_err(|error| match error {
            image::ImageError::IoError(error) => BuildError::Io(error),
            error => unsupported(format!(
                "cannot encode recovered Object Mask frame: {error}"
            )),
        })?;
        numbered_frames.push(NumberedFrame {
            hash: hash(&path)?,
            candidates: vec![path.clone()],
            path,
            container: MediaContainer::Image(ImageFormat::Png),
        });
    }
    let first = numbered_frames
        .first()
        .ok_or_else(|| unsupported("Object Mask has no recovered frames"))?;
    let inspected = InspectedMedia {
        delayed_audio: None,
        audio_omission: None,
        path: first.path.clone(),
        hash: first.hash.clone(),
        numbered_frames,
        native_path: native_path.clone(),
        native_hash,
        candidate_paths: vec![referenced.clone()],
        container: MediaContainer::Image(ImageFormat::Png),
        picture_clock: None,
        note: None,
        colour: None,
        codec: None,
    };
    let mut matte_stream = stream.clone();
    matte_stream.kind = PrMediaKind::NumberedStills { alpha: true };
    let native = PrMedia {
        name: format!("Object Mask {} supplied coverage", tracker.propagation),
        relative_path: None,
        relative_paths: Vec::new(),
        absolute_paths: vec![(MediaPathField::FilePath, referenced)],
        video: Some(matte_stream),
        audio: None,
    };
    Ok((id, native, inspected))
}
