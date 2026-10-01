use super::demand::{Demand, Homography, Segment};
use super::{Bounds, effect_support, finite_mask_gate};
use fx_schema::effect::EffectRecord;

fn box_at(min: [f64; 2], max: [f64; 2]) -> Bounds {
    Bounds { min, max }
}

#[test]
fn offscreen_content_entering_viewport_is_retained_across_coupled_segments() {
    let viewport = box_at([0.0, 0.0], [100.0, 100.0]);
    let mut demand = Demand::root(viewport, 2_000);
    demand.split_at(1_000);
    demand.inverse_segments(|segment| {
        let translation = if segment.root_start == 0 { 500.0 } else { 0.0 };
        Homography::affine([1.0, 0.0, 0.0, 1.0], [translation, 0.0])
    });
    let bounds = demand
        .finite_union()
        .expect("all affine inverses certified");
    assert!(bounds.min[0] <= -500.0 && bounds.max[0] >= 100.0);
}

#[test]
fn finite_projective_plane_checks_inverse_denominator_and_near_plane() {
    let viewport = box_at([0.0, 0.0], [100.0, 100.0]);
    let safe = Homography::new([[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]], 2.0);
    let result = safe.preimage(viewport).expect("identity projective plane");
    assert_eq!(result.min, viewport.min);
    assert_eq!(result.max, viewport.max);
    let horizon = Homography::new([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, -50.0]], 2.0);
    assert!(horizon.preimage(viewport).is_err());
    let near_plane = Homography::new([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0e-18]], 2.0);
    assert!(near_plane.preimage(viewport).is_err());
}

#[test]
fn singular_or_unknown_segment_invalidates_the_entire_crop() {
    let viewport = box_at([0.0, 0.0], [100.0, 100.0]);
    let mut demand = Demand::root(viewport, 2_000);
    demand.split_at(1_000);
    demand.inverse_segments(|segment: Segment| {
        let scale = if segment.root_start == 0 { 1.0 } else { 0.0 };
        Homography::affine([scale, 0.0, 0.0, scale], [0.0, 0.0])
    });
    assert!(demand.finite_union().is_err());
}

#[test]
fn mask_phase_preserves_pre_mask_input_and_refuses_unproved_post_mask_glow() {
    let mask = box_at([10.0, 10.0], [20.0, 20.0]);
    let mut demand = Demand::root(box_at([0.0, 0.0], [100.0, 100.0]), 1_000);
    demand.intersect(mask);
    assert_eq!(demand.finite_union().unwrap().min, mask.min);
    assert_eq!(demand.finite_union().unwrap().max, mask.max);
    let glow: EffectRecord = serde_json::from_value(serde_json::json!({
        "id": 100,
        "enabled": true,
        "effect": {"type":"glow","glowThreshold":20,"glowRadius":8,"glowIntensity":0.5}
    }))
    .unwrap();
    let reason = effect_support::stack(&[glow]).unwrap_err();
    assert!(reason.contains("Glow native finite reach"));
    demand.full(reason);
    assert!(
        demand.finite_union().is_err(),
        "a post-mask effect cannot be dropped by intersecting the mask"
    );
}

#[test]
fn affine_reflection_is_invertible_but_singular_scale_is_not() {
    let region = box_at([0.0, 0.0], [100.0, 100.0]);
    let reflected = Homography::affine([-1.0, 0.0, 0.0, 2.0], [0.0, 0.0]);
    let inverse = reflected.preimage(region).unwrap();
    assert_eq!(inverse.min, [-100.0, 0.0]);
    assert_eq!(inverse.max, [0.0, 50.0]);
    assert!(
        Homography::affine([0.0; 4], [0.0; 2])
            .preimage(region)
            .is_err()
    );
}

#[test]
fn root_local_clock_split_retains_origin_and_exact_rate() {
    let mut demand = Demand::root(box_at([0.0, 0.0], [100.0, 100.0]), 3_000);
    demand.active(1_000, 2_000);
    demand.split_at(1_500);
    demand.map_clock(1_000, 2.0);
    let segments = demand.segments();
    assert_eq!(segments.len(), 2);
    assert_eq!(
        (
            segments[0].root_start,
            segments[0].local_start,
            segments[0].local_rate
        ),
        (1_000, 0.0, 2.0)
    );
    assert_eq!(
        (
            segments[1].root_start,
            segments[1].local_start,
            segments[1].local_rate
        ),
        (1_500, 1_000.0, 2.0)
    );
    demand.active(0, 1_000);
    assert_eq!(demand.segments().len(), 1);
    assert_eq!(demand.segments()[0].root_start, 1_000);
    assert_eq!(demand.segments()[0].local_start, 0.0);
    assert_eq!(demand.segments()[0].root_end, 1_500);
}

#[test]
fn finite_mask_gate_requires_static_hard_add_inline_path() {
    let value = serde_json::json!({
        "id":1,"mode":"add","opacity":1.0,"feather":[0.0,0.0],"expansion":0.0,
        "path":{"commands":[
            {"type":"moveTo","x":10.0,"y":10.0},
            {"type":"lineTo","x":20.0,"y":10.0},
            {"type":"lineTo","x":20.0,"y":20.0},
            {"type":"close"}
        ]}
    });
    let mask: fx_schema::layer::PathMask = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(finite_mask_gate(&[mask], &[]).unwrap().max, [20.0, 20.0]);
    let mut inverted = value;
    inverted["inverted"] = serde_json::json!(true);
    let inverted = serde_json::from_value(inverted).unwrap();
    assert!(finite_mask_gate(&[inverted], &[]).is_err());
}
