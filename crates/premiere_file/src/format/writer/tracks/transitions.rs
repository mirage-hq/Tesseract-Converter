//! Native Cross Dissolve New records and the clips' head/tail references.
use crate::{
    format::{invalid, writer::graph::SequenceGraphIds, Result},
    schema::{native::*, records, PrSequence, PrVideoItem},
};

pub(in crate::format::writer) fn attach(
    sequence: &PrSequence,
    ids: &SequenceGraphIds,
    records_out: &mut Vec<Record>,
) -> Result<()> {
    for (index, track) in sequence.video_tracks.iter().enumerate() {
        for (transition, id) in track.transitions.iter().zip(&ids.video_transitions[index]) {
            if transition.start_ticks < 0
                || transition.start_ticks >= transition.end_ticks
                || !(transition.start_ticks..=transition.end_ticks).contains(&transition.cut_ticks)
                || (transition.outgoing_clip.is_none() && transition.incoming_clip.is_none())
            {
                return Err(invalid("invalid native Cross Dissolve range or links"));
            }
            for (tail, link) in [
                (true, &transition.outgoing_clip),
                (false, &transition.incoming_clip),
            ] {
                let Some(link) = link else { continue };
                let matches: Vec<_> = track
                    .items
                    .iter()
                    .zip(&ids.placements[index])
                    .filter_map(|(item, ids)| {
                        let PrVideoItem::Media(clip) = item else {
                            return None;
                        };
                        (clip.id.as_ref() == Some(link)).then_some((clip, ids.track_item()))
                    })
                    .collect();
                let [(clip, native_id)] = matches.as_slice() else {
                    return Err(invalid(
                        "Cross Dissolve needs one retained physical picture per link",
                    ));
                };
                if (tail
                    && (clip.end_ticks != transition.cut_ticks
                        || clip.start_ticks > transition.start_ticks))
                    || (!tail
                        && (clip.start_ticks != transition.cut_ticks
                            || clip.end_ticks < transition.end_ticks))
                {
                    return Err(invalid(
                        "Cross Dissolve pictures do not meet at the saved cut",
                    ));
                }
                let native_id = native_id.as_native_string();
                let item = records_out
                    .iter_mut()
                    .find_map(|record| match record {
                        Record::VideoClipTrackItem(item)
                            if item.object_id.as_deref() == Some(&native_id) =>
                        {
                            Some(item)
                        }
                        _ => None,
                    })
                    .and_then(|item| item.clip_track_item.as_mut())
                    .ok_or_else(|| invalid("Cross Dissolve picture record is absent"))?;
                let slot = if tail {
                    &mut item.tail_transition
                } else {
                    &mut item.head_transition
                };
                if slot.is_some() {
                    return Err(invalid(
                        "more than one Cross Dissolve on a picture boundary",
                    ));
                }
                *slot = Some(Reference::object(*id));
            }
            records_out.push(Record::VideoTransitionTrackItem(VideoTransitionTrackItem {
                object_id: Some(id.as_native_string()),
                class_id: Some(records::VIDEO_TRANSITION_TRACK_ITEM.class_id.into()),
                version: Some("6".into()),
                transition_track_item: Some(TransitionTrackItem {
                    version: Some("4".into()),
                    track_item: Some(TrackItemRange {
                        version: Some("4".into()),
                        _node: None,
                        _item_type: None,
                        _media_type: None,
                        _track_index: None,
                        _track_ref_count: None,
                        start: (transition.start_ticks != 0)
                            .then(|| transition.start_ticks.to_string()),
                        end: transition.end_ticks.to_string(),
                    }),
                    has_outgoing_clip: Some(transition.outgoing_clip.is_some().to_string()),
                    has_incoming_clip: Some(transition.incoming_clip.is_some().to_string()),
                    display_name: Some("Cross Dissolve".into()),
                    match_name: Some("AE.ADBE Cross Dissolve New".into()),
                    alignment: Some((transition.cut_ticks - transition.start_ticks).to_string()),
                }),
                video_filter_component: None,
                start_percent: None,
                end_percent: None,
                switch_sources: None,
                reverse: None,
            }));
        }
        for (nest, nest_ids) in track.nests.iter().zip(&ids.nests[index]) {
            attach(&nest.sequence, &nest_ids.inner, records_out)?;
        }
    }
    Ok(())
}
