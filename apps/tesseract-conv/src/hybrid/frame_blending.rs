//! Bounded FX sampling policy for root videos in linked Premiere pictures.
mod intervals;

use fx_conv::{Diagnostic, DiagnosticKind};
use fx_schema::{
    layer::FrameBlendingData, EditableFxCompositionDocument, Layer, LayerData, LayerId,
    LayerPlaybackMapping, PropertyKeyframeEasing,
};
use std::collections::BTreeMap;

pub(super) struct Prepared {
    pub document: EditableFxCompositionDocument,
    pub diagnostics: Vec<Diagnostic>,
    pub owners: BTreeMap<LayerId, LayerId>,
}

/// Nested videos remain untouched: their effective speed includes Group clocks.
pub(super) fn prepare(
    document: &EditableFxCompositionDocument,
    selected: std::ops::Range<usize>,
) -> anyhow::Result<Prepared> {
    let mut layers = document.composition().layers().to_vec();
    let mut diagnostics = Vec::new();
    let mut owners = BTreeMap::new();
    for layer in &mut layers[selected] {
        let LayerData::Video(video) = layer.data() else {
            continue;
        };
        if !matches!(video.frame_blending, Some(FrameBlendingData::Boolean(true))) {
            continue;
        }
        if let Some(split) = intervals::split(document, layer, &mut owners)? {
            diagnostics.push(Diagnostic {
                code: "HYBRID-FRAME-BLENDING",
                kind: DiagnosticKind::Warning,
                context: Some(format!("FX layer {} ({:?})", video.id, video.name)),
                message: "Isolated mixed-speed Video uses non-overlapping editable occurrences: FrameMix is enabled only on nonstationary slow intervals; source remap values and key times are retained.".into(),
            });
            *layer = split;
            continue;
        }
        if !has_non_slow_playback(video.playback.mapping()) {
            continue;
        }
        diagnostics.push(Diagnostic {
            code: "HYBRID-FRAME-BLENDING",
            kind: DiagnosticKind::Warning,
            context: Some(format!("FX layer {} ({:?})", video.id, video.name)),
            message: "Boolean frame blending disabled on a linked root video with unit, fast or held playback: FX synthesizes only at nonzero speed below 1x, while AE FrameMix blends the whole layer. Mixed clocks retain editable playback but approximate slow legs with nearest samples; interval-specific blending is not exported.".into(),
        });
        // Round-trip the stored layer, not only its typed projection, so unknown
        // optional records and all editable controls remain intact.
        let mut value = serde_json::to_value(&*layer)?;
        value["frameBlending"] = serde_json::Value::Bool(false);
        *layer = serde_json::from_value::<Layer>(value)?;
    }
    if diagnostics.is_empty() {
        return Ok(Prepared {
            document: document.clone(),
            diagnostics,
            owners,
        });
    }
    let mut result = document.clone();
    let dynamics = document.composition().dynamics().clone();
    result
        .composition_mut()
        .try_replace_parts(dynamics, layers)?;
    Ok(Prepared {
        document: result,
        diagnostics,
        owners,
    })
}

