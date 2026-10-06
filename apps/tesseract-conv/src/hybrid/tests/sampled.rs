use super::*;
use crate::formats::{export_after_effects, export_premiere};

#[test]
fn default_sampled_bake_reaches_direct_and_linked_check_and_write() {
    let parent = tempfile::tempdir().unwrap();
    let input = parent.path().join("scripts.tsrct");
    let mut value = document_value();
    value["composition"]["dynamics"] = json!({"entries": [{
        "target": {"kind":"layer", "layerId":10, "propertyType":"opacity"},
        "animator": {"type":"jsScript", "layerTimeJsCode":"return 50 + input.time.milliseconds / 100;"}
    }]});
    archive(&value, &input);
    let original = fs::read(&input).unwrap();
    for (name, handler) in [
        ("direct", export_after_effects as crate::formats::Handler),
        ("linked", export_premiere as crate::formats::Handler),
    ] {
        let output = parent.path().join(name);
        let mut conversion = request(&input, &output, ConversionMode::Check);
        conversion.fps = Some("25");
        let checked = handler(&conversion).unwrap();
        assert!(!output.exists());
        assert!(
            checked.diagnostics.iter().any(|d| d
                .message
                .contains("FX script baking samples at four times the native 25fps output rate")),
            "{name}: default sampling policy and output FPS must reach script preparation"
        );
        assert!(checked
            .diagnostics
            .iter()
            .any(|d| d.message.contains("sampled-grid fit validation")));
        conversion.mode = ConversionMode::Write;
        let written = handler(&conversion).unwrap();
        assert_eq!(checked, written);
        assert!(output
            .join(if name == "direct" {
                "project.aep"
            } else {
                "project.prproj"
            })
            .is_file());
    }
    assert_eq!(fs::read(&input).unwrap(), original);
}
