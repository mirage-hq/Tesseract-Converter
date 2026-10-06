//! Reuse known editable Stroke geometry; diagnose unavailable controls.
use super::film_impact::{self, read_profile_with, scalar, ui, version, ProfileParam, Rule, Value};
use crate::{
    error::Result,
    format::{graph::Element, Graph, Record},
    schema::PrFilmImpactStroke,
};

const PROFILE: [ProfileParam; 31] = [
    film_impact::ERROR_OCCURRED,
    ui("8220", Some("Pre Transform"), "11"),
    scalar("8222", Some("Apply Prescale"), Rule::Inert, None, None),
    scalar(
        "8223",
        Some("Scale"),
        Rule::Range {
            min: 0.0,
            max: 100.0,
        },
        Some("0"),
        Some("100"),
    ),
    ui("8221", Some("Pre Transform"), "12"),
    ui("1", Some("Controls"), "11"),
    film_impact::CONTROL_8040,
    scalar(
        "8041",
        Some("Seed"),
        Rule::Number(0.0),
        Some("0"),
        Some("99999"),
    ),
    scalar(
        "2",
        Some("Size"),
        Rule::Range {
            min: 0.0,
            max: 1000.0,
        },
        Some("0"),
        Some("1000"),
    ),
    scalar(
        "3",
        Some("Position"),
        Rule::Number(100.0),
        Some("-100"),
        Some("100"),
    ),
    scalar(
        "14",
        Some("Roundness"),
        Rule::Number(0.0),
        Some("0"),
        Some("100"),
    ),
    scalar(
        "4",
        Some("Color"),
        Rule::Colour(18374966859414961920),
        None,
        None,
    ),
    scalar(
        "5",
        Some("Colorize"),
        Rule::Number(100.0),
        Some("0"),
        Some("100"),
    ),
    scalar(
        "6",
        Some("Duo Color"),
        Rule::Colour(18374686479671623680),
        None,
        None,
    ),
    scalar(
        "7",
        Some("Duo Amount"),
        Rule::Number(0.0),
        Some("0"),
        Some("100"),
    ),
    scalar("8", Some("Invert"), Rule::Boolean(false), None, None),
    scalar(
        "9",
        Some("Alpha Falloff"),
        Rule::Number(0.0),
        Some("0"),
        Some("100"),
    ),
    scalar(
        "10",
        Some("Opacity"),
        Rule::Range {
            min: 0.0,
            max: 100.0,
        },
        Some("0"),
        Some("100"),
    ),
    scalar("11", Some("Hide Source"), Rule::Inert, None, None),
    ui("8240", None, "16"),
    ui("13", Some("Controls"), "12"),
    film_impact::OVERLAY_MODE,
    film_impact::OVERLAY_INFO,
    film_impact::CONTROL_8141,
    version("8140"),
    film_impact::CONTROL_8300,
    film_impact::CONTROL_8301,
    film_impact::OVERLAY_ENABLED,
    film_impact::SEQUENCE_WIDTH,
    film_impact::SEQUENCE_HEIGHT,
    film_impact::SEQUENCE_PIXEL_RATIO,
];

pub(super) fn read(
    graph: &Graph<'_>,
    record: Record<'_>,
    omissions: &mut Vec<crate::Omission>,
) -> Result<PrFilmImpactStroke> {
    let hide_source = hides_source(graph, record, omissions)?;
    let values = read_profile_with(graph, record, "AE.Impact_Stroke_FX", &PROFILE, omissions)?;
    let prescale = values
        .get("8223")
        .and_then(|value| match value {
            Value::Number(value) => Some(*value),
            _ => None,
        })
        .unwrap_or(100.0);
    let opacity = values
        .get("10")
        .and_then(|value| match value {
            Value::Number(value) => Some(*value),
            _ => None,
        })
        .unwrap_or(100.0);
    // Apply Prescale is independently representable; disabling it keeps 100%.
    let prescale = if control_text(graph, record, "8222", "StartKeyframe")?
        .as_deref()
        .and_then(|value| value.split(',').nth(1))
        == Some("false")
    {
        100.0
    } else {
        prescale
    };
    if !hide_source && opacity == 100.0 {
        match (values.get("2"), prescale) {
            (Some(Value::Number(6.0)), 99.0) => return Ok(PrFilmImpactStroke::Outline99),
            (Some(Value::Number(6.0)), 100.0) => return Ok(PrFilmImpactStroke::Outline100),
            (Some(Value::Number(66.0)), 99.0) => return Ok(PrFilmImpactStroke::Frame99),
            _ => {}
        }
    }
    crate::approximate(omissions, record.identity(),
        "Stroke uses the existing fixed outline recipe; authored prescale/opacity retained independently, source concealment retained conservatively; requested Size geometry and native fidelity remain unmeasured");
    Ok(PrFilmImpactStroke::Approximate {
        prescale,
        hide_source,
        opacity,
    })
}

fn controls<'a>(graph: &'a Graph<'_>, record: Record<'_>) -> Result<Vec<Record<'a>>> {
    let component = graph.decode::<crate::schema::native::VideoFilterComponent>(record)?;
    let params = component
        .value
        .component
        .and_then(|component| component.params)
        .ok_or_else(|| {
            crate::error::unsupported("Stroke has no bound controls; source concealment is unknown")
        })?;
    params
        .items
        .iter()
        .map(|reference| graph.locate(reference, &component.identity))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn control_text(
    graph: &Graph<'_>,
    record: Record<'_>,
    id: &str,
    field: &str,
) -> Result<Option<String>> {
    Ok(controls(graph, record)?
        .into_iter()
        .find(|param| param.element().child("ParameterID").and_then(Element::text) == Some(id))
        .and_then(|param| {
            param
                .element()
                .child(field)
                .and_then(Element::text)
                .map(str::to_owned)
        }))
}

