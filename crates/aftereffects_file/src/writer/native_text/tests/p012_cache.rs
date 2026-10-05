use super::*;

#[test]
fn p012_box_text_omits_empty_native_cache_without_dropping_authored_content() {
    // P012's full native render aborted in TDB_Stream::CloneCache with an
    // explicit empty document layout cache. Its absence requests native layout
    // construction; no original glyph/cache records are reused.
    for (id, text, size, bounds) in [
        (33, "Heat, color grade and film grain", 18.0, [424.0, 32.4]),
        (3, "Flaming heart", 56.0, [1500.0, 100.8]),
    ] {
        let mut input = spec(text);
        input.font_postscript = "HelveticaNeue-Regular".into();
        input.font_size = size;
        input.box_size = Some(bounds);
        input.box_position = Some([0.0, 0.0]);
        input.leading = None;
        input.tracking = 0.0;
        let group = boxed_properties(&timeline(input), id, PropertyClock::DEFAULT).unwrap();
        let parsed = cos::parse(payload(&group)).unwrap();
        let doc = document(&parsed, 0);
        assert!(at(doc, &["1"]).get("2").is_none());
        assert_eq!(
            at(doc, &["0", "0"]).as_str(),
            Some(format!("{text}\r").as_str())
        );
        assert_eq!(run_style(doc, "6").get("1").unwrap().as_f64(), Some(size));
        assert_eq!(run_style(doc, "6").get("8").unwrap().as_i64(), Some(0));
        let rectangle = at(&parsed, &["0", "8", "0"]).index(0).unwrap();
        let vertices = at(rectangle, &["0", "1", "0"]).as_array().unwrap();
        assert_eq!(vertices[12].as_f64(), Some(bounds[0]));
        assert_eq!(vertices[13].as_f64(), Some(bounds[1]));
    }
}
