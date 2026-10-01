//! Full-record validation and identical controls shared by measured Film Impact profiles.
use super::effects::only_children;
use crate::{
    error::{ensure, unsupported, Result},
    format::{graph::Element, Graph, Record},
    schema::{native::VideoFilterComponent, records},
};
use std::collections::BTreeSet;

pub(super) struct ProfileParam {
    pub(super) id: &'static str,
    pub(super) kind: &'static str,
    pub(super) name: Option<&'static str>,
    pub(super) control: Option<&'static str>,
    pub(super) start: Option<&'static str>,
    pub(super) lower: Option<&'static str>,
    pub(super) upper: Option<&'static str>,
    pub(super) discontinuous: Option<&'static str>,
}

// These complete records are identical in at least two measured profiles.
pub(super) const ERROR_OCCURRED: ProfileParam = ProfileParam {
    id: "8100",
    kind: "VideoComponentParam",
    name: Some("Error occurred"),
    control: Some("16"),
    start: Some("-91445760000000000,false,0,0,0,0,0,0"),
    lower: None,
    upper: None,
    discontinuous: None,
};

pub(super) const TRANSITION_TIMING_8120: ProfileParam = ProfileParam {
    id: "8120",
    kind: "VideoComponentParam",
    name: Some("Transition Timing"),
    control: Some("11"),
    start: Some("-91445760000000000,false,0,0,0,0,0,0"),
    lower: None,
    upper: Some("false"),
    discontinuous: None,
};

pub(super) const START: ProfileParam = ProfileParam {
    id: "2",
    kind: "VideoComponentParam",
    name: Some("Start"),
    control: None,
    start: Some("-91445760000000000,0.,0,0,0,0,0,0"),
    lower: Some("0"),
    upper: Some("100"),
    discontinuous: None,
};

pub(super) const END: ProfileParam = ProfileParam {
    id: "3",
    kind: "VideoComponentParam",
    name: Some("End"),
    control: None,
    start: Some("-91445760000000000,100.,0,0,0,0,0,0"),
    lower: Some("0"),
    upper: Some("100"),
    discontinuous: None,
};

pub(super) const TRANSITION_TIMING_8121: ProfileParam = ProfileParam {
    id: "8121",
    kind: "VideoComponentParam",
    name: Some("Transition Timing"),
    control: Some("12"),
    start: Some("-91445760000000000,false,0,0,0,0,0,0"),
    lower: None,
    upper: Some("false"),
    discontinuous: None,
};

pub(super) const CONTROL_8040: ProfileParam = ProfileParam {
    id: "8040",
    kind: "VideoComponentParam",
    name: None,
    control: Some("16"),
    start: Some("-91445760000000000,false,0,0,0,0,0,0"),
    lower: None,
    upper: None,
    discontinuous: None,
};

pub(super) const VISUAL_CURVE_EDITOR_8020: ProfileParam = ProfileParam {
    id: "8020",
    kind: "VideoComponentParam",
    name: Some("Visual Curve Editor"),
    control: Some("11"),
    start: Some("-91445760000000000,false,0,0,0,0,0,0"),
    lower: None,
    upper: Some("false"),
    discontinuous: None,
};

pub(super) const CURVE_GRAPH: ProfileParam = ProfileParam {
    id: "8022",
    kind: "ArbVideoComponentParam",
    name: Some("Curve Graph"),
    control: Some("9"),
    start: None,
    lower: None,
    upper: None,
    discontinuous: None,
};

pub(super) const VISUAL_CURVE_EDITOR_8021: ProfileParam = ProfileParam {
    id: "8021",
    kind: "VideoComponentParam",
    name: Some("Visual Curve Editor"),
    control: Some("12"),
    start: Some("-91445760000000000,false,0,0,0,0,0,0"),
    lower: None,
    upper: Some("false"),
    discontinuous: None,
};

pub(super) const OVERLAY_MODE: ProfileParam = ProfileParam {
    id: "8280",
    kind: "VideoComponentParam",
    name: Some("_ Overlay Mode"),
    control: None,
    start: Some("-91445760000000000,0,0,0,0,0,0,0"),
    lower: Some("0"),
    upper: Some("2"),
    discontinuous: Some("true"),
};

