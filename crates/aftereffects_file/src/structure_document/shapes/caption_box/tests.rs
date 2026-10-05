use super::*;

#[test]
fn complete_caption_formulas_keep_binding_names_and_operators() {
    assert_eq!(
        canonical(SIZE),
        canonical(&format!(
            "/* caption */ {} // end",
            SIZE.replace('\n', "\r\n\t")
        ))
    );
    for changed in [
        SIZE.replace("index - 1", "index + 1"),
        SIZE.replace("textLayer", "text Layer"),
        SIZE.replace("0.6", "0 .6"),
        SIZE.replace("textWidth", "text/* split */Width"),
        SIZE.replace("time, false", "time, true"),
        SIZE.replace("0.6", "0.8"),
        SIZE.replace("textWidth + padding", "textWidth - padding"),
        SIZE.replace("Width Padding", "WidthPadding"),
        format!("{SIZE}; value"),
    ] {
        assert_ne!(canonical(SIZE), canonical(&changed));
    }
    assert_ne!(
        canonical(ANCHOR),
        canonical(&ANCHOR.replace("Rectangle 1", "Other Rectangle"))
    );
    assert_ne!(canonical(ANCHOR), canonical(&ANCHOR.replace("w/-2", "w/2")));
    assert!(canonical("/* unterminated").is_none());
    assert!(canonical(&" ".repeat(8_193)).is_none());
}

#[test]
fn caption_reveal_has_two_source_local_eased_keys_and_zero_width_endpoint() {
    let curve = reveal(11.5999, vec![0.0, 104.0], vec![835.1543, 104.0]);
    assert_eq!(curve.keyframes.len(), 2);
    assert_eq!(curve.keyframes[0].time_secs, 11.5999);
    assert_eq!(curve.keyframes[1].time_secs, 12.1999);
    assert_eq!(curve.keyframes[0].values, [0.0, 104.0]);
    for key in &curve.keyframes {
        assert_eq!(key.in_speed, [0.0, 0.0]);
        assert_eq!(key.out_speed, [0.0, 0.0]);
        assert_eq!(key.in_interpolation, 2);
        assert_eq!(key.out_interpolation, 2);
        assert_eq!(key.in_influence, [100.0 / 3.0; 2]);
        assert_eq!(key.out_influence, [100.0 / 3.0; 2]);
    }
}
