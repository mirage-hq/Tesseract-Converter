use super::*;

pub(super) fn sample_vertices(track: &PropertyKeyframeTrack, time: i64) -> Vec<[f64; 2]> {
    let keys = track.keyframes();
    assert!(keys.first().unwrap().layer_time().as_millis() <= time);
    assert!(keys.last().unwrap().layer_time().as_millis() >= time);
    let after = keys.partition_point(|key| key.layer_time().as_millis() <= time);
    let from = &keys[after.saturating_sub(1)];
    let to = &keys[after.min(keys.len() - 1)];
    let points = |key: &PropertyKeyframe| {
        let PropertyValue::Path(path) = key.value() else {
            panic!("editable Path key")
        };
        // Generated straight closures use Close directly. A curved closure
        // retains its explicit terminal CubicTo back to the first vertex.
        let mut vertices: Vec<_> = path
            .commands
            .iter()
            .filter_map(ShapePathCommand::endpoint)
            .map(|(x, y)| [x, y])
            .collect();
        if matches!(path.commands.last(), Some(ShapePathCommand::Close))
            && matches!(
                path.commands.iter().rev().nth(1),
                Some(ShapePathCommand::CubicTo { .. })
            )
        {
            assert_eq!(vertices.last(), vertices.first());
            vertices.pop();
        }
        vertices
    };
    let left = points(from);
    let right = points(to);
    assert_eq!(
        left.len(),
        right.len(),
        "generated Path topology stays fixed"
    );
    let from_ms = from.layer_time().as_millis();
    let to_ms = to.layer_time().as_millis();
    let progress = if to_ms == from_ms {
        0.0
    } else {
        (time - from_ms) as f64 / (to_ms - from_ms) as f64
    };
    assert_eq!(to.easing(), PropertyKeyframeEasing::Linear);
    left.iter()
        .zip(&right)
        .map(|(a, b)| {
            [
                a[0] + (b[0] - a[0]) * progress,
                a[1] + (b[1] - a[1]) * progress,
            ]
        })
        .collect()
}

pub(super) fn assert_feature_vertices(
    id: u32,
    track: &PropertyKeyframeTrack,
    time: i64,
    initial: &ShapePath,
) {
    let sampled = sample_vertices(track, time);
    if let Some(expected) = middle_scene_expected(id, time) {
        assert_points_close(id, time, &sampled, &expected, 0.02);
    }
    match id {
        2459 | 2466 | 2468 => {
            assert_eq!(sampled.len(), 2);
            // Independent pre-fix CPU circle-center observation, not this new Path
            // fitter's output. 2px covers the 416.667 -> 417ms clock quantization.
            let expected = [[1798.7489, 994.0236], [2041.2511, 605.9764]];
            for (actual, expected) in sampled.iter().zip(expected) {
                let world = [actual[0] + 1920.0, actual[1] + 800.0];
                assert!(
                    (world[0] - expected[0]).hypot(world[1] - expected[1]) < 2.0,
                    "connector {id} at {time}ms: {world:?}, expected {expected:?}"
                );
            }
            assert!(
                (sampled[0][1] - sampled[1][1]).abs() > 300.0,
                "connector must be diagonal"
            );
        }
        2636 => assert_points_close(
            id,
            time,
            &sampled,
            &[[0.0, 255.311741], [0.0, -417.853087]],
            0.02,
        ),
        2627 => assert_points_close(
            id,
            time,
            &sampled,
            &[
                [271.311508, 255.311741],
                [-271.311508, 255.311741],
                [-74.85, -417.853087],
                [74.85, -417.853087],
            ],
            0.02,
        ),
        _ => {
            assert_eq!(sampled.len(), 4);
            assert!(
                sampled.iter().any(|point| {
                    (point[0].abs() - 250.0).abs() > 1.0 || (point[1].abs() - 250.0).abs() > 1.0
                }),
                "native {id} must not retain the stale 500px square: {sampled:?}"
            );
            if id == 750 {
                // The stock loop explicitly leaves self-selected vertex 2 alone.
                assert_eq!(
                    sampled[2],
                    PathGeometry::new(initial.clone()).unwrap().points[2]
                );
            }
        }
    }
}