pub(super) const OVERLAY_INFO: ProfileParam = ProfileParam {
    id: "8281",
    kind: "VideoComponentParam",
    name: Some("_ Overlay Info"),
    control: None,
    start: Some("-91445760000000000,false,0,0,0,0,0,0"),
    lower: None,
    upper: None,
    discontinuous: None,
};

pub(super) const CONTROL_8141: ProfileParam = ProfileParam {
    id: "8141",
    kind: "VideoComponentParam",
    name: None,
    control: Some("16"),
    start: Some("-91445760000000000,false,0,0,0,0,0,0"),
    lower: None,
    upper: None,
    discontinuous: None,
};

pub(super) const CONTROL_8300: ProfileParam = ProfileParam {
    id: "8300",
    kind: "VideoComponentParam",
    name: None,
    control: None,
    start: Some("-91445760000000000,0.,0,0,0,0,0,0"),
    lower: Some("0"),
    upper: Some("16777215"),
    discontinuous: None,
};

pub(super) const CONTROL_8301: ProfileParam = ProfileParam {
    id: "8301",
    kind: "VideoComponentParam",
    name: None,
    control: None,
    start: Some("-91445760000000000,0.,0,0,0,0,0,0"),
    lower: Some("0"),
    upper: Some("16777215"),
    discontinuous: None,
};

pub(super) const SOURCE_B_LAYER: ProfileParam = ProfileParam {
    id: "9000",
    kind: "VideoComponentParam",
    name: Some("_ Source B Layer"),
    control: None,
    start: Some("-91445760000000000,4294967293,0,0,0,0,0,0"),
    lower: None,
    upper: None,
    discontinuous: None,
};

pub(super) const OVERLAY_ENABLED: ProfileParam = ProfileParam {
    id: "9020",
    kind: "VideoComponentParam",
    name: Some("_ Overlay Enabled"),
    control: None,
    start: Some("-91445760000000000,false,0,0,0,0,0,0"),
    lower: None,
    upper: None,
    discontinuous: None,
};

pub(super) const SEQUENCE_WIDTH: ProfileParam = ProfileParam {
    id: "9040",
    kind: "VideoComponentParam",
    name: Some("_ Sequence Width"),
    control: None,
    start: Some("-91445760000000000,-1.,0,0,0,0,0,0"),
    lower: Some("-1"),
    upper: Some("1000000000"),
    discontinuous: None,
};

pub(super) const SEQUENCE_HEIGHT: ProfileParam = ProfileParam {
    id: "9041",
    kind: "VideoComponentParam",
    name: Some("_ Sequence Height"),
    control: None,
    start: Some("-91445760000000000,-1.,0,0,0,0,0,0"),
    lower: Some("-1"),
    upper: Some("1000000000"),
    discontinuous: None,
};

pub(super) const SEQUENCE_PIXEL_RATIO: ProfileParam = ProfileParam {
    id: "9042",
    kind: "VideoComponentParam",
    name: Some("_ Sequence Pixel Ratio"),
    control: None,
    start: Some("-91445760000000000,-1.,0,0,0,0,0,0"),
    lower: Some("-1"),
    upper: Some("1000000000"),
    discontinuous: None,
};

fn same_value(actual: &str, expected: &str) -> bool {
    // Packed native ARGB values exceed f64's exact-integer range. Comparing
    // them as floats would silently accept changed colour bits.
    if let Ok(integer) = expected.parse::<u64>() {
        if integer > (1_u64 << 53) {
            return actual.parse::<u64>() == Ok(integer);
        }
    }
    actual == expected
        || matches!((actual.parse::<f64>(), expected.parse::<f64>()),
        (Ok(a), Ok(b)) if a.is_finite() && b.is_finite() && a == b)
}