fn has_non_slow_playback(mapping: &LayerPlaybackMapping) -> bool {
    match mapping {
        LayerPlaybackMapping::Linear { input, output } => {
            output.duration.is_zero() || output.duration >= input.duration
        }
        LayerPlaybackMapping::TimeRemap { property } => {
            let keys = property.keyframes();
            // Curved clocks need a derivative envelope; do not guess from their
            // endpoint speeds. Explicit modes also retain their existing mapping.
            keys.len() >= 2
                && keys
                    .iter()
                    .all(|key| matches!(key.easing, PropertyKeyframeEasing::Linear))
                && keys.windows(2).any(|pair| {
                    let elapsed = pair[1].time.as_millis() - pair[0].time.as_millis();
                    let distance = pair[1]
                        .value
                        .as_millis()
                        .abs_diff(pair[0].value.as_millis());
                    distance == 0 || distance >= elapsed
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn document(values: &[u64], blending: Value) -> EditableFxCompositionDocument {
        let mut value: Value = serde_json::from_str(include_str!(
            "../../../../crates/premiere_file/tests/fixtures/editable-video.json"
        ))
        .unwrap();
        // This publishable document supplies the normal video structure. Only
        // the sampling policy and editable clock are changed for this control.
        let video = value["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| layer["type"] == "Video")
            .unwrap();
        video["frameBlending"] = blending;
        video["sourceRange"] = json!({"start":0,"duration":8000});
        video["sourceIntrinsicDuration"] = json!(8000);
        video["playback"] = json!({"type":"windowed", "inputRange":{"start":0,"duration":2000}, "inputOffsetMs":0,
            "mapping":{"type":"timeRemap", "property":{"before":"inactive", "after":"inactive", "keyframes":
            values.iter().enumerate().map(|(i, value)| json!({
                "id":format!("blend-{i}"), "time":i * 1000, "value":value,
                "easing":{"type":"linear"}
            })).collect::<Vec<_>>()}}});
        EditableFxCompositionDocument::from_json_value(value).unwrap()
    }

    #[test]
    fn linked_boolean_blending_does_not_ghost_unit_or_mixed_clocks() {
        for values in [
            &[0, 1000, 1500][..],
            &[2000, 1000, 0],
            &[0, 2000, 4000],
            &[0, 0, 0],
        ] {
            let original = document(values, json!(true));
            let Prepared {
                document: prepared,
                diagnostics,
                ..
            } = prepare(&original, 0..original.composition().layers().len()).unwrap();
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(diagnostics[0].code, "HYBRID-FRAME-BLENDING");
            let mut expected = serde_json::to_value(&original).unwrap();
            let video = expected["composition"]["layers"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|layer| layer["type"] == "Video")
                .unwrap();
            video["frameBlending"] = json!(false);
            assert_eq!(
                serde_json::to_value(prepared.composition().layers()).unwrap(),
                expected["composition"]["layers"]
            );
            assert_eq!(
                serde_json::to_value(prepared.composition().dynamics()).unwrap(),
                serde_json::to_value(original.composition().dynamics()).unwrap()
            );
        }
    }

    #[test]
    fn isolated_reverse_and_hold_spans_preserve_offset_and_remap() {
        for hold in [false, true] {
            let original = document(&[2000, 1000, 500], json!(true));
            let mut value = original.to_json_value().unwrap();
            value["composition"]["layers"]
                .as_array_mut()
                .unwrap()
                .retain(|layer| layer["type"] == "Video");
            let video = &mut value["composition"]["layers"][0];
            video["playback"]["inputRange"] = json!({"start":100,"duration":1000});
            video["playback"]["inputOffsetMs"] = json!(400);
            if hold {
                video["playback"]["mapping"]["property"]["keyframes"][1]["easing"] =
                    json!({"type":"hold"});
            }
            let original = EditableFxCompositionDocument::from_json_value(value).unwrap();
            let prepared = prepare(&original, 0..1).unwrap();
            let value = prepared.document.to_json_value().unwrap();
            let children = value["composition"]["layers"][0]["layers"]
                .as_array()
                .unwrap();
            assert_eq!(children.len(), 2);
            for (child, (start, slow)) in children.iter().zip([(100, false), (600, true)]) {
                assert_eq!(
                    child["playback"]["inputRange"],
                    json!({"start":start,"duration":500})
                );
                assert_eq!(child["playback"]["inputOffsetMs"], json!(400));
                assert_eq!(child["frameBlending"], json!(slow));
                assert_eq!(
                    child["playback"]["mapping"],
                    original.to_json_value().unwrap()["composition"]["layers"][0]["playback"]
                        ["mapping"]
                );
            }
            assert_eq!(prepared.owners.len(), 2);
        }
    }

    #[test]
    fn linked_uniform_slow_and_explicit_modes_keep_blending() {
        for (values, blending) in [
            (&[0, 500, 1000][..], json!(true)),
            (&[0, 1000, 2000], json!("simple")),
            (&[0, 1000, 2000], json!(false)),
        ] {
            let original = document(values, blending);
            let Prepared {
                document: prepared,
                diagnostics,
                ..
            } = prepare(&original, 0..original.composition().layers().len()).unwrap();
            assert!(diagnostics.is_empty());
            assert_eq!(prepared, original);
        }
    }
}
