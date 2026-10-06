//! Recover known Film Impact controls independently of UI serialization.
mod rules;
use crate::{
    error::{ensure, unsupported, Result},
    format::{graph::Element, Graph, Record},
    schema::native::VideoFilterComponent,
    {approximate, Omission},
};
pub(super) use rules::{Rule, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy)]
pub(super) struct ProfileParam {
    pub(super) id: &'static str,
    pub(super) kind: &'static str,
    pub(super) name: Option<&'static str>,
    pub(super) control: Option<&'static str>,
    pub(super) rule: Rule,
    pub(super) lower: Option<&'static str>,
    pub(super) upper: Option<&'static str>,
    pub(super) discontinuous: Option<&'static str>,
}

pub(super) const fn scalar(
    id: &'static str,
    name: Option<&'static str>,
    rule: Rule,
    lower: Option<&'static str>,
    upper: Option<&'static str>,
) -> ProfileParam {
    ProfileParam {
        id,
        kind: "VideoComponentParam",
        name,
        control: None,
        rule,
        lower,
        upper,
        discontinuous: None,
    }
}
pub(super) const fn ui(
    id: &'static str,
    name: Option<&'static str>,
    control: &'static str,
) -> ProfileParam {
    ProfileParam {
        control: Some(control),
        upper: if matches!(control.as_bytes(), [b'1', b'1'] | [b'1', b'2']) {
            Some("false")
        } else {
            None
        },
        ..scalar(id, name, Rule::Inert, None, None)
    }
}
pub(super) const fn popup(
    id: &'static str,
    name: &'static str,
    value: f64,
    upper: &'static str,
) -> ProfileParam {
    ProfileParam {
        discontinuous: Some("true"),
        ..scalar(id, Some(name), Rule::Number(value), Some("0"), Some(upper))
    }
}
pub(super) const fn point(id: &'static str, name: &'static str, value: [f64; 2]) -> ProfileParam {
    ProfileParam {
        kind: "PointComponentParam",
        ..scalar(id, Some(name), Rule::Point(value), None, None)
    }
}
pub(super) const fn version(id: &'static str) -> ProfileParam {
    scalar(
        id,
        Some("_ Applied Version"),
        Rule::Version,
        Some("0"),
        Some("999999"),
    )
}

// UI rules ignore expansion values, never opaque payloads or new controls.
pub(super) const ERROR_OCCURRED: ProfileParam = ProfileParam {
    control: Some("16"),
    ..scalar(
        "8100",
        Some("Error occurred"),
        Rule::Boolean(false),
        None,
        None,
    )
};
pub(super) const TRANSITION_TIMING_8120: ProfileParam = ui("8120", Some("Transition Timing"), "11");
pub(super) const START: ProfileParam = scalar(
    "2",
    Some("Start"),
    Rule::Number(0.0),
    Some("0"),
    Some("100"),
);
pub(super) const END: ProfileParam = scalar(
    "3",
    Some("End"),
    Rule::Number(100.0),
    Some("0"),
    Some("100"),
);
pub(super) const TRANSITION_TIMING_8121: ProfileParam = ui("8121", Some("Transition Timing"), "12");
pub(super) const CONTROL_8040: ProfileParam = ui("8040", None, "16");
pub(super) const VISUAL_CURVE_EDITOR_8020: ProfileParam =
    ui("8020", Some("Visual Curve Editor"), "11");
pub(super) const CURVE_GRAPH: ProfileParam = ProfileParam {
    kind: "ArbVideoComponentParam",
    control: Some("9"),
    ..scalar(
        "8022",
        Some("Curve Graph"),
        Rule::EmptyCurveGraph,
        None,
        None,
    )
};
pub(super) const VISUAL_CURVE_EDITOR_8021: ProfileParam =
    ui("8021", Some("Visual Curve Editor"), "12");
pub(super) const OVERLAY_MODE: ProfileParam = popup("8280", "_ Overlay Mode", 0.0, "2");
pub(super) const OVERLAY_INFO: ProfileParam = scalar(
    "8281",
    Some("_ Overlay Info"),
    Rule::Boolean(false),
    None,
    None,
);
pub(super) const CONTROL_8141: ProfileParam = ui("8141", None, "16");
pub(super) const CONTROL_8300: ProfileParam =
    scalar("8300", None, Rule::Number(0.0), Some("0"), Some("16777215"));
