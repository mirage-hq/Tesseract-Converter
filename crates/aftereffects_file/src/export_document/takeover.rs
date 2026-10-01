//! Bounded native lowering for root half-canvas image takeovers.
//!
//! This module deliberately owns only the graph rewrite. The caller remains
//! responsible for reserving both synthetic identities in the document-wide
//! namespace before publishing the returned layers.

use std::collections::BTreeSet;

use fx_schema::{
    Dimensions, ImageSource, Layer, LayerData, LayerId, MediaFit, MediaPlacement, PositiveRect,
    RectBounds, animator::AnimationGraphEntry,
};

use crate::{
    schema::CompositionRecord,
    timing::Duration24,
    writer::{
        AepWriteError, CompositionOptions, KeyframeEasing, LayerSpec, LayerTiming,
        NativeLayerOptions, NullLayerSpec, NumericKeyframe, NumericTrack, PrecompositionSpec,
        SolidTransform, TransformAnimations, footage::FootageClock,
    },
};

use super::{ExportDiagnostic, media};

const SLIDE_MILLIS: i64 = 120;
const PAN_MIN_MILLIS: i64 = 1_000;
const PAN_EASING: KeyframeEasing = KeyframeEasing::CubicBezier {
    x1: 0.30,
    y1: 0.40,
    x2: 0.70,
    y2: 0.60,
};

/// Caller-provided identities for the two generated native occurrences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TakeoverIds {
    pub inner_footage: LayerId,
    pub slide_parent: LayerId,
}

/// Reference facts which cannot be established from one isolated layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct TakeoverGraphFacts {
    pub has_source_variants: bool,
    pub has_external_references: bool,
    pub has_unsupported_graph_edges: bool,
}

/// Complete takeover rewrite in AE timeline order.
pub(super) struct TakeoverPlan {
    pub layers: Vec<LayerSpec>,
    pub required_synthetic_ids: [LayerId; 2],
    pub diagnostics: Vec<ExportDiagnostic>,
}

