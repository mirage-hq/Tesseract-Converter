//! Direct same-composition Source Text aliases copy strings, not TextStyle or cached layout.

use super::*;
use crate::structure::Composition;

pub(super) fn parse(mut text: &str) -> Option<&str> {
    control_links::token(&mut text, "thisComp.layer")?;
    control_links::token(&mut text, "(")?;
    let name = control_links::quoted(&mut text)?;
    control_links::token(&mut text, ")")?;
    control_links::token(&mut text, ".text.sourceText")?;
    (!name.is_empty() && control_links::finished(text)).then_some(name)
}

fn descriptor(layer: &Layer) -> Result<Option<&[Chunk]>, TextError> {
    let roots = root_runs(&layer.content)?;
    let text_group = roots
        .iter()
        .find(|(name, _)| *name == "ADBE Text Properties")
        .ok_or(TextError::Layout("missing Text properties"))?;
    let properties = runs(unique_list(text_group.1, *b"tdgp")?)?;
    let document = properties
        .iter()
        .find(|(name, _)| *name == "ADBE Text Document")
        .ok_or(TextError::Layout("missing Source Text property"))?;
    enabled_expression_descriptor(unique_list(document.1, *b"btds")?)
}

fn uniform_styles(document: &Value, kind: &str) -> bool {
    let Some(runs) =
        at(document, &[Key::Name("0"), Key::Name(kind), Key::Name("0")]).and_then(Value::as_array)
    else {
        return false;
    };
    let path = [Key::Name("0"), Key::Name("0"), Key::Name(kind)];
    let Some(first) = runs.first().and_then(|run| at(run, &path)) else {
        return false;
    };
    runs.iter().all(|run| at(run, &path) == Some(first))
}

// Admit only explicit justification codes with an exact existing FX mapping.
// Other native paragraph controls remain lossy; never infer absent defaults.
pub(super) fn uniform_mapped_paragraphs(document: &Value) -> bool {
    let Some(runs) =
        at(document, &[Key::Name("0"), Key::Name("5"), Key::Name("0")]).and_then(Value::as_array)
    else {
        return false;
    };
    let path = [
        Key::Name("0"),
        Key::Name("0"),
        Key::Name("5"),
        Key::Name("0"),
    ];
    let code = runs
        .first()
        .and_then(|run| at(run, &path))
        .and_then(Value::as_i64);
    matches!(code, Some(0..=3))
        && runs
            .iter()
            .all(|run| at(run, &path).and_then(Value::as_i64) == code)
}

fn resolve(
    layer: &Layer,
    composition: &Composition,
    source: &SourceText,
) -> Result<Option<(String, u32)>, String> {
    let Some(metadata) = descriptor(layer).map_err(|error| error.to_string())? else {
        return Ok(None);
    };
    let expression = control_links::expression(metadata).map_err(|error| error.to_string())?;
    let Some(name) = parse(expression) else {
        return Ok(None);
    };
    let flags = only_data(metadata, *b"tdb4").map_err(|error| error.to_string())?;
    if source.documents.len() != 1
        || flags[68] != 0
        || metadata
            .iter()
            .any(|chunk| chunk.list_kind() == Some(*b"list"))
    {
        return Err("consumer must have one unkeyed Source Text document".into());
    }
    let document = &source.documents[0];
    let (editable, _) = convert_document(document, &source.fonts, source.frame.as_ref());
    if editable.box_text || !uniform_styles(document, "6") || !uniform_mapped_paragraphs(document) {
        return Err("consumer box or mixed-run layout cannot reuse cached text geometry".into());
    }
    if source.path_options.iter().any(|(name, value)| {
        name == "ADBE Text Path"
            && (value.as_ref().map_or(true, numeric_has_keyframes)
                || scalar_property_value(&source.path_options, name) != Some(0.0))
    }) {
        return Err("consumer text-path ownership is unsupported for a content alias".into());
    }
    let mut targets = composition
        .layers
        .iter()
        .filter(|target| target.name.as_ref() == name);
    let target = targets
        .next()
        .ok_or_else(|| format!("target {name:?} is missing"))?;
    if targets.next().is_some() || target.record.id() == layer.record.id() {
        return Err(format!("target {name:?} is ambiguous or self-referential"));
    }
    let target_source = read_source(target)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("target {name:?} is not text"))?;
    if !target_source.document_is_static || target_source.documents.len() != 1 {
        return Err(format!("target {name:?} is keyed or expression-backed"));
    }
    let text = at(
        &target_source.documents[0],
        &[Key::Name("0"), Key::Name("0")],
    )
    .and_then(Value::as_str)
    .ok_or_else(|| format!("target {name:?} has no decoded string"))?;
    Ok(Some((text.to_owned(), target.record.id())))
}

pub(super) fn lower(layer: &Layer, composition: &Composition, source: &mut SourceText) {
    let resolved = match resolve(layer, composition, source) {
        Ok(Some(resolved)) => resolved,
        Ok(None) => return,
        Err(reason) => {
            source.warnings.push(format!(
                "Source Text direct content alias not lowered ({reason}); authored content retained"
            ));
            return;
        }
    };
    if at(
        &source.documents[0],
        &[Key::Name("0"), Key::Name("5"), Key::Name("0")],
    )
    .and_then(Value::as_array)
    .is_some_and(|runs| {
        runs.iter().any(|run| {
            matches!(
                at(run, &[Key::Name("0"), Key::Name("0"), Key::Name("5")]),
                Some(Value::Dict(fields)) if fields.keys().any(|field| field != "0")
            )
        })
    }) {
        source.warnings.push(
            "Source Text direct content alias: unmapped paragraph fields, including unknown COS controls, are omitted by the existing consumer mapper; only explicit justification is preserved, without inferring native defaults".into(),
        );
    }
    if !uniform_styles(&source.documents[0], "5") {
        source.warnings.push(
            "Source Text direct content alias: native paragraph controls differ between runs; only uniform explicit justification is mapped, and unmapped paragraph differences are approximated using the existing consumer style".into(),
        );
    }
    // Only replace the decoded string. Consumer style stays local; no target COS
    // layout, font table, glyph bounds or text animator is transplanted.
    let Value::Dict(document) = &mut source.documents[0] else {
        return;
    };
    let Some(Value::Dict(text)) = document.get_mut("0") else {
        return;
    };
    if !matches!(text.get("0"), Some(Value::String(_))) {
        return;
    }
    text.insert("0".into(), Value::String(resolved.0));
    source.frame = None;
    source
        .warnings
        .retain(|warning| !warning.starts_with("Source Text: enabled expression not lowered"));
    source.warnings.push(format!(
        "Source Text direct content alias to layer {} copied into editable text with consumer style; live linkage and unsupported animator programs are not reconstructed, and cached glyph layout is not reused",
        resolved.1
    ));
    // Keep document_is_static=false: resolving content does not admit the old
    // cached caption-width/geometry profile for this expression owner.
}
