//! Storage-unit regressions; generated layers are supplementary structural evidence,
//! not Adobe acceptance or independent render-fidelity proof.

use super::*;
use crate::properties::{NumericProperty, read_transform};
use crate::writer::{KeyframeEasing, NumericKeyframe};

fn property(content: &[Chunk], name: &str) -> NumericProperty {
    read_transform(content)
        .unwrap()
        .into_iter()
        .find(|property| property.match_name == name)
        .unwrap_or_else(|| panic!("missing {name}"))
        .numeric
        .unwrap()
}

#[test]
fn static_footage_anchor_uses_native_source_relative_storage() {
    // The pinned native file establishes a real file-footage owner, but omits
    // its default Anchor leaf. Do not invent an explicit binary oracle for it:
    // the controlled native render pair establishes the .5/.5 storage units.
    let native = read_project(include_bytes!(
        "../../../../tests/fixtures/pr4442_native/sources/media_video.aep"
    ))
    .unwrap();
    let native_layer = native
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            crate::structure::ItemKind::Composition(composition) => Some(composition),
            _ => None,
        })
        .flat_map(|composition| &composition.layers)
        .find(|layer| {
            native.item(layer.record.source_id()).is_some_and(|source| {
                source.footage.as_ref().is_some_and(|footage| {
                    footage.main_source == crate::structure::FootageSourceKind::File
                })
            })
        })
        .expect("pinned native file-footage layer");
    assert!(
        read_transform(&native_layer.content)
            .unwrap()
            .iter()
            .all(|property| property.match_name != "ADBE Anchor Point")
    );

    let mut spec = quicktime_spec(FootageKind::Video, false);
    spec.source.dimensions = [1280, 720];
    spec.transform.width = 1280;
    spec.transform.height = 720;
    spec.transform.transform.anchor = [640.0, 360.0];
    spec.transform.transform.position = [905.0, 528.0];
    spec.transform.transform.scale = [172.5, 172.5];
    let layer = timeline_layer(&spec, 3, 2, Duration24::from_frames(24).unwrap(), None).unwrap();
    let content = layer.children().unwrap();
    assert_eq!(
        property(content, "ADBE Anchor Point").values,
        [0.5, 0.5, 0.0]
    );
    assert_eq!(
        property(content, "ADBE Position").values,
        [905.0, 528.0, 0.0]
    );
    assert_eq!(property(content, "ADBE Scale").values, [1.725, 1.725, 1.0]);
}

#[test]
fn footage_anchor_normalizes_after_source_geometry_without_moving_position() {
    let mut spec = quicktime_spec(FootageKind::Video, false);
    spec.source.dimensions = [1280, 720];
    spec.transform.width = 1280;
    spec.transform.height = 720;
    spec.source_geometry = SourceGeometry {
        origin: [100.0, -75.0],
        scale: [2.0, 3.0],
    };
    spec.transform.transform.anchor = [1380.0, 1005.0];
    spec.transform.transform.position = [905.0, 528.0];
    let layer = timeline_layer(&spec, 3, 2, Duration24::from_frames(24).unwrap(), None).unwrap();
    let content = layer.children().unwrap();
    assert_eq!(
        property(content, "ADBE Anchor Point").values,
        [0.5, 0.5, 0.0]
    );
    assert_eq!(
        property(content, "ADBE Position").values,
        [905.0, 528.0, 0.0]
    );
    assert_eq!(property(content, "ADBE Scale").values, [2.0, 3.0, 1.0]);
}

#[test]
fn animated_footage_anchor_normalizes_values_and_spatial_tangents() {
    let mut spec = quicktime_spec(FootageKind::Video, false);
    spec.source.dimensions = [1280, 720];
    spec.transform.width = 1280;
    spec.transform.height = 720;
    let animations = TransformAnimations {
        anchor: Some(NumericTrack {
            keys: [(0, [640.0, 360.0, 7.0]), (1000, [960.0, 180.0, 9.0])]
                .into_iter()
                .map(|(time_millis, values)| NumericKeyframe {
                    time_millis,
                    values: values.to_vec(),
                    easing: vec![KeyframeEasing::Linear],
                    spatial_in: vec![-128.0, -72.0, 3.0],
                    spatial_out: vec![256.0, 144.0, 4.0],
                })
                .collect(),
        }),
        ..Default::default()
    };
    let layer = timeline_layer(
        &spec,
        3,
        2,
        Duration24::from_frames(24).unwrap(),
        Some(&animations),
    )
    .unwrap();
    let anchor = property(layer.children().unwrap(), "ADBE Anchor Point");
    assert!(anchor.animated);
    assert_eq!(anchor.keyframes.len(), 2);
    for (key, (time, values)) in anchor
        .keyframes
        .iter()
        .zip([(0.0, [0.5, 0.5, 7.0]), (1.0, [0.75, 0.25, 9.0])])
    {
        assert_eq!(key.values, values);
        assert_eq!(key.time_secs, time);
        assert_eq!(key.spatial_in, [-0.1, -0.1, 3.0]);
        assert_eq!(key.spatial_out, [0.2, 0.2, 4.0]);
    }
}
