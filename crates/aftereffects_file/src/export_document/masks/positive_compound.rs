//! Bounded positive primitive union, not a general winding conversion.

use super::*;
use crate::writer::{KeyframeEasing, PathKeyframe, PathTrack};

fn profile(
    mask: &PathMask,
    count: usize,
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> bool {
    count == 1
        && mask.layer.is_some()
        && mask.legacy_path.is_none()
        && mask.mode == MaskMode::Add
        && !mask.inverted
        && mask.feather == [0.0; 2]
        && mask.opacity.value() == 1.0
        && mask.expansion == 0.0
        && !dynamics
            .iter()
            .any(|entry| entry.target.fx_item_id() == Some(mask.id))
}

pub(super) fn lower(
    mask: &PathMask,
    count: usize,
    index: usize,
    owner: MaskOwner<'_>,
    siblings: &[Layer],
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> Result<(Vec<NativeMaskSpec>, LayerId, String), &'static str> {
    if !profile(mask, count, dynamics) {
        return Err(
            "requires one hard opaque non-inverted unexpanded Add guide mask without mask control animators",
        );
    }
    let id = mask.layer.ok_or("compound mask has no guide")?;
    let guide = checked_guide(id, owner, siblings, dynamics, true)?;
    let (base, transform) = checked_shape_guide(guide)?;
    let source = super::super::track(dynamics, id, fx_schema::PropType::ShapePath)?;
    let authored = super::super::path_animation::track(source)?;
    if authored.is_some() {
        let clock = owner
            .clock
            .ok_or("mask owner has an unproven source clock")?;
        let range = guide.active_range();
        if range.start != clock.start || range.duration < clock.duration {
            return Err("animated mask guide and owner clocks are not proven equivalent");
        }
    }
    let track = authored.unwrap_or_else(|| PathTrack {
        keyframes: vec![PathKeyframe {
            time_millis: 0,
            path: base.clone(),
            easing: KeyframeEasing::Hold,
        }],
    });
    for key in &track.keyframes {
        check_raw(&key.path)?;
    }
    let mut tracks = crate::writer::split_path_track(&track)
        .map_err(|_| "compound Path keys exceed native bounds or unsupported topology/easing")?;
    if tracks.len() < 2 {
        return Err("guide does not contain at least two positive primitive contours");
    }
    if u16::try_from(tracks.len()).is_err() {
        return Err("positive compound contour count exceeds the native mask field");
    }
    for track in &tracks {
        if track.keyframes.windows(2).any(|pair| {
            pair[1].easing != KeyframeEasing::Hold
                && pair[0].path.commands.len() > 1
                && pair[1].path.commands.len() > 1
                && pair[0].path.commands.len() != pair[1].path.commands.len()
        }) {
            return Err("non-Hold compound contour changes its positive primitive kind");
        }
    }
    let affine = relative_affine(owner.transform, transform)?;
    let mut specs = Vec::with_capacity(tracks.len());
    for (ordinal, track) in tracks.iter_mut().enumerate() {
        let base = track
            .keyframes
            .iter()
            .find(|key| key.path.commands.len() > 1)
            .ok_or("compound ordinal has no drawn key")?;
        let path = checked_path(transform_path(base.path.clone(), affine))?;
        for key in &mut track.keyframes {
            // Only split_path_track introduces absent slots. A native open
            // one-vertex mask has no filled area; edge alpha remains unmeasured.
            let transformed = transform_path(key.path.clone(), affine);
            key.path = if transformed.commands.len() == 1 {
                transformed
            } else {
                checked_path(transformed)?
            };
        }
        crate::writer::validate_path_track(track)
            .map_err(|_| "transformed compound mask Path keys exceed native bounds")?;
        specs.push(NativeMaskSpec {
            name: format!("Mask {index} contour {}", ordinal + 1),
            path,
            path_track: Some(track.clone()),
            source_size: owner.source_size,
            mode: NativeMaskMode::Add,
            inverted: false,
            feather: [0.0; 2],
            opacity: 1.0,
            expansion: 0.0,
            feather_track: None,
            opacity_track: None,
            expansion_track: None,
        });
    }
    let diagnostic = format!(
        "guide layer {id} was copied as {} positive compound contours into native Add masks with authored Path keys; the live cross-layer link is not retained; edge alpha/antialias fidelity versus the compound mask is unmeasured",
        specs.len()
    );
    Ok((specs, id, diagnostic))
}

fn check_raw(path: &ShapePath) -> Result<(), &'static str> {
    if !path.is_finite() {
        return Err("compound guide contains non-finite geometry");
    }
    let mut start = 0;
    for index in 1..=path.commands.len() {
        if index == path.commands.len()
            || matches!(path.commands[index], ShapePathCommand::MoveTo { .. })
        {
            if !positive_primitive(&path.commands[start..index]) {
                return Err(
                    "every drawn compound contour must be a canonical positive circle or axis-aligned rectangle",
                );
            }
            start = index;
        }
    }
    Ok(())
}

