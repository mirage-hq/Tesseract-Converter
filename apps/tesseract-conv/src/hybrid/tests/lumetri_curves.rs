//! FX `colorCurves` through CLI export at the default 30 fps, where a linked
//! After Effects scope would be exact, and a fresh import of the written
//! project. A non-identity Master curve and curves that Premiere's spline
//! draws differently stay a native Lumetri Color RGB Curves component: the
//! Master is folded into the Red, Green and Blue curves (Luma identity) and
//! reported as a retained approximation, with no linked After Effects scope.
use super::lumetri_round_trip::human_lumetri_document;
use super::*;

fn points(points: &[[f64; 2]]) -> Value {
    Value::Array(
        points
            .iter()
            .map(|[x, y]| json!({"x": x, "y": y}))
            .collect(),
    )
}

/// FX's straight-segment value of the wire curve `curve` at `x`.
fn polyline(curve: &Value, x: f64) -> f64 {
    let curve = curve.as_array().unwrap();
    let pair = curve
        .windows(2)
        .find(|pair| x <= pair[1]["x"].as_f64().unwrap())
        .unwrap();
    let [x0, y0, x1, y1] = [&pair[0]["x"], &pair[0]["y"], &pair[1]["x"], &pair[1]["y"]]
        .map(|value| value.as_f64().unwrap());
    y0 + (x - x0) / (x1 - x0) * (y1 - y0)
}

#[test]
fn color_curves_with_a_master_export_natively_at_30_fps_folded_into_the_channels() {
    let parent = tempfile::tempdir().unwrap();
    let mut document = human_lumetri_document(parent.path());
    let master = points(&[[0.0, 0.0], [0.5, 0.65], [1.0, 1.0]]);
    let red = points(&[[0.0, 0.1], [1.0, 0.9]]);
    let diagonal = points(&[[0.0, 0.0], [1.0, 1.0]]);
    document["composition"]["layers"][0]["effects"] = json!([
        {"id": 1, "enabled": true, "effect": {
            "type": "colorCurves", "semanticVersion": 1, "curves": {
                "master": master, "red": red, "green": diagonal, "blue": diagonal
            }
        }}
    ]);
    let input = parent.path().join("curves.tsrct");
    archive_with_video(&document, &input, "video-30fps-10s.mp4");
    let output = parent.path().join("curves-exported");
    let report = export(
        &request(&input, &output, ConversionMode::Write),
        &Default::default(),
    )
    .unwrap();
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|note| note.message.contains("not exported") || note.code.starts_with("HYBRID")),
        "{report:?}"
    );
    assert!(
        report.diagnostics.iter().any(|note| note
            .message
            .contains("colorCurves exports as native Lumetri Color RGB Curves with the Master curve folded into the Red, Green and Blue curves")),
        "{report:?}"
    );
    let prproj = output.join("project.prproj");
    let text = xml(&prproj);
    assert_eq!(
        text.matches("<MatchName>AE.ADBE Lumetri</MatchName>")
            .count(),
        1
    );
    assert!(!text.contains(".aep"));
    assert!(!output.join("media/ae-0001").exists());

    let reimported = parent.path().join("curves-reimported");
    import_premiere(&request(&prproj, &reimported, ConversionMode::Write)).unwrap();
    let again = TesseractFile::open(reimported.join("project.tsrct"))
        .unwrap()
        .project_json()
        .unwrap();
    let effects = again["composition"]["layers"][0]["effects"]
        .as_array()
        .unwrap();
    assert_eq!(effects.len(), 1, "{effects:?}");
    let curves = &effects[0]["effect"]["curves"];
    assert_eq!(effects[0]["effect"]["type"], "colorCurves");
    assert_eq!(curves["master"], diagonal);
    // Each reread channel follows channel(Master(x)) within two 8-bit codes
    // (export fold and import fit, one code each).
    for (channel, outer) in [("red", &red), ("green", &diagonal), ("blue", &diagonal)] {
        for code in 0..=255 {
            let x = f64::from(code) / 255.0;
            let expected = polyline(outer, polyline(&master, x));
            let reread = polyline(&curves[channel], x);
            assert!(
                (reread - expected).abs() * 255.0 <= 2.0,
                "{channel} {code}: {reread} vs {expected}"
            );
        }
    }
}