pub(super) const CONTROL_8301: ProfileParam =
    scalar("8301", None, Rule::Number(0.0), Some("0"), Some("16777215"));
pub(super) const SOURCE_B_LAYER: ProfileParam = scalar(
    "9000",
    Some("_ Source B Layer"),
    Rule::Number(4294967293.0),
    None,
    None,
);
pub(super) const OVERLAY_ENABLED: ProfileParam = scalar(
    "9020",
    Some("_ Overlay Enabled"),
    Rule::Boolean(false),
    None,
    None,
);
pub(super) const SEQUENCE_WIDTH: ProfileParam = scalar(
    "9040",
    Some("_ Sequence Width"),
    Rule::Number(-1.0),
    Some("-1"),
    Some("1000000000"),
);
pub(super) const SEQUENCE_HEIGHT: ProfileParam = scalar(
    "9041",
    Some("_ Sequence Height"),
    Rule::Number(-1.0),
    Some("-1"),
    Some("1000000000"),
);
pub(super) const SEQUENCE_PIXEL_RATIO: ProfileParam = scalar(
    "9042",
    Some("_ Sequence Pixel Ratio"),
    Rule::Number(-1.0),
    Some("-1"),
    Some("1000000000"),
);

/// Recover supported controls; ambiguous bindings and active opaque coverage
/// are not treated as decorative metadata.
pub(super) fn read_profile_with(
    graph: &Graph<'_>,
    record: Record<'_>,
    match_name: &str,
    profile: &[ProfileParam],
    omissions: &mut Vec<Omission>,
) -> Result<BTreeMap<&'static str, Value>> {
    let root = record.element();
    ensure!(
        root.child("MatchName").and_then(Element::text) == Some(match_name),
        "conflicting Film Impact effect identity"
    );
    let component = root
        .child("Component")
        .ok_or_else(|| unsupported("missing Film Impact Component"))?;
    match component.child("Bypass").and_then(Element::text) {
        None | Some("false") => {}
        Some("true") => {
            return Err(unsupported(
                "bypassed Film Impact feature omitted; picture retained",
            ))
        }
        Some(_) => {
            return Err(unsupported(
                "ambiguous Film Impact Bypass flag; picture retained without this feature",
            ))
        }
    }
    for element in [root, component] {
        let mut fields = BTreeSet::new();
        for child in element.children() {
            if !fields.insert(child.tag()) {
                ensure!(
                    !matches!(child.tag(), "Component" | "Params" | "MatchName" | "Bypass"),
                    "ambiguous duplicate Film Impact component binding"
                );
                approximate(omissions, record.identity(), format!("duplicate decorative Film Impact {} field ignored; known controls retained", child.tag()));
            }
        }
        ensure!(
            element
                .attributes()
                .all(|name| !matches!(name, "ObjectRef" | "ObjectURef")),
            "ambiguous Film Impact component binding"
        );
        for child in element.children() {
            if ![
                "Component",
                "MatchName",
                "VideoFilterType",
                "Node",
                "Params",
                "ID",
                "DisplayName",
                "Bypass",
                "Intrinsic",
                "ArchivedType",
                "Unique",
            ]
            .contains(&child.tag())
            {
                approximate(
                    omissions,
                    record.identity(),
                    format!(
                        "Film Impact field {} was not reproduced; known controls retained",
                        child.tag()
                    ),
                );
            }
        }
    }
    let filter = graph.decode::<VideoFilterComponent>(record)?;
    let params = filter
        .value
        .component
        .and_then(|c| c.params)
        .ok_or_else(|| unsupported("missing Film Impact Params"))?;
    let mut values = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for reference in &params.items {
        let param = graph.locate(reference, &filter.identity)?;
        let e = param.element();
        let id = e
            .child("ParameterID")
            .and_then(Element::text)
            .ok_or_else(|| unsupported("missing Film Impact control identity"))?;
        ensure!(
            seen.insert(id.to_owned()),
            "duplicate Film Impact control identity"
        );
        ensure!(
            e.attributes()
                .all(|name| !matches!(name, "ObjectRef" | "ObjectURef")),
            "ambiguous Film Impact control binding"
        );
        let Some(spec) = profile.iter().find(|spec| spec.id == id) else {
            approximate(
                omissions,
                param.identity(),
                format!(
                    "unknown Film Impact control {id} was not reproduced; known controls retained"
                ),
            );
            continue;
        };
        ensure!(
            param.tag() == spec.kind,
            "conflicting Film Impact control class"
        );
        if matches!(spec.rule, Rule::Inert) {
            continue;
        }
        if matches!(spec.rule, Rule::EmptyCurveGraph) {
            ensure!(
                !has_opaque_curve(e),
                "active opaque Film Impact Curve Graph has no editable mapping"
            );
            continue;
        }
        let mut fields = BTreeSet::new();
        let mut ambiguous_value = false;
        for child in e.children() {
            if !fields.insert(child.tag()) {
                ensure!(
                    !matches!(
                        child.tag(),
                        "StartKeyframe" | "CurrentValue" | "ParameterID" | "Keyframes"
                    ),
                    "ambiguous duplicate Film Impact control value binding"
                );
                approximate(
                    omissions,
                    param.identity(),
                    format!(
                        "duplicate Film Impact {} metadata ignored; known control retained",
                        child.tag()
                    ),
                );
            }
            if child.tag() == "ParameterID" {
                ensure!(
                    child.is_text_only() && child.attributes().next().is_none(),
                    "ambiguous Film Impact control identity binding"
                );
            } else if ["StartKeyframe", "CurrentValue"].contains(&child.tag())
                && (!child.is_text_only() || child.attributes().next().is_some())
            {
                // Do not interpret only the first text fragment as a complete
                // authored number. Recover this control, not the whole profile.
                ambiguous_value = true;
            }
        }
        if e.child("Keyframes").is_some()
            || e.child("IsTimeVarying").and_then(Element::text) == Some("true")
        {
            approximate(
                omissions,
                param.identity(),
                "Film Impact animation was not reproduced; usable authored base control retained",
            );
        }
        for (name, expected) in [
            ("Name", spec.name),
            ("ParameterControlType", spec.control),
            ("LowerBound", spec.lower),
            ("UpperBound", spec.upper),
            ("DiscontinuousInterpolate", spec.discontinuous),
        ] {
            if e.child(name).and_then(Element::text) != expected {
                approximate(omissions, param.identity(),
                    format!("Film Impact {name} metadata differed; control identity and usable value retained"));
            }
        }
        let parsed = if ambiguous_value {
            Some(Err(unsupported(
                "ambiguous Film Impact control value binding",
            )))
        } else {
            e.child("StartKeyframe")
                .and_then(Element::text)
                .map(|text| spec.rule.key(text, spec.kind == "PointComponentParam"))
                .or_else(|| {
                    e.child("CurrentValue")
                        .and_then(Element::text)
                        .map(|text| spec.rule.value(text))
                })
        };
        let value = match parsed {
            Some(Ok(value)) => Some(value),
            failed => {
                // Hide Source is coverage, not a UI preference. Do not recover
                // it to a visible-source default that would reveal pixels.
                ensure!(!(match_name == "AE.Impact_Stroke_FX" && id == "11"),
                    "Stroke Hide Source cannot be represented by the visible-source geometry recipe");
                let reason = failed
                    .and_then(Result::err)
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "missing authored control".into());
                approximate(omissions, param.identity(),
                    format!("Film Impact control {id} approximated using the known template default: {reason}; other controls retained"));
                spec.rule.default_value()
            }
        };
        if let Some(value) = value {
            values.insert(spec.id, value);
        }
    }
    for spec in profile {
        if !values.contains_key(spec.id)
            && !matches!(spec.rule, Rule::Inert | Rule::EmptyCurveGraph)
        {
            if let Some(value) = spec.rule.default_value() {
                approximate(omissions, record.identity(),
                    format!("missing Film Impact control {} used its known template default; other controls retained", spec.id));
                values.insert(spec.id, value);
            }
        }
    }
    Ok(values)
}

/// Serialized curve payload is semantic; expansion UI/hash labels alone are not.
pub(super) fn has_opaque_curve(element: Element<'_>) -> bool {
    element.children().any(|child| {
        matches!(
            child.tag(),
            "Keyframes" | "StartKeyframe" | "CurrentValue" | "CustomCurve" | "BinaryData"
        ) || (matches!(child.tag(), "Node" | "Properties") && has_opaque_curve(child))
    })
}