fn positive_primitive(commands: &[ShapePathCommand]) -> bool {
    let Some(ShapePathCommand::MoveTo {
        x,
        y,
        mirror: None,
        corner_radius: None,
    }) = commands.first()
    else {
        return false;
    };
    if let [
        _,
        ShapePathCommand::LineTo {
            x: right,
            y: top,
            mirror: None,
            corner_radius: None,
        },
        ShapePathCommand::LineTo {
            x: right2,
            y: bottom,
            mirror: None,
            corner_radius: None,
        },
        ShapePathCommand::LineTo {
            x: left,
            y: bottom2,
            mirror: None,
            corner_radius: None,
        },
        ShapePathCommand::Close,
    ] = commands
    {
        return right > x
            && bottom > y
            && top == y
            && right2 == right
            && left == x
            && bottom2 == bottom;
    }
    if commands.len() != 6
        || *y != 0.0
        || *x <= 0.0
        || !matches!(commands[5], ShapePathCommand::Close)
    {
        return false;
    }
    let ShapePathCommand::CubicTo { c1y: k, .. } = commands[1] else {
        return false;
    };
    let r = *x;
    if k <= 0.0 || k >= r {
        return false;
    }
    let expected = [
        [r, k, k, r, 0.0, r],
        [-k, r, -r, k, -r, 0.0],
        [-r, -k, -k, -r, 0.0, -r],
        [k, -r, r, -k, r, 0.0],
    ];
    commands[1..5].iter().zip(expected).all(|(command, expected)| {
        matches!(command, ShapePathCommand::CubicTo { c1x, c1y, c2x, c2y, x, y, mirror: None, corner_radius: None }
            if [*c1x, *c1y, *c2x, *c2y, *x, *y] == expected)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(r: f64, k: f64) -> ShapePath {
        let mut commands = vec![anchor(r, 0.0, true)];
        for [c1x, c1y, c2x, c2y, x, y] in [
            [r, k, k, r, 0.0, r],
            [-k, r, -r, k, -r, 0.0],
            [-r, -k, -k, -r, 0.0, -r],
            [k, -r, r, -k, r, 0.0],
        ] {
            commands.push(ShapePathCommand::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
                mirror: None,
                corner_radius: None,
            });
        }
        commands.push(ShapePathCommand::Close);
        ShapePath { commands }
    }

    #[test]
    fn only_positive_canonical_primitives_are_accepted() {
        assert!(positive_primitive(&circle(10.0, 5.0).commands));
        assert!(positive_primitive(
            &rectangle_path([2.0, 3.0], [4.0, 5.0]).commands
        ));
        let mut reversed = rectangle_path([2.0, 3.0], [4.0, 5.0]);
        reversed.commands.swap(1, 3);
        assert!(!positive_primitive(&reversed.commands));
        assert!(!positive_primitive(&circle(-10.0, 5.0).commands));
        for k in [-1.0, 0.0, 10.0, 11.0] {
            assert!(!positive_primitive(&circle(10.0, k).commands));
        }
        let mut arbitrary = circle(10.0, 5.0);
        if let ShapePathCommand::CubicTo { c2x, .. } = &mut arbitrary.commands[2] {
            *c2x += 1.0;
        }
        assert!(!positive_primitive(&arbitrary.commands));
    }

    #[test]
    fn profile_rejects_mask_controls() {
        let mut mask: PathMask = serde_json::from_value(serde_json::json!({
            "id": 123, "layer": 456, "mode": "add"
        }))
        .unwrap();
        let dynamics = crate::export_document::AnimationIndex::new(&[]);
        assert!(profile(&mask, 1, &dynamics));
        mask.inverted = true;
        assert!(!profile(&mask, 1, &dynamics));
        mask.inverted = false;
        mask.feather = [1.0, 0.0];
        assert!(!profile(&mask, 1, &dynamics));
        mask.feather = [0.0; 2];
        mask.expansion = 1.0;
        assert!(!profile(&mask, 1, &dynamics));
        mask.expansion = 0.0;
        mask.mode = MaskMode::Subtract;
        assert!(!profile(&mask, 1, &dynamics));
        mask.mode = MaskMode::Add;
        mask.opacity = serde_json::from_value(serde_json::json!(0.5)).unwrap();
        assert!(!profile(&mask, 1, &dynamics));
        mask.opacity = serde_json::from_value(serde_json::json!(1.0)).unwrap();
        let track = PropertyKeyframeTrack::new(vec![fx_schema::animator::PropertyKeyframe::new(
            fx_schema::animator::KeyframeId::new(String::from("compound-mask-control")),
            fx_schema::TimeOffset::from_millis(0),
            PropertyValue::Float(1.0),
            fx_schema::PropertyKeyframeEasing::Hold,
        )])
        .unwrap();
        let entries = [fx_schema::animator::AnimationGraphEntry {
            target: PropertyTarget::fx_item(mask.id, "opacity"),
            animator: fx_schema::PropertyAnimator::keyframes(track),
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        }];
        assert!(!profile(
            &mask,
            1,
            &crate::export_document::AnimationIndex::new(&entries)
        ));
    }
}
