//! Public Pop consent uses pinned native graphic records under test-only
//! sequence/nest wrappers. The wrappers are structural cases, not Adobe proof.
use super::{support::first_project, test_support::write_prproj};
use fx_conv::{ConversionMode, ImportToTesseract};
use premiere_file::{
    Omission, OmissionKind, Premiere, PremiereImportOptions, PremiereImportOptionsWithConsent,
};
use serde_json::{json, Value};
use std::path::Path;
use tesseract_file::TesseractFile;

const END: i64 = 2_751_840_000_000;

fn sequence(guid: &str, group: u32, items: &str, transitions: &str) -> String {
    let chain = group + 1;
    format!(
        r#"<Sequence ObjectUID="{guid}"><Name>{guid}</Name><TrackGroups><TrackGroup><Second ObjectRef="{group}"/></TrackGroup></TrackGroups></Sequence>
<VideoTrackGroup ObjectID="{group}"><TrackGroup><Tracks><Track ObjectURef="{guid}-track"/></Tracks><FrameRate>8467200000</FrameRate></TrackGroup><FrameRect>0,0,1080,1920</FrameRect><ComponentOwner><Components ObjectRef="{chain}"/></ComponentOwner></VideoTrackGroup>
<VideoComponentChain ObjectID="{chain}"><ComponentChain/></VideoComponentChain>
<VideoClipTrack ObjectUID="{guid}-track"><ClipTrack><Track><ID>1</ID></Track><ClipItems><TrackItems>{items}</TrackItems></ClipItems><TransitionItems><TrackItems>{transitions}</TrackItems></TransitionItems></ClipTrack></VideoClipTrack>"#
    )
}

fn graphic_xml() -> String {
    let records = include_str!("../fixtures/graphic_pop/native.xml")
        .replace("<PremiereData>", "")
        .replace("</PremiereData>", "");
    let wrapper = sequence(
        "pop-graphics",
        1,
        r#"<TrackItem ObjectRef="421"/><TrackItem ObjectRef="422"/><TrackItem ObjectRef="423"/><TrackItem ObjectRef="424"/>"#,
        r#"<TrackItem ObjectRef="425"/><TrackItem ObjectRef="426"/><TrackItem ObjectRef="427"/>"#,
    );
    format!("<PremiereData Version=\"3\">{wrapper}{records}</PremiereData>")
}

fn nest(guid: &str, child: &str, base: u32) -> String {
    let (item, chain, sub, clip, source) = (base + 10, base + 11, base + 12, base + 13, base + 14);
    let sequence = sequence(
        guid,
        base,
        &format!(r#"<TrackItem ObjectRef="{item}"/>"#),
        "",
    );
    format!(
        r#"{sequence}
<VideoClipTrackItem ObjectID="{item}"><ClipTrackItem><ComponentOwner><Components ObjectRef="{chain}"/></ComponentOwner><TrackItem><Start>0</Start><End>{END}</End></TrackItem><SubClip ObjectRef="{sub}"/></ClipTrackItem><FrameRect>0,0,1080,1920</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio></VideoClipTrackItem>
<VideoComponentChain ObjectID="{chain}"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>
<SubClip ObjectID="{sub}"><Clip ObjectRef="{clip}"/><Name>{child}</Name></SubClip>
<VideoClip ObjectID="{clip}"><Clip><Source ObjectRef="{source}"/><InPoint>0</InPoint><OutPoint>{END}</OutPoint></Clip></VideoClip>
<VideoSequenceSource ObjectID="{source}"><SequenceSource><Sequence ObjectURef="{child}"/></SequenceSource><OriginalDuration>{END}</OriginalDuration></VideoSequenceSource>"#
    )
}

fn import(root: &Path, input: &Path, target: &str, allow: bool) -> (Value, Vec<Omission>) {
    let options = PremiereImportOptions {
        sequence: Some(target.to_owned()),
    };
    let output = root.join(if allow { "allowed" } else { "default" });
    let consent = PremiereImportOptionsWithConsent {
        selection: options.clone(),
        allow_film_impact_pop: allow,
    };
    let run = |mode| {
        if allow {
            Premiere.import_with_consent_with_progress(
                input,
                &output,
                &consent,
                mode,
                None,
                None,
                fx_conv::Progress::default(),
            )
        } else {
            // Original downstream literal and trait call remain source-compatible.
            Premiere.import_to_tesseract(input, &output, &options, mode)
        }
    };
    let checked = run(ConversionMode::Check).unwrap();
    assert!(!output.exists());
    let written = run(ConversionMode::Write).unwrap();
    assert_eq!(
        checked
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        written
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
    );
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    (document, written.diagnostics)
}

fn texts(layers: &Value) -> Vec<&str> {
    let mut result = Vec::new();
    for layer in layers.as_array().unwrap() {
        if layer["type"] == "Text" {
            result.push(layer["sourceText"]["text"].as_str().unwrap());
        } else if layer["type"] == "Group" {
            result.extend(texts(&layer["layers"]));
        }
    }
    result
}

fn pop_reports(omissions: &[Omission]) -> Vec<&Omission> {
    omissions
        .iter()
        .filter(|omission| {
            ["425", "426", "427"]
                .iter()
                .any(|id| omission.record.ends_with(id))
        })
        .collect()
}

#[test]
fn film_impact_pop_legacy_options_literal_and_new_consent_default_remain_denied() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("graphics.prproj");
    write_prproj(&input, &graphic_xml());
    // No struct update: this is the original published options shape.
    let original = PremiereImportOptions {
        sequence: Some("pop-graphics".to_owned()),
    };
    let legacy_output = root.path().join("legacy-check");
    let legacy = Premiere
        .import_to_tesseract(&input, &legacy_output, &original, ConversionMode::Check)
        .unwrap();
    assert!(!legacy_output.exists());
    let consent = PremiereImportOptionsWithConsent {
        selection: original,
        ..Default::default()
    };
    assert!(!consent.allow_film_impact_pop);
    let output = root.path().join("new-default-check");
    let current = Premiere
        .import_with_consent_with_progress(
            &input,
            &output,
            &consent,
            ConversionMode::Check,
            None,
            None,
            fx_conv::Progress::default(),
        )
        .unwrap();
    assert!(!output.exists());
    assert_eq!(legacy.diagnostics, current.diagnostics);
    let losses = pop_reports(&current.diagnostics);
    assert_eq!(losses.len(), 3);
    assert!(losses
        .iter()
        .all(|loss| loss.kind == OmissionKind::Omitted
            && loss.reason.contains("emulation is disabled")));
}