fn field(element: Element<'_>, name: &str, expected: Option<&str>) -> Result<()> {
    let child = element.child(name);
    ensure!(
        child.is_none_or(|child| child.is_text_only() && child.attributes().next().is_none()),
        "non-text dissolve {name}"
    );
    ensure!(
        match (child, expected) {
            (None, None) => true,
            (Some(child), Some(expected)) => child
                .text()
                .is_some_and(|actual| same_value(actual, expected)),
            _ => false,
        },
        "nondefault Film Impact dissolve {name}"
    );
    Ok(())
}

/// Shared static-control validation for measured Film Impact profiles.
pub(super) fn read_profile_with(
    graph: &Graph<'_>,
    record: Record<'_>,
    match_name: &str,
    profile: &[ProfileParam],
) -> Result<()> {
    let root = record.element();
    only_children(
        root,
        &["Component", "MatchName", "VideoFilterType"],
        "transition ",
    )?;
    field(root, "MatchName", Some(match_name))?;
    field(root, "VideoFilterType", Some("2"))?;
    let component = root
        .child("Component")
        .ok_or_else(|| unsupported("missing transition Component"))?;
    only_children(
        component,
        &[
            "Node",
            "Params",
            "ID",
            "DisplayName",
            "Bypass",
            "Intrinsic",
            "ArchivedType",
            "Unique",
        ],
        "transition Component/",
    )?;
    for (name, expected) in [
        ("Bypass", "false"),
        ("Intrinsic", "false"),
        ("ArchivedType", "0"),
        ("Unique", "false"),
    ] {
        if component.child(name).is_some() {
            field(component, name, Some(expected))?;
        }
    }
    if let Some(node) = component.child("Node") {
        read_expansion_ui(node, "ECP.Filter.Expanded")?;
    }

    for element in [root, component] {
        let mut fields = BTreeSet::new();
        ensure!(
            element.children().all(|child| fields.insert(child.tag())),
            "duplicate transition component field"
        );
        ensure!(
            element
                .attributes()
                .all(|name| ["ObjectID", "ClassID", "Version"].contains(&name)),
            "private transition component reference"
        );
    }
    let params_element = component
        .child("Params")
        .ok_or_else(|| unsupported("missing transition Params"))?;
    only_children(params_element, &["Param"], "transition Params/")?;
    ensure!(
        params_element.attributes().all(|name| name == "Version")
            && params_element
                .children()
                .all(|child| child
                    .attributes()
                    .all(|name| ["Index", "ObjectRef"].contains(&name))
                    && child.children().next().is_none()),
        "unexpected transition parameter binding"
    );
    let filter = graph.decode::<VideoFilterComponent>(record)?;
    let params = filter
        .value
        .component
        .and_then(|c| c.params)
        .ok_or_else(|| unsupported("missing transition Params"))?;
    ensure!(
        params.items.len() == profile.len(),
        "unexpected Film Impact control count"
    );
    let mut seen = BTreeSet::new();
    for (index, reference) in params.items.iter().enumerate() {
        ensure!(
            reference
                .index
                .as_deref()
                .is_none_or(|value| value.parse::<usize>().ok() == Some(index)),
            "conflicting transition parameter Index"
        );
        let param = graph.locate(reference, &filter.identity)?;
        let e = param.element();
        ensure!(
            e.attributes()
                .all(|name| ["ObjectID", "ClassID", "Version"].contains(&name)),
            "referenced or private transition control"
        );
        let id = e.child("ParameterID").and_then(Element::text);
        let spec = profile
            .iter()
            .find(|p| Some(p.id) == id)
            .ok_or_else(|| unsupported("unknown Film Impact transition control"))?;
        ensure!(
            seen.insert(spec.id) && param.tag() == spec.kind,
            "duplicate or unexpected Film Impact transition control"
        );
        let allowed = [
            "Node",
            "Name",
            "ParameterID",
            "ParameterControlType",
            "StartKeyframe",
            "StartKeyframePosition",
            "LowerBound",
            "UpperBound",
            "DiscontinuousInterpolate",
            "IsTimeVarying",
            "CurrentValue",
            "Keyframes",
        ];
        only_children(e, &allowed, "transition parameter/")?;
        let mut fields = BTreeSet::new();
        for child in e.children() {
            ensure!(fields.insert(child.tag()), "duplicate transition field");
            if child.tag() == "Node" {
                match (match_name, spec.id) {
                    (_, "8022") => read_expansion_ui(child, "ECP.Custom.Expanded")?,
                    ("AE.Impact_Stroke_FX", "8220") => {
                        read_expansion_ui(child, "ECP.Group.Expanded")?
                    }
                    _ => return Err(unsupported("unobserved transition parameter UI")),
                }
            } else {
                ensure!(
                    child.attributes().next().is_none() && child.is_text_only(),
                    "referenced or non-text transition field"
                );
            }
        }
        field(e, "Name", spec.name)?;
        field(e, "ParameterControlType", spec.control)?;
        field(e, "LowerBound", spec.lower)?;
        field(e, "UpperBound", spec.upper)?;
        field(e, "DiscontinuousInterpolate", spec.discontinuous)?;
        if e.child("IsTimeVarying").is_some() {
            field(e, "IsTimeVarying", Some("false"))?;
        }
        ensure!(
            e.child("Keyframes").is_none(),
            "animated Film Impact transition control"
        );
        if let Some(expected) = spec.start {
            field(e, "StartKeyframePosition", None)?;
            let actual = e
                .child("StartKeyframe")
                .and_then(Element::text)
                .ok_or_else(|| unsupported("missing transition static key"))?;
            let a: Vec<_> = actual.split(',').collect();
            let b: Vec<_> = expected.split(',').collect();
            ensure!(
                a.len() == b.len()
                    && a.first() == b.first()
                    && a.iter()
                        .skip(1)
                        .zip(b.iter().skip(1))
                        .all(|(a, b)| same_value(a, b)),
                "nondefault Film Impact transition static key"
            );
            if let Some(current) = e.child("CurrentValue") {
                let current = current
                    .text()
                    .ok_or_else(|| unsupported("missing transition CurrentValue text"))?;
                // The measured neutral 66/99 Stroke also saves a 95 slider
                // cache. Independent native geometry follows static Start 99.
                // Admit only that complete profile, never arbitrary cache drift.
                let measured_stroke_cache = match_name == "AE.Impact_Stroke_FX"
                    && spec.id == "8223"
                    && same_value(b[1], "99")
                    && current == "95"
                    && profile.iter().any(|param| {
                        param.id == "2" && param.start == Some("-91445760000000000,66.,0,0,0,0,0,0")
                    });
                ensure!(
                    same_value(current, b[1]) || measured_stroke_cache,
                    "conflicting transition CurrentValue"
                );
            }
        } else {
            field(e, "StartKeyframe", None)?;
            field(e, "CurrentValue", None)?;
            field(e, "Keyframes", None)?;
            ensure!(
                e.child("StartKeyframePosition").and_then(Element::text)
                    == Some(records::STATIC_KEYFRAME_TIME),
                "nondefault transition Curve Graph position"
            );
        }
    }
    Ok(())
}

/// The native Curve Graph expansion switch is UI state, never curve data.
fn read_expansion_ui(node: Element<'_>, property: &str) -> Result<()> {
    only_children(node, &["Properties"], "transition Curve Graph Node/")?;
    ensure!(
        node.children().count() == 1
            && node.attribute("Version") == Some("1")
            && node.attributes().all(|name| name == "Version"),
        "unobserved Curve Graph UI node"
    );
    let properties = node
        .child("Properties")
        .ok_or_else(|| unsupported("missing Curve Graph UI properties"))?;
    only_children(properties, &[property], "transition Curve Graph UI/")?;
    ensure!(
        properties.children().count() == 1
            && properties.attribute("Version") == Some("1")
            && properties.attributes().all(|name| name == "Version"),
        "unobserved Curve Graph UI properties"
    );
    let expanded = properties
        .child(property)
        .ok_or_else(|| unsupported("missing Curve Graph expansion switch"))?;
    ensure!(
        expanded.attributes().next().is_none()
            && expanded.is_text_only()
            && matches!(expanded.text(), Some("true" | "false")),
        "invalid Curve Graph UI expansion switch"
    );
    Ok(())
}
