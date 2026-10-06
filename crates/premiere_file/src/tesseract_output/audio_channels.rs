//! Package native mono selections as full-source mono WAV assets.
//! PCM copying or compressed decoding leaves placement, trimming and gain editable.
use super::{InspectedMedia, MediaInspection};
use crate::{
    error::{ensure, unsupported, Result},
    media::{unsupported_media_reason, MediaContainer},
    omit,
    schema::{records, AudioChannels, MediaId, PrAudioSourceChannel, PrMedia, PrSequence},
    Omission, OmissionScope,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

type Selection = (MediaId, usize);

fn selections(sequence: &PrSequence, selected: &mut BTreeSet<Selection>) {
    for clip in &sequence.audio {
        if let Some(selection) = &clip.source_channel {
            selected.insert((clip.media.clone(), selection.channel()));
        }
    }
    for nest in sequence.nest_occurrences() {
        selections(&nest.sequence, selected);
    }
}

pub(super) fn prepare(
    sequence: &mut PrSequence,
    media: &mut Arc<BTreeMap<MediaId, PrMedia>>,
    inspected: &mut BTreeMap<MediaId, MediaInspection>,
    omissions: &mut Vec<Omission>,
) -> Result<Option<tempfile::TempDir>> {
    let mut selected = BTreeSet::new();
    selections(sequence, &mut selected);
    if selected.is_empty() {
        return Ok(None);
    }
    let media = Arc::make_mut(media);
    let directory = tempfile::tempdir()?;
    let mut prepared = BTreeMap::new();
    for (ordinal, (id, channel)) in selected.into_iter().enumerate() {
        let original = match &inspected[&id] {
            MediaInspection::Ready(original) => original,
            MediaInspection::Omitted(reason) | MediaInspection::UnavailableVideo(reason) => {
                prepared.insert((id, channel), Err(reason.clone()));
                continue;
            }
            MediaInspection::Synthetic | MediaInspection::Linked { .. } => {
                prepared.insert(
                    (id, channel),
                    Err("source-channel selection requires physical audio media".into()),
                );
                continue;
            }
        };
        let mut mono_media = media[&id].clone();
        let stream = mono_media
            .audio
            .as_mut()
            .ok_or_else(|| unsupported("selected audio source has no stream"))?;
        let path = directory
            .path()
            .join(format!("audio-{ordinal}-channel-{}.wav", channel + 1));
        if let Err(error) =
            crate::audio_media::copy_source_channel(&original.path, &path, channel, stream)
        {
            prepared.insert((id, channel), Err(unsupported_media_reason(error)?));
            continue;
        }
        stream.channels = AudioChannels::Mono;
        mono_media.video = None;
        mono_media.name = format!("{} channel {}", mono_media.name, channel + 1);
        let mono_id = MediaId(format!("channel-{channel}-{}", id.as_str()));
        ensure!(
            !media.contains_key(&mono_id),
            "selected-channel media identity collision"
        );
        let mono = InspectedMedia {
            delayed_audio: None,
            audio_omission: None,
            hash: crate::hash::hash(&path)?,
            path,
            container: MediaContainer::Wav,
            codec: None,
            colour: None,
            picture_clock: None,
            numbered_frames: Vec::new(),
            note: original.note,
            native_path: original.native_path.clone(),
            native_hash: original.native_hash.clone(),
            candidate_paths: original.candidate_paths.clone(),
        };
        media.insert(mono_id.clone(), mono_media);
        inspected.insert(mono_id.clone(), MediaInspection::Ready(mono));
        prepared.insert((id, channel), Ok(mono_id));
    }
    replace(sequence, &prepared, omissions);
    Ok(Some(directory))
}

fn replace(
    sequence: &mut PrSequence,
    prepared: &BTreeMap<Selection, std::result::Result<MediaId, String>>,
    omissions: &mut Vec<Omission>,
) {
    sequence.audio.retain_mut(|clip| {
        let Some(selection) = &clip.source_channel else {
            return true;
        };
        match &prepared[&(clip.media.clone(), selection.channel())] {
            Ok(media) => {
                clip.media = media.clone();
                clip.source_channel = None;
                true
            }
            Err(reason) => match selection {
                PrAudioSourceChannel::FillRight(record) => {
                    omit(
                        omissions,
                        OmissionScope::Feature,
                        record,
                        format!("audio filter {:?} not converted: {reason}; stereo source plays unchanged", records::FILL_RIGHT_MATCH_NAME),
                    );
                    clip.source_channel = None;
                    true
                }
                PrAudioSourceChannel::Mono(_) => {
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        clip.record(),
                        reason.clone(),
                    );
                    false
                }
            }
        }
    });
    for track in &mut sequence.video_tracks {
        for nest in &mut track.nests {
            replace(&mut nest.sequence, prepared, omissions);
        }
    }
}
