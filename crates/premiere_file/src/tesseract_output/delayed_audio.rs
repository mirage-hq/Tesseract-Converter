//! Full-source PCM preparation, separate from editable presentation timing.
use super::{InspectedMedia, MediaInspection};
use crate::{
    audio_media::inspect_audio_media,
    error::{ensure, unsupported, Result},
    media::MediaContainer,
    schema::{MediaId, PrMedia, PrSequence, TICKS},
    Omission,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    sync::Arc,
};

pub(super) fn prepare(
    sequence: &mut PrSequence,
    media: &mut Arc<BTreeMap<MediaId, PrMedia>>,
    inspected: &mut BTreeMap<MediaId, MediaInspection>,
    omissions: &mut Vec<Omission>,
) -> Result<Option<tempfile::TempDir>> {
    let mut referenced_audio = BTreeSet::new();
    collect_audio_sources(sequence, &mut referenced_audio);
    let delayed: Vec<_> = inspected
        .iter()
        .filter(|(id, _)| referenced_audio.contains(*id))
        .filter_map(|(id, value)| match value {
            MediaInspection::Ready(source) => source
                .delayed_audio
                .clone()
                .map(|clock| (id.clone(), clock)),
            _ => None,
        })
        .collect();
    if delayed.is_empty() {
        return Ok(None);
    }
    let directory = tempfile::tempdir()?;
    let root = directory.path().canonicalize()?;
    let media = Arc::make_mut(media);
    let mut replacements = BTreeMap::new();
    for (ordinal, (id, clock)) in delayed.into_iter().enumerate() {
        let MediaInspection::Ready(original) = &inspected[&id] else {
            return Err(unsupported("delayed audio inspection disappeared"));
        };
        let mut prepared = media[&id].clone();
        let native = prepared
            .audio
            .as_ref()
            .ok_or_else(|| unsupported("delayed audio stream missing"))?;
        let path = root.join(format!("raw-audio-{ordinal}.wav"));
        media_transcode::prepare_raw_movie_audio(
            &original.path,
            &path,
            media_transcode::RawMovieAudio {
                sample_rate: native.sample_rate,
                channels: u16::try_from(native.channels.count())
                    .map_err(|_| unsupported("audio channels overflow"))?,
                samples: u64::try_from(clock.raw_ticks() / (TICKS / i64::from(native.sample_rate)))
                    .map_err(|_| unsupported("raw sample count overflows"))?,
            },
        )?;
        let file = File::open(&path)?;
        let size = file.metadata()?.len();
        let mut raw = inspect_audio_media(file, size, "wav")?
            .ok_or_else(|| unsupported("prepared audio stream is missing"))?;
        ensure!(
            raw.intrinsic_ticks == clock.raw_ticks()
                && raw.sample_rate == native.sample_rate
                && raw.channels == native.channels,
            "prepared raw audio facts differ from inspected source"
        );
        let presentation_note = clock.presentation_note();
        raw.prepared_clock = Some(*clock);
        prepared.audio = Some(raw);
        prepared.video = None;
        let prepared_id = MediaId(format!("raw-audio-{}", id.as_str()));
        ensure!(
            !media.contains_key(&prepared_id),
            "prepared audio media identity collision"
        );
        let facts = InspectedMedia {
            delayed_audio: None,
            audio_omission: None,
            picture_clock: None,
            numbered_frames: Vec::new(),
            hash: crate::hash::hash(&path)?,
            path,
            native_path: original.native_path.clone(),
            native_hash: original.native_hash.clone(),
            candidate_paths: original.candidate_paths.clone(),
            container: MediaContainer::Wav,
            note: None,
            colour: None,
            codec: None,
        };
        crate::approximate(omissions, format!("audio source {}", id.as_str()),
            format!("empty-leading-edit AAC prepared once as zero-origin full-source PCM with movie edits disabled; original picture bytes unchanged; delay/tail trim and gain remain editable, native presentation endpoint admitted only within matching millisecond rounding; {presentation_note}; audio fidelity unmeasured"));
        replacements.insert(id, prepared_id.clone());
        media.insert(prepared_id.clone(), prepared);
        inspected.insert(prepared_id, MediaInspection::Ready(facts));
    }
    replace(sequence, &replacements);
    Ok(Some(directory))
}

fn collect_audio_sources(sequence: &PrSequence, sources: &mut BTreeSet<MediaId>) {
    sources.extend(sequence.audio.iter().map(|clip| clip.media.clone()));
    for track in &sequence.video_tracks {
        for nest in &track.nests {
            collect_audio_sources(&nest.sequence, sources);
        }
    }
}

fn replace(sequence: &mut PrSequence, replacements: &BTreeMap<MediaId, MediaId>) {
    for clip in &mut sequence.audio {
        if let Some(id) = replacements.get(&clip.media) {
            clip.media = id.clone();
        }
    }
    for track in &mut sequence.video_tracks {
        for nest in &mut track.nests {
            replace(&mut nest.sequence, replacements);
        }
    }
}