/// Unknown or animated coverage must not recover to a visible-source default.
/// UI expansion/private labels alone do not establish active opaque coverage.
pub(super) fn hides_source(
    graph: &Graph<'_>,
    record: Record<'_>,
    omissions: &mut Vec<crate::Omission>,
) -> Result<bool> {
    let params = controls(graph, record)?;
    let hides = params
        .iter()
        .filter(|param| param.element().child("ParameterID").and_then(Element::text) == Some("11"))
        .collect::<Vec<_>>();
    let opaque = params.iter().any(|param| {
        let e = param.element();
        (param.tag() == "CurveGraphComponentParam"
            || (param.tag() == "ArbVideoComponentParam"
                && (e.child("Name").and_then(Element::text) == Some("Curve Graph")
                    || e.child("ParameterID").and_then(Element::text) == Some("8022"))))
            && film_impact::has_opaque_curve(e)
    });
    let mut hidden = opaque || hides.len() != 1;
    if let [param] = hides.as_slice() {
        let e = param.element();
        let mut any = false;
        for field in ["StartKeyframe", "CurrentValue", "Keyframes"] {
            if let Some(element) = e.child(field) {
                let values = element.text().unwrap_or("");
                for key in values.split(';').filter(|key| !key.is_empty()) {
                    any = true;
                    let value = if field == "CurrentValue" {
                        key
                    } else {
                        key.split(',').nth(1).unwrap_or("")
                    };
                    hidden |= value != "false";
                }
                hidden |= !element.is_text_only()
                    || values.is_empty()
                    || element.attributes().next().is_some()
                    || e.children().filter(|child| child.tag() == field).count() != 1;
            }
        }
        hidden |= !any
            || param.tag() != "VideoComponentParam"
            || e.attribute("ClassID")
                != Some(crate::schema::records::VIDEO_BOOL_COMPONENT_PARAM.class_id)
            || e.attributes()
                .any(|name| matches!(name, "ObjectRef" | "ObjectURef"));
    }
    if hidden {
        crate::approximate(omissions, record.identity(),
            "Stroke Hide Source is active, missing, animated or opaque; source remains concealed for the whole occurrence, retaining an independently editable fixed outline where the host permits it");
    }
    Ok(hidden)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NATIVE: &str = include_str!("../../../tests/fixtures/film-impact-stroke-profile.xml");

    fn profile(xml: &str) -> Result<PrFilmImpactStroke> {
        let graph = Graph::parse(xml).unwrap();
        let component = graph
            .records()
            .find(|record| record.tag() == "VideoFilterComponent")
            .unwrap();
        read(&graph, component, &mut Vec::new())
    }

    #[test]
    fn film_impact_stroke_patch_ui_and_numeric_serialization_keep_measured_geometry() {
        let changed = NATIVE
            .replace("260300.,", "260399.,")
            .replace(
                "<ECP.Group.Expanded>true</ECP.Group.Expanded>",
                "<ECP.Group.Expanded>false</ECP.Group.Expanded>",
            )
            .replace("-91445760000000000,6.,", "-91445760000000000,6e0,")
            .replace(
                "<CurrentValue>6</CurrentValue>",
                "<CurrentValue>6.0</CurrentValue>",
            )
            .replace(
                "<CurrentValue>99</CurrentValue>",
                "<CurrentValue>9.9e1</CurrentValue>",
            );
        assert_ne!(changed, NATIVE);
        assert_eq!(profile(&changed).unwrap(), PrFilmImpactStroke::Outline99);
    }

    #[test]
    fn film_impact_stroke_authored_prescale_survives_stale_caches() {
        let cached = include_str!("../../../tests/fixtures/film-impact-stroke-prescale-cache.xml")
            .replace("ObjectID=\"23300\"", "ObjectID=\"5367\"");
        let start = NATIVE
            .find("<VideoComponentParam ObjectID=\"5367\"")
            .unwrap();
        let end = start
            + NATIVE[start..].find("</VideoComponentParam>").unwrap()
            + "</VideoComponentParam>".len();
        let mut source = NATIVE.to_owned();
        source.replace_range(start..end, &cached);
        let measured = source
            .replace("-91445760000000000,6.,", "-91445760000000000,66.,")
            .replace(
                "<CurrentValue>6</CurrentValue>",
                "<CurrentValue>66</CurrentValue>",
            );
        assert_eq!(profile(&measured).unwrap(), PrFilmImpactStroke::Frame99);
        // Parameter order must not make the stale cache bypass the Size check.
        let reordered = measured
            .replace(
                "<Param Index=\"3\" ObjectRef=\"5367\" />",
                "<Param Index=\"3\" ObjectRef=\"5372\" />",
            )
            .replace(
                "<Param Index=\"8\" ObjectRef=\"5372\" />",
                "<Param Index=\"8\" ObjectRef=\"5367\" />",
            );
        assert_eq!(profile(&reordered).unwrap(), PrFilmImpactStroke::Frame99);
        assert_eq!(profile(&source).unwrap(), PrFilmImpactStroke::Outline99);
        assert_eq!(
            profile(&measured.replace(
                "<CurrentValue>95</CurrentValue>",
                "<CurrentValue>94</CurrentValue>"
            ))
            .unwrap(),
            PrFilmImpactStroke::Frame99
        );
        assert_eq!(
            profile(&measured.replace(
                "<CurrentValue>66</CurrentValue>",
                "<CurrentValue>65</CurrentValue>"
            ))
            .unwrap(),
            PrFilmImpactStroke::Frame99
        );
    }
}