#[test]
fn film_impact_pop_consent_survives_source_bound_map_and_relink_routes() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("graphics.prproj");
    write_prproj(&input, &graphic_xml());
    let source = fx_conv::MediaMapSource {
        format: "premiere".to_owned(),
        target: "pop-graphics".to_owned(),
        sha256: format!("{:x}", Sha256::digest(std::fs::read(&input).unwrap())),
    };
    // Empty manifests prove option routing/source authentication, not a new
    // independent prepared/relinked-media Pop fidelity case.
    let map_path = root.path().join("media-map.json");
    std::fs::write(
        &map_path,
        serde_json::to_vec(&fx_conv::MediaMap {
            version: 1,
            source: source.clone(),
            replacements: Vec::new(),
        })
        .unwrap(),
    )
    .unwrap();
    let map = fx_conv::ValidatedMediaMap::load(&map_path).unwrap();
    let relink = premiere_file::ValidatedMediaRelink::new(premiere_file::MediaRelink {
        version: 1,
        source: source.clone(),
        bindings: Vec::new(),
    })
    .unwrap();
    for (route, map, relink) in [
        ("map", Some(&map), None),
        ("relink", None, Some(&relink)),
        ("both", Some(&map), Some(&relink)),
    ] {
        for allow in [false, true] {
            let options = PremiereImportOptionsWithConsent {
                selection: PremiereImportOptions {
                    sequence: Some("pop-graphics".to_owned()),
                },
                allow_film_impact_pop: allow,
            };
            let output = root.path().join(format!("{route}-{allow}"));
            let run = |mode| {
                Premiere.import_with_consent_with_progress(
                    &input,
                    &output,
                    &options,
                    mode,
                    map,
                    relink,
                    fx_conv::Progress::default(),
                )
            };
            let checked = run(ConversionMode::Check).unwrap();
            assert!(!output.exists());
            let written = run(ConversionMode::Write).unwrap();
            assert_eq!(checked.diagnostics, written.diagnostics);
            let losses = pop_reports(&written.diagnostics);
            assert_eq!(losses.len(), 3);
            assert!(losses.iter().all(|loss| if allow {
                loss.kind == OmissionKind::Approximated
                    && loss.reason.contains("early blur/fade remain approximate")
            } else {
                loss.kind == OmissionKind::Omitted && loss.reason.contains("emulation is disabled")
            }));
            let wire = TesseractFile::open(first_project(&output))
                .unwrap()
                .project_json()
                .unwrap();
            assert_eq!(texts(&wire["composition"]["layers"]).len(), 5);
            assert_eq!(
                wire["composition"]["dynamics"]["entries"]
                    .as_array()
                    .unwrap()
                    .is_empty(),
                !allow
            );
        }
    }
    let stale = premiere_file::ValidatedMediaRelink::new(premiere_file::MediaRelink {
        version: 1,
        source: fx_conv::MediaMapSource {
            sha256: "0".repeat(64),
            ..source
        },
        bindings: Vec::new(),
    })
    .unwrap();
    let output = root.path().join("stale-relocation");
    let options = PremiereImportOptionsWithConsent {
        selection: PremiereImportOptions {
            sequence: Some("pop-graphics".to_owned()),
        },
        allow_film_impact_pop: true,
    };
    assert!(Premiere
        .import_with_consent_with_progress(
            &input,
            &output,
            &options,
            ConversionMode::Write,
            Some(&map),
            Some(&stale),
            fx_conv::Progress::default()
        )
        .is_err());
    assert!(!output.exists());
}