/// Builds the bounded asset-image half-canvas takeover graph.
///
/// `composition_end_millis` is the already-established exact FX endpoint; it
/// is supplied separately from the native duration so this helper never
/// guesses a millisecond endpoint from the 24-fps record timebase.
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_image_takeover(
    layer: &Layer,
    source: &media::ResolvedMediaSource,
    dynamics: &[AnimationGraphEntry],
    canvas: Dimensions,
    duration: Duration24,
    composition_end_millis: i64,
    composition_options: CompositionOptions,
    mut root_options: NativeLayerOptions,
    ids: TakeoverIds,
    occupied_ids: &BTreeSet<LayerId>,
    graph: TakeoverGraphFacts,
) -> Result<TakeoverPlan, AepWriteError> {
    let image = match layer.data() {
        LayerData::Image(image) => image,
        LayerData::Video(_) => {
            return Err(invalid(
                "Video takeover tail is not implemented: exact native end-sample/Time-Remap integration is unavailable",
            ));
        }
        _ => return Err(invalid("half-canvas takeover requires an Image layer")),
    };
    let placement = image
        .placement
        .ok_or_else(|| invalid("image does not request a half-canvas takeover placement"))?;
    let ImageSource::Asset(asset) = &image.source;
    if asset.fit != MediaFit::Cover {
        return Err(invalid("half-canvas takeover requires Image Cover fit"));
    }
    if image.parent.is_some() || root_options.parent.is_some() {
        return Err(invalid(
            "half-canvas takeover requires an unparented root occurrence",
        ));
    }
    if image.track_matte.is_some() || root_options.matte.is_some() {
        return Err(invalid(
            "half-canvas takeover does not support matte ownership or references",
        ));
    }
    if !image.masks.is_empty() || !root_options.masks.is_empty() {
        return Err(invalid(
            "half-canvas takeover does not support authored masks",
        ));
    }
    if root_options.transform_3d.is_some() {
        return Err(invalid(
            "half-canvas takeover does not support a 3D replacement",
        ));
    }
    if graph.has_source_variants {
        return Err(invalid(
            "half-canvas takeover cannot remap source variants atomically",
        ));
    }
    if graph.has_external_references {
        return Err(invalid(
            "half-canvas takeover has external identity references",
        ));
    }
    if graph.has_unsupported_graph_edges {
        return Err(invalid(
            "half-canvas takeover contains unsupported graph edges",
        ));
    }
    if root_options.fx_id != image.id {
        return Err(invalid(
            "centralized root options do not belong to the takeover image",
        ));
    }
    check_synthetic_ids(image.id, ids, occupied_ids)?;

    let width = u16::try_from(canvas.width).map_err(|_| invalid("canvas width exceeds u16"))?;
    let full_height =
        u16::try_from(canvas.height).map_err(|_| invalid("canvas height exceeds u16"))?;
    if width == 0 || full_height == 0 || full_height % 2 != 0 {
        return Err(invalid(
            "half-canvas takeover requires positive dimensions and an even height",
        ));
    }
    let viewport_height = full_height / 2;

    let start = i64::try_from(image.active_range.start.as_millis())
        .map_err(|_| invalid("takeover start exceeds i64 milliseconds"))?;
    let end = i64::try_from(image.active_range.end().as_millis())
        .map_err(|_| invalid("takeover end exceeds i64 milliseconds"))?;
    let visible_millis = end
        .checked_sub(start)
        .ok_or_else(|| invalid("takeover range is reversed"))?;
    if visible_millis < SLIDE_MILLIS {
        return Err(invalid(
            "half-canvas takeover window is shorter than 120 ms",
        ));
    }
    let tail_end = end
        .checked_add(SLIDE_MILLIS)
        .ok_or_else(|| invalid("takeover post-range tail overflows"))?;
    if start < 0 || tail_end > composition_end_millis {
        return Err(invalid(
            "takeover visibility including its 120 ms tail is outside the composition",
        ));
    }

    let mut lowered_image = image.clone();
    lowered_image.placement = None;
    let ImageSource::Asset(lowered_asset) = &mut lowered_image.source;
    lowered_asset.frame_rect = PositiveRect::new(RectBounds {
        x: 0.0,
        y: 0.0,
        width: f64::from(width),
        height: f64::from(viewport_height),
    });
    let lowered_layer = Layer::from_data(&LayerData::Image(lowered_image))
        .map_err(|_| invalid("placement-cleared takeover image could not be materialized"))?;
    let mut footage = media::lower(&lowered_layer, source, canvas).map_err(invalid)?;

    // The ordinary media lowerer puts the authored Transform on the footage.
    // Move that Transform to the clipped precomposition occurrence: runtime
    // clips first, applies the authored Transform second, then applies slide.
    let mut authored_transform = footage.transform.transform.clone();
    let authored_animations =
        super::solid_transform_animations(dynamics, image.id, &authored_transform, false)
            .map_err(invalid)?;
    let viewport_y = match placement {
        MediaPlacement::TopHalf => 0.0,
        MediaPlacement::BottomHalf => f64::from(viewport_height),
    };
    authored_transform.anchor[1] -= viewport_y;

    footage.transform.transform = identity_transform();
    footage.clock = FootageClock::Still {
        start_millis: u64::try_from(start).map_err(|_| invalid("takeover start is negative"))?,
        duration_millis: u64::try_from(tail_end - start)
            .map_err(|_| invalid("takeover duration exceeds u64"))?,
    };

    let pan = content_pan(
        &footage,
        [f64::from(width), f64::from(viewport_height)],
        start,
        end,
    );
    let inner_animations = TransformAnimations {
        position: pan.clone(),
        ..TransformAnimations::default()
    };
    media::validate_native(&footage, duration)
        .map_err(|_| invalid("native footage validation rejected the takeover image"))?;

    let mut composition_record = CompositionRecord::empty_ae26(width, viewport_height, duration)?;
    crate::writer::apply_composition_options(&mut composition_record, composition_options)?;

    let inner_options = plain_options(ids.inner_footage);
    let inner = LayerSpec::Options(
        Box::new(LayerSpec::Footage(footage, inner_animations)),
        inner_options,
    );
    root_options.parent = Some(ids.slide_parent);
    let occurrence = LayerSpec::Options(
        Box::new(LayerSpec::Timed(
            Box::new(LayerSpec::Precomposition(PrecompositionSpec {
                collapse_transformations: false,
                name: "Media Takeover Viewport".to_owned(),
                width,
                height: viewport_height,
                duration,
                transform: authored_transform,
                transform_animations: authored_animations,
                layers: vec![inner],
                composition_record: Some(composition_record),
            })),
            LayerTiming {
                start_millis: start,
                end_millis: tail_end,
            },
        )),
        root_options,
    );

    let slide = slide_track(placement, f64::from(viewport_height), start, end, tail_end);
    let parent = LayerSpec::Options(
        Box::new(LayerSpec::Timed(
            Box::new(LayerSpec::Null(NullLayerSpec {
                name: "Media Takeover Slide".to_owned(),
                transform: identity_transform(),
                transform_animations: TransformAnimations {
                    position: Some(slide.clone()),
                    ..TransformAnimations::default()
                },
            })),
            LayerTiming {
                start_millis: start,
                end_millis: tail_end,
            },
        )),
        plain_options(ids.slide_parent),
    );

    Ok(TakeoverPlan {
        layers: vec![occurrence, parent],
        required_synthetic_ids: [ids.inner_footage, ids.slide_parent],
        diagnostics: Vec::new(),
    })
}