fn middle_scene_expected(id: u32, time: i64) -> Option<[[f64; 2]; 4]> {
    // Supplementary frozen fitted-output regression at the aligned native-frame
    // neighborhoods. These values were captured from the corrected TransformRig
    // and are not independent native proof; the pinned Adobe-frame comparison is
    // the independent visual oracle for this geometry.
    match (id, time) {
        (2147, 42) => Some([
            [-1933.755527, 407.956053],
            [-1929.308678, 2182.899462],
            [-1933.249007, 1419.954728],
            [-1933.249007, 407.956053],
        ]),
        (2147, 83) => Some([
            [-1933.965801, 407.917037],
            [-1929.519378, 2182.690697],
            [-1933.006056, 1419.818928],
            [-1933.006056, 407.917037],
        ]),
        (2147, 125) => Some([
            [-1934.327813, 407.849379],
            [-1929.882127, 2182.328672],
            [-1932.584772, 1419.583434],
            [-1932.584772, 407.849379],
        ]),
        (2147, 2_333) => Some([
            [-1953.645906, 229.842303],
            [-1951.140553, 1229.844824],
            [-882.961044, 800.002017],
            [-882.961044, 229.842303],
        ]),
        (2147, 2_375) => Some([
            [-1960.780293, 229.630217],
            [-1958.277008, 1228.807134],
            [-874.164920, 800.831793],
            [-874.218099, 229.630217],
        ]),
        (2147, 2_417) => Some([
            [-1968.556078, 229.359001],
            [-1966.055207, 1227.572480],
            [-864.404764, 804.822901],
            [-864.646865, 229.359001],
        ]),
        (2157, 42) => Some([
            [1933.249007, -1419.954728],
            [1933.064578, -407.956053],
            [1933.249007, -407.956053],
            [1933.249007, -1419.954728],
        ]),
        (2157, 83) => Some([
            [1933.007019, -1419.819467],
            [1932.274532, -407.917192],
            [1933.007019, -407.917192],
            [1933.007019, -1419.819467],
        ]),
        (2157, 125) => Some([
            [1932.584772, -1419.583434],
            [1930.902008, -407.849379],
            [1932.584772, -407.849379],
            [1932.584772, -1419.583434],
        ]),
        (2157, 2_333) => Some([
            [882.961044, -800.002017],
            [0.003055, -229.842303],
            [882.961044, -229.842303],
            [882.961044, -800.002017],
        ]),
        (2157, 2_375) => Some([
            [874.164920, -800.831793],
            [-0.021371, -229.548827],
            [874.218099, -229.630217],
            [874.164920, -800.831793],
        ]),
        (2157, 2_417) => Some([
            [864.404764, -804.822901],
            [-0.096338, -228.995209],
            [864.646865, -229.359001],
            [864.404764, -804.822901],
        ]),
        (2199, 750) => Some([
            [-882.897759, -799.999999],
            [-882.897759, -229.841723],
            [882.897759, -229.841723],
            [882.897759, -799.999999],
        ]),
        (2199, 792) => Some([
            [-874.243094, -800.684981],
            [-874.189014, -229.462738],
            [874.145550, -229.628256],
            [874.091469, -800.850499],
        ]),
        (2199, 833) => Some([
            [-865.233184, -804.016237],
            [-864.995539, -228.649133],
            [864.806365, -229.363596],
            [864.568720, -804.730701],
        ]),
        _ => None,
    }
}

fn assert_points_close(
    id: u32,
    time: i64,
    actual: &[[f64; 2]],
    expected: &[[f64; 2]],
    tolerance: f64,
) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert!(
            (actual[0] - expected[0]).hypot(actual[1] - expected[1]) <= tolerance,
            "native {id} at {time}ms: {actual:?}, expected {expected:?}"
        );
    }
}