#[test]
fn film_impact_pop_public_default_denial_and_opt_in_preserve_graphic_siblings() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("graphics.prproj");
    write_prproj(&source, &graphic_xml());
    let (default, omissions) = import(root.path(), &source, "pop-graphics", false);
    let reports = pop_reports(&omissions);
    assert_eq!(reports.len(), 3, "{omissions:?}");
    assert!(reports
        .iter()
        .all(|report| report.kind == OmissionKind::Omitted
            && report.reason.contains("emulation is disabled")));
    assert!(default["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .is_empty());
    let (allowed, omissions) = import(root.path(), &source, "pop-graphics", true);
    let reports = pop_reports(&omissions);
    assert_eq!(reports.len(), 3, "{omissions:?}");
    assert!(reports
        .iter()
        .all(|report| report.kind == OmissionKind::Approximated
            && report.reason.contains("early blur/fade remain approximate")));
    let default_texts = texts(&default["composition"]["layers"]);
    assert_eq!(
        default_texts.len(),
        5,
        "complete-line block plus three siblings"
    );
    assert_eq!(texts(&allowed["composition"]["layers"]), default_texts);
    let owner = &allowed["composition"]["layers"][0];
    assert_eq!(owner["type"], "Group");
    assert_eq!(owner["layers"].as_array().unwrap().len(), 2);
    let scale = allowed["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == owner["id"] && entry["target"]["propertyType"] == "scaleX"
        })
        .unwrap();
    let keys = scale["animator"]["keyframes"].as_array().unwrap();
    assert_eq!(keys.first().unwrap()["value"]["value"], 100.0);
    assert_eq!(keys.last().unwrap()["layerTime"], 1267);
    assert_eq!(keys.last().unwrap()["value"]["value"], 0.0);
    assert!(keys.iter().all(|key| key["easing"]["type"] == "linear"));
}

#[test]
fn film_impact_pop_unknown_plugin_field_recovers_transition_and_retains_text_owners() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("unknown-ui.prproj");
    write_prproj(&source, &graphic_xml());
    let (baseline, _) = import(root.path(), &source, "pop-graphics", false);
    let mut xml = graphic_xml();
    let component = xml.find("<VideoFilterComponent ObjectID=\"761\"").unwrap();
    let end = component + xml[component..].find("</Component>").unwrap();
    xml.insert_str(end, "<FutureUIState>expanded</FutureUIState>");
    write_prproj(&source, &xml);
    let (retained, omissions) = import(root.path(), &source, "pop-graphics", true);
    assert_eq!(
        texts(&retained["composition"]["layers"]),
        texts(&baseline["composition"]["layers"])
    );
    assert_eq!(
        retained["composition"]["layers"].as_array().unwrap().len(),
        baseline["composition"]["layers"].as_array().unwrap().len()
    );
    let reports = pop_reports(&omissions);
    assert_eq!(reports.len(), 3, "{omissions:?}");
    assert!(
        reports
            .iter()
            .all(|report| report.kind == OmissionKind::Approximated),
        "{omissions:?}"
    );
    assert!(
        omissions
            .iter()
            .any(|loss| loss.reason.contains("FutureUIState")
                && loss.reason.contains("known controls retained")),
        "{omissions:?}"
    );
}

#[test]
fn film_impact_pop_public_denial_recurses_and_opt_in_keeps_nested_host_guard() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("nested.prproj");
    let records = nest("middle", "pop-graphics", 90_000) + &nest("outer", "middle", 91_000);
    let xml = graphic_xml().replace("</PremiereData>", &format!("{records}</PremiereData>"));
    write_prproj(&source, &xml);
    let (default, omissions) = import(root.path(), &source, "outer", false);
    let reports = pop_reports(&omissions);
    assert_eq!(reports.len(), 3, "{omissions:?}");
    assert!(reports
        .iter()
        .all(|report| report.kind == OmissionKind::Omitted
            && report.reason.contains("emulation is disabled")));
    let outer = &default["composition"]["layers"][0];
    let middle = &outer["layers"][0];
    assert_eq!(outer["type"], "Group");
    assert_eq!(middle["type"], "Group");
    assert_eq!(
        outer["playback"]["inputRange"],
        json!({"start":0,"duration":10833})
    );
    assert_eq!(texts(&middle["layers"]).len(), 5);
    let (allowed, omissions) = import(root.path(), &source, "outer", true);
    let reports = pop_reports(&omissions);
    assert_eq!(reports.len(), 3, "{omissions:?}");
    assert!(reports
        .iter()
        .all(|report| report.kind == OmissionKind::Omitted
            && report.reason.contains("document clock")
            && !report.reason.contains("emulation is disabled")));
    assert_eq!(
        allowed["composition"]["layers"],
        default["composition"]["layers"]
    );
    assert_eq!(
        allowed["composition"]["dynamics"],
        default["composition"]["dynamics"]
    );
}
