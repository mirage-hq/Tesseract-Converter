//! Converter-owned snapshots are dependencies, not live FX scripts.
use super::*;

fn text_dependency(code: &str) -> Model {
    let mut model = model("value", 1);
    let mut text = property(1, 1, "value", 1);
    text.identity = PropertyIdentity::SourceText {};
    text.initial.clear();
    text.keys.clear();
    text.expression = None;
    text.expression_enabled = false;
    text.text = Some("Native".into());
    text.text_style = Some([0.2, 0.4, 0.6, 1.0]);
    model.properties.push(text);
    model.comps[0].layers[0].source_text = Some(2);
    model.properties[1].expression = Some(syntax::compile(code).unwrap());
    model
}

#[test]
fn source_text_data_programs_tile_selected_words() {
    let mut model = text_dependency("value");
    model.properties[1].text = Some("Stored".into());
    model.properties[2].text = Some("First Second".into());
    model.properties[1].expression = Some(
        syntax::compile(
            r#"
        var source = thisComp.layer('subject').text.sourceText;
        function word(text, ordinal) {
            var pattern = /\S+/g, match, count = 0;
            while ((match = pattern.exec(text)) !== null) {
                if (count === ordinal) return {text: match[0], start: match.index};
                count++;
            }
            return null;
        }
        var selected = word(source.value, time < 0.5 ? 0 : 1);
        var result = '';
        if (selected && selected.text !== undefined) {
            for (var row = 0; row < 2; row++) {
                for (var column = 0; column < 2; column++) result += selected.text + '  ';
                if (row < 1) result += '\r';
            }
            source.getStyleAt(selected.start).setText(result);
        } else '';
    "#,
        )
        .unwrap(),
    );
    let mut prepared = prepare(&model).unwrap();
    let actual = evaluate_text_grid(&mut prepared, 1, &[0.0, 0.5]).unwrap();
    assert_eq!(
        actual,
        [
            "First  First  \rFirst  First  ",
            "Second  Second  \rSecond  Second  "
        ]
    );
    assert!(prepared.approximated.contains("sourceText:setText"));
}

#[test]
fn source_text_mutable_data_locals_capture_values_not_receivers() {
    let model = text_dependency(
        "var chosen=''; chosen=thisComp.layer('subject').text.sourceText; chosen.length",
    );
    assert_eq!(
        evaluate_grid(&mut prepare(&model).unwrap(), 1, &[0.0]).unwrap(),
        vec![vec![6.0]]
    );
    assert!(syntax::compile("var chosen=0; chosen=thisProperty; chosen.valueAtTime(0)").is_err());
}

#[test]
fn source_text_records_do_not_hide_api_receivers_or_writes() {
    for code in [
        "var record={text:thisLayer,start:0}; record.text.name",
        "var record={text:thisComp.layer('subject').text.sourceText,start:0}; record.text.length",
        "var record={text:time?thisLayer:0,start:0}; record.text",
        "var record={get text(){return thisLayer;}}; record.text",
        "var record={text:'a',start:0}; record.text='b'; record.text",
        "function a(p){return p.value;} function b(p){return p.value;} (time?a:b)(thisProperty)",
    ] {
        assert!(syntax::compile(code).is_err(), "{code}");
    }
    let model = text_dependency("var words=['a']; words[time ? 'constructor' : 0].length");
    assert!(evaluate_grid(&mut prepare(&model).unwrap(), 1, &[1.0]).is_err());
}

#[test]
fn source_text_catch_bindings_expose_error_messages_not_api_aliases() {
    let mut model = text_dependency("value");
    model.properties[1].text = Some("Stored".into());
    model.properties[1].expression =
        Some(syntax::compile("try { key(0); } catch (error) { error.message; }").unwrap());
    assert_eq!(
        evaluate_text_grid(&mut prepare(&model).unwrap(), 1, &[0.0]).unwrap(),
        ["invalid native key index"]
    );
    assert!(
        syntax::compile(
            "var receiver=thisLayer; try { key(0); } catch(receiver) { receiver.name; }"
        )
        .is_err()
    );
}

#[test]
fn source_text_optional_effect_defaults_require_a_complete_native_inventory() {
    let mut model = text_dependency("value");
    model.properties[1].text = Some("Stored".into());
    model.properties[1].expression = Some(
        syntax::compile("try { effect('optional')(1); } catch(error) { 'default'; }").unwrap(),
    );
    assert_eq!(
        evaluate_text_grid(&mut prepare(&model).unwrap(), 1, &[0.0]).unwrap(),
        ["default"]
    );
    model.comps[0].layers[1].effect_inventory_complete = false;
    assert!(evaluate_text_grid(&mut prepare(&model).unwrap(), 1, &[0.0]).is_err());
}