fn check_synthetic_ids(
    root: LayerId,
    ids: TakeoverIds,
    occupied: &BTreeSet<LayerId>,
) -> Result<(), AepWriteError> {
    if ids.inner_footage == root
        || ids.slide_parent == root
        || ids.inner_footage == ids.slide_parent
        || occupied.contains(&ids.inner_footage)
        || occupied.contains(&ids.slide_parent)
    {
        return Err(invalid(
            "takeover synthetic layer identity collides with the document",
        ));
    }
    Ok(())
}

fn plain_options(fx_id: LayerId) -> NativeLayerOptions {
    NativeLayerOptions {
        fx_id,
        parent: None,
        matte: None,
        enabled: true,
        adjustment_layer: false,
        motion_blur: false,
        blend_mode: 0,
        masks: Vec::new(),
        effects: Vec::new(),
        styles: Vec::new(),
        source_clock: None,
        transform_3d: None,
    }
}

fn identity_transform() -> SolidTransform {
    SolidTransform {
        anchor: [0.0; 2],
        position: [0.0; 2],
        scale: [100.0; 2],
        rotation: 0.0,
        opacity: 100.0,
    }
}

fn content_pan(
    footage: &crate::writer::footage::FootageSpec,
    viewport: [f64; 2],
    start: i64,
    end: i64,
) -> Option<NumericTrack> {
    if end - start <= PAN_MIN_MILLIS {
        return None;
    }
    let rendered = [
        f64::from(footage.source.dimensions[0]) * footage.source_geometry.scale[0],
        f64::from(footage.source.dimensions[1]) * footage.source_geometry.scale[1],
    ];
    let overflow = [rendered[0] - viewport[0], rendered[1] - viewport[1]];
    let axis = usize::from(overflow[1] > overflow[0]);
    if overflow[axis] <= 0.0 {
        return None;
    }
    let leading = -footage.source_geometry.origin[axis];
    let trailing = viewport[axis] - footage.source_geometry.origin[axis] - rendered[axis];
    let mut from = [0.0; 2];
    let mut to = [0.0; 2];
    from[axis] = leading;
    to[axis] = trailing;
    Some(NumericTrack {
        keys: vec![
            position_key(start, from, KeyframeEasing::Hold),
            position_key(start + SLIDE_MILLIS, from, KeyframeEasing::Hold),
            position_key(end - SLIDE_MILLIS, to, PAN_EASING),
            position_key(end, to, KeyframeEasing::Hold),
        ],
    })
}

