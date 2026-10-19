//! Conservative sequence-target admission, independent of source RGB bindings.

use super::super::{Graph, Located};
use crate::{
    error::{ensure, unsupported, Result},
    omit,
    schema::{native::VideoTrackGroup, records, ColorSpace, SequenceHdr},
    Omission, OmissionScope,
};
use serde_json::Value;

pub(super) fn is_admitted(graph: &Graph<'_>, video: &Located<VideoTrackGroup>) -> bool {
    admit(graph, video).ok().flatten().is_some()
}

pub(super) fn sequence(
    graph: &Graph<'_>,
    video: &Located<VideoTrackGroup>,
    omissions: &mut Vec<Omission>,
) -> Option<SequenceHdr> {
    match admit(graph, video) {
        Ok(settings) => settings,
        Err(error) => {
            omit(omissions, OmissionScope::Feature, &video.identity,
                format!("HDR sequence target not admitted: {error}; independent source bindings and otherwise convertible content retained, native target/tone/gamut behaviour remains an explicit approximation"));
            None
        }
    }
}

fn json(encoded: Option<&str>, field: &str) -> Result<Value> {
    serde_json::from_str(encoded.ok_or_else(|| unsupported(format!("missing {field}")))?)
        .map_err(Into::into)
}

fn admit(graph: &Graph<'_>, video: &Located<VideoTrackGroup>) -> Result<Option<SequenceHdr>> {
    let fields = &video.value;
    let output = fields
        .output_color_space
        .as_deref()
        .map(serde_json::from_str::<ColorSpace>)
        .transpose()?;
    let Some(encoded) = fields.working_color_space.as_deref() else {
        ensure!(
            output
                .as_ref()
                .and_then(ColorSpace::output_hdr_transfer)
                .is_none(),
            "HDR output without an explicit working-space declaration"
        );
        return Ok(None);
    };
    let working: ColorSpace = serde_json::from_str(encoded)?;
    if working.is_sequence_sdr() && output.as_ref().is_none_or(ColorSpace::is_sequence_sdr) {
        return Ok(None);
    }
    let transfer = working
        .rgb_hdr_transfer()
        .ok_or_else(|| unsupported("unknown working RGB domain"))?;
    ensure!(
        output.as_ref().and_then(ColorSpace::output_hdr_transfer) == Some(transfer),
        "working/output HDR profiles disagree or output payload is unknown"
    );
    let configuration = json(
        fields.working_color_space_configuration.as_deref(),
        "WorkingColorSpaceConfiguration",
    )?;
    let settings = json(
        fields.color_management_settings.as_deref(),
        "sequence ColorManagementSettings",
    )?;
    let white = settings["graphicsWhiteLuminance"]
        .as_f64()
        .ok_or_else(|| unsupported("missing or nonnumeric sequence graphics white"))?;
    ensure!(
        white.is_finite() && (100.0..=1000.0).contains(&white),
        "graphics white must be finite in 100..=1000"
    );
    let hdr = SequenceHdr {
        transfer,
        graphics_white: white,
    };
    ensure!(
        configuration["workingSpaceConfigVersion"].as_u64() == Some(1)
            && configuration["workingSpaceID"].as_str() == Some(hdr.working_name())
            && configuration["workingSpaceIsLinearized"].as_u64() == Some(0),
        "working-space configuration does not declare the same nonlinear HDR domain"
    );
    ensure!(
        fields.allow_linear_compositing.as_deref() == Some("false")
            && fields.tone_mapping_algorithm.as_deref() == Some("5")
            && fields.gamut_compression_algorithm.as_deref() == Some("1"),
        "unproved linear-compositing/tone/gamut controls"
    );
    ensure!(
        settings["autoToneMapEnabled"].as_bool() == Some(true)
            && settings["enableLogColorManagement"].as_u64() == Some(2)
            && settings["lutInterpolationMethod"].as_u64() == Some(1),
        "unproved sequence colour-management controls"
    );
    let projects: Vec<_> = graph
        .records()
        .filter(|record| {
            record.tag() == records::PROJECT.tag
                && record.element().attribute(records::OBJECT_REF).is_some()
        })
        .collect();
    ensure!(
        projects.len() == 1,
        "root project reference is not uniquely owned"
    );
    ensure!(
        projects[0]
            .element()
            .attribute(records::OBJECT_ID)
            .is_none(),
        "mixed root project reference/definition role"
    );
    let project = graph.locate(&projects[0].element().reference(), "HDR root project")?;
    ensure!(
        project.tag() == records::PROJECT.tag,
        "root project reference has the wrong native role"
    );
    let settings_refs: Vec<_> = project
        .element()
        .children()
        .filter(|element| element.tag() == records::PROJECT_SETTINGS.tag)
        .collect();
    ensure!(
        settings_refs.len() == 1,
        "project settings reference is not uniquely owned"
    );
    ensure!(
        settings_refs[0].attribute(records::OBJECT_REF).is_some()
            && settings_refs[0].attribute(records::OBJECT_ID).is_none(),
        "project settings must be one reference, not an inline/mixed definition"
    );
    let settings = graph.locate(&settings_refs[0].reference(), &project.identity())?;
    ensure!(
        settings.tag() == records::PROJECT_SETTINGS.tag,
        "project settings reference has the wrong native role"
    );
    let declarations: Vec<_> = settings
        .element()
        .children()
        .filter(|element| element.tag() == "ColorManagementSettings")
        .collect();
    ensure!(
        declarations.len() == 1,
        "missing or duplicate project colour-management declarations"
    );
    let project = json(declarations[0].text(), "project ColorManagementSettings")?;
    ensure!(
        project["graphicsWhiteLuminance"].as_f64() == Some(white)
            && project["enableLogColorManagement"].as_u64() == Some(2)
            && project["lutInterpolationMethod"].as_u64() == Some(1),
        "project/sequence white declarations or control domains disagree"
    );
    Ok(Some(hdr))
}