#[test]
fn source_text_literals_keep_escapes_and_unicode() {
    let mut model = text_dependency("value");
    model.properties[1].text = Some("Stored".into());
    model.properties[1].expression =
        Some(syntax::compile(r#"'Quote: \" \r\n\t\\ \u2028 😀'"#).unwrap());
    let actual = evaluate_text_grid(&mut prepare(&model).unwrap(), 1, &[0.0]).unwrap();
    assert_eq!(actual, ["Quote: \" \r\n\t\\ \u{2028} 😀"]);
}

#[test]
fn static_source_text_is_a_real_string_dependency() {
    let model = text_dependency("thisComp.layer('subject').text.sourceText.length");
    let actual = evaluate_grid(&mut prepare(&model).unwrap(), 1, &[0.0, 0.5]).unwrap();
    assert_eq!(actual, vec![vec![6.0], vec![6.0]]);
}

#[test]
fn source_text_native_hold_clock_and_style_are_read_together() {
    let mut model = text_dependency(
        "var s=thisComp.layer('subject').text.sourceText; s.length+s.style.fillColor[0]*10",
    );
    model.properties[2].text_keys = vec![
        model::TextKey {
            time: 0.25,
            text: "A".into(),
            fill_color: [0.1, 0.2, 0.3, 1.0],
        },
        model::TextKey {
            time: 0.75,
            text: "Long".into(),
            fill_color: [0.8, 0.2, 0.3, 1.0],
        },
    ];
    let mut prepared = prepare(&model).unwrap();
    let actual = evaluate_grid(&mut prepared, 1, &[0.0, 0.74, 0.75, 1.0]).unwrap();
    assert_eq!(actual, vec![vec![2.0], vec![2.0], vec![12.0], vec![12.0]]);
}

#[test]
fn source_text_value_string_methods_keep_native_hold_timing() {
    let mut model = text_dependency(
        "var textLayer=thisComp.layer('subject'); var text=textLayer.text.sourceText.value.toString(); text.split('|').length-1",
    );
    model.properties[2].text_keys = vec![
        model::TextKey {
            time: 0.25,
            text: "First|Second".into(),
            fill_color: [1.0; 4],
        },
        model::TextKey {
            time: 0.75,
            text: "One|Two|Three".into(),
            fill_color: [1.0; 4],
        },
    ];
    let actual = evaluate_grid(
        &mut prepare(&model).unwrap(),
        1,
        &[0.0, 0.25, 0.74, 0.75, 1.0],
    )
    .unwrap();
    assert_eq!(
        actual,
        vec![vec![1.0], vec![1.0], vec![1.0], vec![2.0], vec![2.0]]
    );
}

#[test]
fn cached_text_bounds_are_data_in_implicit_numeric_bindings() {
    let mut model = text_dependency(
        "s=thisComp.layer('subject').sourceRectAtTime(0,false); h=s.height; w=s.width; Math.max(h/20,w/100)*100",
    );
    model.properties[2].text_bounds = Some([1.0, -10.0, 42.0, 12.0]);
    let actual = evaluate_grid(&mut prepare(&model).unwrap(), 1, &[0.0]).unwrap();
    assert_eq!(actual, vec![vec![60.0]]);
    for code in [
        "var s=thisComp.layer('subject').sourceRectAtTime(0,false); s.width",
        "s=thisComp.layer('subject').sourceRectAtTime(0,false); s=thisComp.layer('subject').sourceRectAtTime(1,false); s.width",
    ] {
        model.properties[1].expression = Some(syntax::compile(code).unwrap());
        assert_eq!(
            evaluate_grid(&mut prepare(&model).unwrap(), 1, &[0.0]).unwrap(),
            vec![vec![42.0]],
            "{code}"
        );
    }
    assert!(syntax::compile("s=sourceRectAtTime(0,false); s.width").is_ok());
    for code in [
        "s=sourceRectAtTime(0,false); s.missing",
        "s=sourceRectAtTime(0,false); s.width=10; s.width",
        "var s=thisLayer; s=sourceRectAtTime(0,false); s.width",
        "var s=thisLayer; s=thisComp.layer('subject'); s.width",
        "thisComp.layer('subject').text.sourceText.style.fontSize",
    ] {
        assert!(syntax::compile(code).is_err(), "{code}");
    }
}

#[test]
fn source_text_errors_cannot_be_hidden_by_an_expression_fallback() {
    let mut model = text_dependency(
        "try { thisComp.layer('subject').text.sourceText.length; } catch (error) { 7; }",
    );
    model.properties[2].error = Some("invalid native document clock".into());
    assert!(evaluate_grid(&mut prepare(&model).unwrap(), 1, &[0.0]).is_err());
}

#[test]
fn cached_text_bounds_are_diagnosed_and_do_not_mask_unknown_style_fields() {
    let mut model = text_dependency("thisComp.layer('subject').sourceRectAtTime(0,false).width");
    model.properties[2].text_bounds = Some([1.0, -10.0, 42.0, 12.0]);
    let mut prepared = prepare(&model).unwrap();
    let actual = evaluate_grid(&mut prepared, 1, &[0.0]).unwrap();
    assert_eq!(actual, vec![vec![42.0]]);
    assert!(
        prepared
            .approximated
            .iter()
            .any(|api| api.contains("text-cache"))
    );
    assert!(
        syntax::compile("thisComp.layer('subject').text.sourceText.style.fontSize || 12",).is_err()
    );
}