fn slide_track(
    placement: MediaPlacement,
    viewport_height: f64,
    start: i64,
    end: i64,
    tail_end: i64,
) -> NumericTrack {
    let displacement = match placement {
        MediaPlacement::TopHalf => -viewport_height,
        MediaPlacement::BottomHalf => viewport_height,
    };
    let mut keys = vec![
        position_key(start, [0.0, displacement], KeyframeEasing::Linear),
        position_key(start + SLIDE_MILLIS, [0.0; 2], KeyframeEasing::Linear),
    ];
    if end > start + SLIDE_MILLIS {
        keys.push(position_key(end, [0.0; 2], KeyframeEasing::Linear));
    }
    keys.push(position_key(
        tail_end,
        [0.0, displacement],
        KeyframeEasing::Linear,
    ));
    NumericTrack { keys }
}

fn position_key(time_millis: i64, values: [f64; 2], easing: KeyframeEasing) -> NumericKeyframe {
    NumericKeyframe {
        time_millis,
        values: vec![values[0], values[1], 0.0],
        easing: vec![easing],
        spatial_in: vec![0.0; 3],
        spatial_out: vec![0.0; 3],
    }
}

fn invalid(message: &'static str) -> AepWriteError {
    AepWriteError::Invalid(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_slide_is_after_authored_transform_and_has_exact_tail() {
        let track = slide_track(MediaPlacement::TopHalf, 960.0, 1_000, 2_000, 2_120);
        assert_eq!(
            track
                .keys
                .iter()
                .map(|key| key.time_millis)
                .collect::<Vec<_>>(),
            vec![1_000, 1_120, 2_000, 2_120]
        );
        assert_eq!(track.keys[0].values, vec![0.0, -960.0, 0.0]);
        assert_eq!(track.keys[1].values, vec![0.0, 0.0, 0.0]);
        assert_eq!(track.keys[3].values, vec![0.0, -960.0, 0.0]);
        assert!(
            track
                .keys
                .iter()
                .all(|key| key.easing == vec![KeyframeEasing::Linear])
        );
    }

    #[test]
    fn minimum_window_does_not_emit_duplicate_zero_keys() {
        let track = slide_track(MediaPlacement::BottomHalf, 500.0, 40, 160, 280);
        assert_eq!(
            track
                .keys
                .iter()
                .map(|key| key.time_millis)
                .collect::<Vec<_>>(),
            vec![40, 160, 280]
        );
        assert_eq!(track.keys[0].values, vec![0.0, 500.0, 0.0]);
    }

    #[test]
    fn pan_segment_uses_pinned_bezier_between_endpoint_holds() {
        let track = NumericTrack {
            keys: vec![
                position_key(0, [50.0, 0.0], KeyframeEasing::Hold),
                position_key(120, [50.0, 0.0], KeyframeEasing::Hold),
                position_key(1_880, [-50.0, 0.0], PAN_EASING),
                position_key(2_000, [-50.0, 0.0], KeyframeEasing::Hold),
            ],
        };
        assert_eq!(track.keys[2].easing, vec![PAN_EASING]);
        assert_eq!(track.keys[1].time_millis - track.keys[0].time_millis, 120);
        assert_eq!(track.keys[3].time_millis - track.keys[2].time_millis, 120);
    }

    #[test]
    fn synthetic_ids_must_be_fresh_and_distinct() {
        let root = LayerId::new(7);
        let mut occupied = BTreeSet::new();
        occupied.insert(LayerId::new(9));
        assert!(
            check_synthetic_ids(
                root,
                TakeoverIds {
                    inner_footage: LayerId::new(8),
                    slide_parent: LayerId::new(9),
                },
                &occupied,
            )
            .is_err()
        );
        assert!(
            check_synthetic_ids(
                root,
                TakeoverIds {
                    inner_footage: LayerId::new(8),
                    slide_parent: LayerId::new(10),
                },
                &occupied,
            )
            .is_ok()
        );
    }
}
