use super::*;

#[test]
fn direct_source_text_alias_parser_rejects_style_and_programs() {
    assert_eq!(
        source_alias::parse(r#"thisComp.layer("Thank You").text.sourceText"#),
        Some("Thank You")
    );
    assert_eq!(
        source_alias::parse(" thisComp.layer('Title').text.sourceText; "),
        Some("Title")
    );
    for expression in [
        r#"thisComp.layer("Title").text.sourceText.style"#,
        r#"thisComp.layer("Title").text.sourceText.value"#,
        r#"thisComp.layer("Title").text.sourceText.setText("Other")"#,
        r#"thisComp.layer("Title").text.sourceText + "Other""#,
        r#"thisComp.layer("Title").text.sourceText; value"#,
        r#"thisComp.layer(1).text.sourceText"#,
        r#"thisComp.layer("").text.sourceText"#,
    ] {
        assert!(source_alias::parse(expression).is_none(), "{expression}");
    }
}

#[test]
fn direct_source_text_alias_paragraph_guard_requires_explicit_equal_mapped_codes() {
    // Native COS nesting, with deliberately differing unmapped paragraph records.
    for (records, admitted) in [
        (
            "<< /0 << /0 << /5 << /0 2 >> >> >> >> << /0 << /0 << /5 << /0 2 /24 0 >> >> >> >>",
            true,
        ),
        (
            "<< /0 << /0 << /5 << /0 2 >> >> >> >> << /0 << /0 << /5 << /0 1 >> >> >> >>",
            false,
        ),
        (
            "<< /0 << /0 << /5 << /0 2 >> >> >> >> << /0 << /0 << /5 << /24 0 >> >> >> >>",
            false,
        ),
        ("<< /0 << /0 << /5 << /0 4 >> >> >> >>", false),
        ("<< /0 << /0 << /5 << /0 (2) >> >> >> >>", false),
        ("", false),
    ] {
        let encoded = format!("<< /0 << /5 << /0 [{records}] >> >> >>");
        let document = cos::parse(encoded.as_bytes()).unwrap();
        assert_eq!(
            source_alias::uniform_mapped_paragraphs(&document),
            admitted,
            "{records}"
        );
    }
}
