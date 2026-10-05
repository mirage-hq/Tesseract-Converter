use super::hue_saturation::{self, CHANNEL_RANGE};
use crate::rifx::Chunk;

fn payload() -> Vec<u8> {
    let mut values = vec![142_i32, -75, 20];
    for range in [
        [315, 345, 15, 45],
        [15, 45, 75, 105],
        [75, 105, 135, 165],
        [135, 165, 195, 225],
        [195, 225, 255, 285],
        [255, 285, 315, 345],
    ] {
        values.extend(range);
        values.extend([0, 0, 0]);
    }
    values.into_iter().flat_map(i32::to_be_bytes).collect()
}

fn run(bytes: Vec<u8>, animated: bool, expression: bool) -> Vec<Chunk> {
    let mut meta = vec![0; 124];
    meta[..4].copy_from_slice(&[0xdb, 0x99, 0, 1]);
    meta[57] = 1;
    meta[59] = 8;
    meta[68] = u8::from(animated);
    meta[120] = u8::from(expression);
    vec![
        Chunk::list(
            *b"tdbs",
            vec![
                Chunk::data(*b"tdb4", meta).unwrap(),
                Chunk::data(*b"tdsb", [0, 0, 0, 1]).unwrap(),
            ],
        ),
        Chunk::list(*b"aRbs", vec![Chunk::data(*b"aRbp", bytes).unwrap()]),
    ]
}

#[test]
fn packed_hue_static_master_and_nonmaster_omission_are_independent() {
    let mut bytes = payload();
    // One independent red-channel saturation adjustment is not a Master value.
    bytes[32..36].copy_from_slice(&30_i32.to_be_bytes());
    let run = run(bytes, false, false);
    let mut warnings = Vec::new();
    let master = hue_saturation::read(&[(CHANNEL_RANGE, &run)], &mut warnings)
        .unwrap()
        .unwrap();
    for (name, value) in [
        ("ADBE HUE SATURATION-0004", 142.0),
        ("ADBE HUE SATURATION-0005", -75.0),
        ("ADBE HUE SATURATION-0006", 20.0),
    ] {
        assert_eq!(master.numeric(name).unwrap().values, [value]);
    }
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("channel record 1"));
}

#[test]
fn packed_hue_rejects_animated_expression_duplicate_and_unknown_records() {
    for (animated, expression) in [(true, false), (false, true)] {
        let run = run(payload(), animated, expression);
        assert!(hue_saturation::read(&[(CHANNEL_RANGE, &run)], &mut Vec::new()).is_err());
    }
    let valid = run(payload(), false, false);
    assert!(
        hue_saturation::read(
            &[(CHANNEL_RANGE, &valid), (CHANNEL_RANGE, &valid)],
            &mut Vec::new()
        )
        .is_err()
    );
    for bytes in [vec![0; 176], vec![0; 184]] {
        let run = run(bytes, false, false);
        assert!(hue_saturation::read(&[(CHANNEL_RANGE, &run)], &mut Vec::new()).is_err());
    }
    let mut bytes = payload();
    bytes[4..8].copy_from_slice(&101_i32.to_be_bytes());
    let invalid = run(bytes, false, false);
    assert!(hue_saturation::read(&[(CHANNEL_RANGE, &invalid)], &mut Vec::new()).is_err());
}

#[test]
fn packed_hue_master_matches_writer_angle_bounds_without_losing_turns() {
    for hue in [-32_768_i32, -540, 540, 32_767] {
        let mut bytes = payload();
        bytes[..4].copy_from_slice(&hue.to_be_bytes());
        let run = run(bytes, false, false);
        let master = hue_saturation::read(&[(CHANNEL_RANGE, &run)], &mut Vec::new())
            .unwrap()
            .unwrap();
        assert_eq!(
            master.numeric("ADBE HUE SATURATION-0004").unwrap().values,
            [f64::from(hue)]
        );
        assert!(crate::writer::effects::hue_master_fixed(f64::from(hue)).is_ok());
    }
    for hue in [i32::MIN, -32_769, 32_768, i32::MAX] {
        let mut bytes = payload();
        bytes[..4].copy_from_slice(&hue.to_be_bytes());
        let run = run(bytes, false, false);
        assert!(hue_saturation::read(&[(CHANNEL_RANGE, &run)], &mut Vec::new()).is_err());
        assert!(crate::writer::effects::hue_master_fixed(f64::from(hue)).is_err());
    }
}

#[test]
fn packed_hue_disabled_expression_retains_static_master() {
    for (flags, accepted) in [(0, false), (1, true), (2, false)] {
        let mut run = run(payload(), false, true);
        let leaf = run[0].children_mut().unwrap();
        let mut meta = leaf[0].data_payload().unwrap().to_vec();
        meta[119] = flags;
        leaf[0] = Chunk::data(*b"tdb4", meta).unwrap();
        // Physically retained expression source is not an enabled expression.
        let mut children = leaf.to_vec();
        children.push(Chunk::data(*b"Utf8", b"value".to_vec()).unwrap());
        run[0] = Chunk::list(*b"tdbs", children);
        let mut warnings = Vec::new();
        let result = hue_saturation::read(&[(CHANNEL_RANGE, &run)], &mut warnings);
        if accepted {
            let master = result.unwrap().unwrap();
            assert_eq!(
                master.numeric("ADBE HUE SATURATION-0004").unwrap().values,
                [142.0]
            );
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].contains("disabled expression"));
        } else {
            assert!(result.is_err());
        }
    }
}

#[test]
fn packed_hue_absent_record_preserves_existing_numeric_import() {
    assert!(
        hue_saturation::read(&[], &mut Vec::new())
            .unwrap()
            .is_none()
    );
}
