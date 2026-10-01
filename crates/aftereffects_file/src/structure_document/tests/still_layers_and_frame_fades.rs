//! Synthetic regressions for still-image lifetimes, still Geometry2 stages and
//! the `Fade In+Out - frames` owner lowering. The effect records are authored
//! here from the documented control layout, with small arbitrary values, and
//! grafted into public host fixtures; no recorded project content is used.

use fx_schema::{
    LayerId, Position, PropType, PropertyTarget, PropertyValue,
    animator::PropertyKeyframeEasing::{self, CubicBezier, Linear},
};

use super::*;
use crate::rifx::Chunk;

/// The preset's Source Opacity expression. The importer compares tokens, so
/// only its spelling matters, not its line breaks.
const PRESET_EXPRESSION: &str = "\
var fadeInDuration = framesToTime(effect(\"Fade In+Out - frames\")(\"Fade In Duration (frames)\"));
var fadeOutDuration = framesToTime(effect(\"Fade In+Out - frames\")(\"Fade Out Duration (frames)\"));
var fadeInOpacity = fadeInDuration ? linear(time, inPoint, inPoint + fadeInDuration, 0, value) : value;
var fadeOutOpacity = fadeOutDuration ? linear(time, outPoint - fadeOutDuration, outPoint, value, 0) : value;
fadeInOpacity + fadeOutOpacity - value;";
/// The controller's local parameter table: name, `pard` kind and label.
const CONTROLLER_ROWS: [(&str, u32, &str); 4] = [
    ("ADBE CM FadeInOutFrames-0000", 0, ""),
    (
        "ADBE CM FadeInOutFrames-0001",
        10,
        "Fade In Duration (frames)",
    ),
    (
        "ADBE CM FadeInOutFrames-0002",
        10,
        "Fade Out Duration (frames)",
    ),
    ("ADBE Effect Built In Params", 9, ""),
];

fn data(id: [u8; 4], bytes: impl Into<Vec<u8>>) -> Chunk {
    Chunk::data(id, bytes.into()).expect("small test chunk")
}

fn match_name(name: &str) -> Chunk {
    let mut bytes = name.as_bytes().to_vec();
    bytes.resize(40, 0);
    data(*b"tdmn", bytes)
}

fn is_match_name(chunk: &Chunk, name: &str) -> bool {
    chunk.id() == *b"tdmn"
        && chunk
            .data_payload()
            .is_some_and(|bytes| bytes.split(|byte| *byte == 0).next() == Some(name.as_bytes()))
}

/// A static one-dimensional numeric leaf, with an enabled expression if given.
fn scalar(value: f64, expression: Option<&str>) -> Chunk {
    let mut meta = vec![0; 124];
    meta[..2].copy_from_slice(&[0xdb, 0x99]);
    meta[3] = 1;
    let mut leaf = vec![
        data(*b"tdb4", meta),
        data(*b"tdsb", [0, 0, 0, 1]),
        data(*b"cdat", value.to_be_bytes()),
    ];
    leaf.extend(expression.map(|source| data(*b"Utf8", source.as_bytes())));
    Chunk::list(*b"tdbs", leaf)
}

/// A keyed plugin Point in fractions of the source size, with Bezier keys:
/// (time in `timebase` units, value, incoming and outgoing influence).
fn keyed_point(timebase: u32, keys: &[(i32, [f64; 2], f64, f64)]) -> Chunk {
    let mut meta = vec![0; 124];
    meta[..2].copy_from_slice(&[0xdb, 0x99]);
    meta[3] = 2;
    meta[12..16].copy_from_slice(&timebase.to_be_bytes());
    meta[59] = 4;
    meta[68] = 1;
    let mut header = vec![0; 24];
    header[10..12].copy_from_slice(&u16::try_from(keys.len()).unwrap().to_be_bytes());
    header[18..20].copy_from_slice(&88_u16.to_be_bytes());
    header[23] = 4;
    let mut items = Vec::new();
    for (time, [x, y], incoming, outgoing) in keys {
        items.extend(time.to_be_bytes());
        items.extend([2, 2, 0, 0]);
        // Values, in speeds, in influences, out speeds, out influences.
        for value in [
            *x, *y, 0.0, 0.0, *incoming, *incoming, 0.0, 0.0, *outgoing, *outgoing,
        ] {
            items.extend(value.to_be_bytes());
        }
    }
    Chunk::list(
        *b"tdbs",
        vec![
            data(*b"tdb4", meta),
            data(*b"tdsb", [0, 0, 0, 1]),
            Chunk::list(
                *b"list",
                vec![data(*b"lhd3", header), data(*b"ldat", items)],
            ),
        ],
    )
}

/// A `parT` table: its row count, then one `pard` row per control.
fn declarations(rows: &[(&str, u32, &str)]) -> Vec<Chunk> {
    let count = u32::try_from(rows.len()).unwrap();
    let mut table = vec![data(*b"parn", count.to_be_bytes())];
    for (name, kind, label) in rows {
        let mut pard = vec![0; 148];
        pard[12..16].copy_from_slice(&kind.to_be_bytes());
        pard[16..16 + label.len()].copy_from_slice(label.as_bytes());
        table.extend([match_name(name), data(*b"pard", pard)]);
    }
    table
}

/// One effect instance: its match name, then a descriptor with its display
/// name, its parameter table (empty for a sparse instance) and its controls.
fn effect(
    name: &str,
    display: &str,
    table: Vec<Chunk>,
    controls: Vec<(&str, Chunk)>,
) -> Vec<Chunk> {
    let mut label = b"Utf8".to_vec();
    label.extend(u32::try_from(display.len()).unwrap().to_be_bytes());
    label.extend(display.as_bytes());
    let mut body: Vec<_> = controls
        .into_iter()
        .flat_map(|(control, leaf)| [match_name(control), leaf])
        .collect();
    body.push(match_name("ADBE Group End"));
    vec![
        match_name(name),
        Chunk::list(
            *b"sspc",
            vec![
                data(*b"fnam", label),
                Chunk::list(*b"parT", table),
                Chunk::list(*b"tdgp", body),
            ],
        ),
    ]
}

/// An Effect Parade root run holding `effects` in order.
fn parade(effects: Vec<Vec<Chunk>>) -> Vec<Chunk> {
    let mut body: Vec<_> = effects.into_iter().flatten().collect();
    body.push(match_name("ADBE Group End"));
    vec![
        match_name("ADBE Effect Parade"),
        Chunk::list(*b"tdgp", body),
    ]
}

/// The controller and its Solid Composite, with every mapped control explicit.
struct Preset {
    frames: f64,
    fade_out: f64,
    controller: &'static str,
    table: Vec<Chunk>,
    expression: &'static str,
    background: f64,
    after: Vec<Vec<Chunk>>,
}

impl Default for Preset {
    fn default() -> Self {
        Self {
            frames: 3.0,
            fade_out: 0.0,
            controller: "Fade In+Out - frames",
            table: Vec::new(),
            expression: PRESET_EXPRESSION,
            background: 0.0,
            after: Vec::new(),
        }
    }
}

impl Preset {
    fn parade(self) -> Vec<Chunk> {
        let controller = effect(
            "ADBE CM FadeInOutFrames",
            self.controller,
            self.table,
            vec![
                ("ADBE CM FadeInOutFrames-0001", scalar(self.frames, None)),
                ("ADBE CM FadeInOutFrames-0002", scalar(self.fade_out, None)),
            ],
        );
        let composite = effect(
            "ADBE Solid Composite",
            "Solid Composite",
            Vec::new(),
            vec![
                (
                    "ADBE Solid Composite-0001",
                    scalar(100.0, Some(self.expression)),
                ),
                ("ADBE Solid Composite-0003", scalar(self.background, None)),
            ],
        );
        let mut effects = vec![controller, composite];
        effects.extend(self.after);
        parade(effects)
    }
}

/// A sparse Transform (`ADBE Geometry2`) instance with the given controls.
fn geometry2(controls: Vec<(&str, Chunk)>) -> Vec<Chunk> {
    effect("ADBE Geometry2", "Transform", Vec::new(), controls)
}

fn host_layer(project: &mut StructuralProject, composition: u32, id: u32) -> &mut Layer {
    composition_mut(project, composition)
        .layers
        .iter_mut()
        .find(|layer| layer.record.id() == id)
        .expect("host layer")
}

/// Puts Effect Parade runs into the layer's property root before its Transform.
fn graft(layer: &mut Layer, parades: Vec<Vec<Chunk>>) {
    let properties = layer
        .content
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .and_then(Chunk::children_mut)
        .expect("host property root");
    let transform = properties
        .iter()
        .position(|chunk| is_match_name(chunk, "ADBE Transform Group"))
        .expect("host Transform");
    properties.splice(transform..transform, parades.into_iter().flatten());
}

/// Sets a layer's start time to `start / 4` seconds.
fn start_quarters(layer: &mut Layer, start: i32) {
    patch(layer, 12, &start.to_be_bytes());
    patch(layer, 16, &4_u32.to_be_bytes());
}

fn convert(project: &StructuralProject, composition: u32) -> StructuralConversion {
    to_linked_picture(
        project,
        composition,
        &mut |_| MediaResolution::Asset,
        Destination::LinkedPicture {
            parent: None,
            first_id: 1,
            asset_namespace: AssetNamespace::new("linked"),
        },
    )
    .expect("best-effort linked import")
}

fn owner<'a>(converted: &'a StructuralConversion, name: &str) -> &'a GroupLayer {
    let mut matches = root(converted)
        .layers
        .iter()
        .map(as_group)
        .filter(|group| group.name == name);
    let owner = matches.next().unwrap_or_else(|| panic!("owner {name}"));
    assert!(matches.next().is_none(), "one owner named {name}");
    owner
}

fn only_owner(converted: &StructuralConversion) -> &GroupLayer {
    let [layer] = root(converted).layers.as_slice() else {
        panic!("one occurrence owner")
    };
    as_group(layer)
}

fn descendant<'a>(group: &'a GroupLayer, name: &str) -> &'a GroupLayer {
    group
        .layers
        .iter()
        .filter_map(|layer| match layer.data() {
            FxLayer::Group(child) if child.name == name => Some(child),
            FxLayer::Group(child) => Some(descendant(child, name)),
            _ => None,
        })
        .next()
        .unwrap_or_else(|| panic!("{name} below {}", group.name))
}

/// Millisecond time, value and incoming easing of one scalar track.
fn keys(
    converted: &StructuralConversion,
    layer: LayerId,
    property: PropType,
) -> Vec<(i64, f64, PropertyKeyframeEasing)> {
    let target = PropertyTarget::layer(layer, property);
    let mut tracks = converted
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .filter(|entry| entry.target == target);
    let Some(track) = tracks.next() else {
        return Vec::new();
    };
    assert!(tracks.next().is_none(), "one track for {target}");
    track
        .animator
        .keyframe_track()
        .expect("editable keyframes")
        .keyframes()
        .iter()
        .map(|key| {
            let PropertyValue::Float(value) = key.value() else {
                panic!("scalar key")
            };
            (key.layer_time().as_millis(), *value, key.easing())
        })
        .collect()
}

fn messages(converted: &StructuralConversion, layer: u32) -> Vec<&str> {
    converted
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.layer_id == Some(layer))
        .map(|diagnostic| diagnostic.message.as_str())
        .collect()
}

/// `group` and every Group below it.
fn groups_below(group: &GroupLayer) -> Vec<&GroupLayer> {
    let mut groups = vec![group];
    for layer in &group.layers {
        if let FxLayer::Group(child) = layer.data() {
            groups.extend(groups_below(child));
        }
    }
    groups
}

/// The Groups whose Opacity keys change value: the fades.
fn faded(converted: &StructuralConversion, groups: &[&GroupLayer]) -> Vec<LayerId> {
    let entries = converted.document.composition().dynamics().entries();
    groups
        .iter()
        .filter(|group| {
            let target = PropertyTarget::layer(group.id, PropType::Opacity);
            entries.iter().any(|entry| {
                entry.target == target
                    && entry.animator.keyframe_track().is_some_and(|track| {
                        track
                            .keyframes()
                            .windows(2)
                            .any(|pair| pair[0].value() != pair[1].value())
                    })
            })
        })
        .map(|group| group.id)
        .collect()
}

/// Composition 2 of the public still fixture: one AV layer (14) of a 200 × 200
/// still, with start 1 s, in point -0.25 s and out point 4 s on unit stretch.
fn still_project() -> StructuralProject {
    let mut project = read_project(&crate::test_fixtures::read("media/footage_not_missing.aep"))
        .expect("public still host");
    let layer = host_layer(&mut project, 2, 14);
    start_quarters(layer, 4);
    patch(layer, 20, &(-1_i32).to_be_bytes());
    patch(layer, 24, &4_u32.to_be_bytes());
    patch(layer, 28, &4_i32.to_be_bytes());
    patch(layer, 32, &1_u32.to_be_bytes());
    project
}

/// The public text host with its text layer 16 starting at 0.25 s.
fn text_project() -> StructuralProject {
    let mut project = read_project(&crate::test_fixtures::read(
        "text/import_text_document_controls.aep",
    ))
    .expect("public text host");
    assert_eq!(composition(&project, 1).frame_rate, 24.0);
    start_quarters(host_layer(&mut project, 1, 16), 1);
    project
}

#[test]
fn still_layer_keeps_its_lifetime_before_start_without_a_source_clock() {
    let converted = convert(&still_project(), 2);
    let content = descendant(only_owner(&converted), "Source content clock");
    // Parent time 1 s - 0.25 s to 1 s + 4 s.
    assert_eq!(content.playback.input_range().start.as_millis(), 750);
    assert_eq!(content.playback.input_range().duration.as_millis(), 4250);
    assert!(content.playback.time_remap().is_none(), "no sampled clock");
    assert!(!content.is_hidden);

    // Video keeps its affine source clock, which starts at the layer start.
    let mut project = still_project();
    let footage = project.items.iter_mut().find(|item| item.id == 1).unwrap();
    let Some(Ok(media)) = &mut footage.media else {
        panic!("still descriptor")
    };
    media.kind = crate::media::MediaKind::Video;
    media.duration = crate::media::MediaDuration {
        numerator: 180,
        denominator: 30,
    };
    let converted = convert(&project, 2);
    let content = descendant(only_owner(&converted), "Source content clock");
    assert_eq!(content.playback.input_range().start.as_millis(), 1000);
    assert!(content.playback.time_remap().is_some());
}

#[test]
fn still_geometry2_is_an_editable_stage_on_the_source_image_plane() {
    let mut project = still_project();
    graft(
        host_layer(&mut project, 2, 14),
        vec![parade(vec![geometry2(vec![
            ("ADBE Geometry2-0003", scalar(50.0, None)),
            ("ADBE Geometry2-0007", scalar(30.0, None)),
            ("ADBE Geometry2-0008", scalar(80.0, None)),
        ])])],
    );
    let converted = convert(&project, 2);
    let owner = only_owner(&converted);
    assert!(owner.effects.is_empty(), "no FX effect substitute");
    let [child] = owner.layers.as_slice() else {
        panic!("one stage")
    };
    let stage = as_group(child);
    assert_eq!(stage.name, "Transform");
    assert_eq!(
        as_group(&stage.layers[0]).name,
        "Source content clock",
        "the stage wraps the source content"
    );
    // Without explicit points, Anchor Point and Position default to the
    // centre of the 200 × 200 source; Uniform Scale applies Height to both.
    assert_eq!(stage.transform.anchor_point, [100.0, 100.0]);
    assert_eq!(stage.transform.position, Position::TwoD([100.0, 100.0]));
    assert_eq!(stage.transform.scale, [50.0, 50.0]);
    assert_eq!(stage.transform.rotation, 30.0);
    assert_eq!(stage.transform.opacity.value(), 80.0);
    assert!(
        !messages(&converted, 14)
            .iter()
            .any(|message| message.starts_with("Geometry2:") && message.contains("omitted")),
        "{:?}",
        messages(&converted, 14)
    );
}

#[test]
fn keyed_still_geometry2_position_keys_the_stage_on_the_layer_clock() {
    // Layer time 0 s and 0.5 s in 1/8 s units; fractions of the 200 x 200
    // source. 25 % outgoing, then 75 % incoming influence, zero speeds.
    let position = keyed_point(
        8,
        &[(0, [0.5, 0.25], 0.5, 0.25), (4, [0.5, 0.75], 0.75, 0.5)],
    );
    let mut project = still_project();
    graft(
        host_layer(&mut project, 2, 14),
        vec![parade(vec![geometry2(vec![(
            "ADBE Geometry2-0002",
            position,
        )])])],
    );
    let converted = convert(&project, 2);
    let stage = as_group(&only_owner(&converted).layers[0]);
    assert_eq!(as_group(&stage.layers[0]).name, "Source content clock");
    assert_eq!(stage.transform.anchor_point, [100.0, 100.0]);
    assert_eq!(stage.transform.position, Position::TwoD([100.0, 50.0]));
    // The layer starts at 1 s, so its keys sit at parent 1 s and 1.5 s.
    let y = keys(&converted, stage.id, PropType::PositionY);
    assert_eq!(
        y.iter().map(|key| (key.0, key.1)).collect::<Vec<_>>(),
        [(1000, 50.0), (1500, 150.0)]
    );
    assert_eq!(
        y[1].2,
        CubicBezier {
            x1: 0.25,
            y1: 0.0,
            x2: 0.25,
            y2: 1.0
        }
    );
    for (_, value, _) in keys(&converted, stage.id, PropType::PositionX) {
        assert_eq!(value, 100.0);
    }
    assert!(
        !messages(&converted, 14)
            .iter()
            .any(|message| message.starts_with("Geometry2:") && message.contains("omitted")),
        "{:?}",
        messages(&converted, 14)
    );
}

#[test]
fn a_modern_zero_matte_id_leaves_both_occurrences_unmasked() {
    for modern in [true, false] {
        let mut project = still_project();
        let composition = composition_mut(&mut project, 2);
        let mut upper = composition.layers[0].clone();
        let mut lower = upper.clone();
        upper.name = "Upper still".into();
        lower.name = "Lower still".into();
        patch(&mut upper, 0, &41_u32.to_be_bytes());
        patch(&mut lower, 0, &42_u32.to_be_bytes());
        // Alpha matte mode, with the modern matte-layer field set to zero.
        patch(&mut lower, 107, &[1]);
        patch(&mut lower, 160, &0_u32.to_be_bytes());
        if !modern {
            // The legacy layout has no matte-layer field: the layer above keys it.
            lower.record = LayerRecord::decode(&lower.record.raw_bytes()[..160]).unwrap();
        }
        composition.layers = vec![upper, lower];
        let converted = convert(&project, 2);
        if modern {
            let names: Vec<_> = root(&converted)
                .layers
                .iter()
                .map(|layer| layer.data().name())
                .collect();
            assert_eq!(names, ["Upper still", "Lower still"], "no matte copy");
            for name in names {
                assert!(owner(&converted, name).track_matte.is_none(), "{name}");
            }
            assert!(!has(&converted, Limitation::TrackMatte));
        } else {
            assert!(
                owner(&converted, "Lower still").track_matte.is_some(),
                "legacy record keeps the implicit matte"
            );
        }
    }
}

#[test]
fn still_geometry2_outside_the_mapped_profile_is_diagnosed() {
    let variants = [
        (
            "nondefault skew",
            vec![parade(vec![geometry2(vec![(
                "ADBE Geometry2-0005",
                scalar(10.0, None),
            )])])],
        ),
        (
            "several Transform effects",
            vec![parade(vec![geometry2(Vec::new()), geometry2(Vec::new())])],
        ),
    ];
    for (variant, parades) in variants {
        let mut project = still_project();
        graft(host_layer(&mut project, 2, 14), parades);
        let converted = convert(&project, 2);
        assert_eq!(
            as_group(&only_owner(&converted).layers[0]).name,
            "Source content clock",
            "{variant}: content stays directly under its owner"
        );
        let messages = messages(&converted, 14);
        assert!(
            messages
                .iter()
                .any(|message| message.starts_with("Geometry2:")
                    && message.contains(variant)
                    && message.contains("effect omitted")),
            "{variant}: {messages:?}"
        );
    }
}

#[test]
fn frame_fade_becomes_owner_opacity_keys() {
    for table in [Vec::new(), declarations(&CONTROLLER_ROWS)] {
        let declared = !table.is_empty();
        let mut project = text_project();
        graft(
            host_layer(&mut project, 1, 16),
            vec![
                Preset {
                    table,
                    ..Preset::default()
                }
                .parade(),
            ],
        );
        let converted = convert(&project, 1);
        let owner = owner(&converted, "Editable Text");
        // Three frames at 24 fps from the 0.25 s inPoint.
        assert_eq!(
            keys(&converted, owner.id, PropType::Opacity),
            [(250, 0.0, Linear), (375, 100.0, Linear)],
            "declared table: {declared}"
        );
        assert!(owner.effects.is_empty(), "no FX effect substitute");
        let messages = messages(&converted, 16);
        assert!(
            !messages
                .iter()
                .any(|message| message.contains("no current native FX counterpart")),
            "declared table: {declared}: {messages:?}"
        );
    }
}

#[test]
fn frame_fades_outside_the_profile_keep_explicit_omissions() {
    let mut renamed_label = CONTROLLER_ROWS;
    renamed_label[1].2 = "Fade In Length";
    let variants = [
        (
            "only a zero fade-out is mapped",
            Preset {
                fade_out: 2.0,
                ..Preset::default()
            },
        ),
        (
            "only a transparent background is mapped",
            Preset {
                background: 40.0,
                ..Preset::default()
            },
        ),
        (
            "Source Opacity is not the enabled complete preset expression",
            Preset {
                expression: "value;",
                ..Preset::default()
            },
        ),
        (
            "parameter label no longer binds the expression",
            Preset {
                table: declarations(&renamed_label),
                ..Preset::default()
            },
        ),
        // A present table that declares nothing is not a sparse instance.
        (
            "incomplete parameter declarations",
            Preset {
                table: vec![data(*b"parn", [0; 4])],
                ..Preset::default()
            },
        ),
        (
            "a renamed controller does not bind the expression",
            Preset {
                controller: "Fade In+Out - frames 2",
                ..Preset::default()
            },
        ),
        (
            "an effect after the Solid Composite renders the faded image",
            Preset {
                after: vec![
                    geometry2(Vec::new()),
                    effect("ADBE Invert", "Invert", Vec::new(), Vec::new()),
                ],
                ..Preset::default()
            },
        ),
    ];
    for (reason, preset) in variants {
        let mut project = text_project();
        graft(host_layer(&mut project, 1, 16), vec![preset.parade()]);
        let converted = convert(&project, 1);
        let owner = owner(&converted, "Editable Text");
        assert!(
            keys(&converted, owner.id, PropType::Opacity).is_empty(),
            "{reason}"
        );
        let messages = messages(&converted, 16);
        for effect in ["ADBE CM FadeInOutFrames", "ADBE Solid Composite"] {
            assert!(
                messages.iter().any(|message| message.starts_with(&format!(
                    "Effect {effect}: no current native FX counterpart"
                ))),
                "{reason}: {messages:?}"
            );
        }
        assert!(
            messages
                .iter()
                .any(|message| message.contains(&format!("frame fade not lowered ({reason})"))),
            "{reason}: {messages:?}"
        );
    }
}

#[test]
fn frame_fade_on_a_non_caption_shape_stays_on_its_owner() {
    // The Shape importer lowers only its caption profile; the occurrence owner
    // keeps the preset on any other Shape layer.
    let mut project = read_project(&crate::test_fixtures::read("shapes/shape_basic.aep"))
        .expect("public Shape host");
    assert_eq!(composition(&project, 1).frame_rate, 24.0);
    let layer = host_layer(&mut project, 1, 15);
    start_quarters(layer, 2);
    graft(layer, vec![Preset::default().parade()]);
    let converted = convert(&project, 1);
    let owner = owner(&converted, "TestLayer");
    assert_eq!(
        keys(&converted, owner.id, PropType::Opacity),
        [(500, 0.0, Linear), (625, 100.0, Linear)]
    );
    let shape = descendant(owner, "Source content clock");
    assert!(!shape.layers.is_empty(), "the shape still imports");
    assert_eq!(
        faded(&converted, &groups_below(owner)),
        [owner.id],
        "one fade"
    );
}

#[test]
fn duplicated_effect_parades_keep_their_ambiguity_without_a_fade() {
    let mut project = text_project();
    graft(
        host_layer(&mut project, 1, 16),
        vec![Preset::default().parade(), Preset::default().parade()],
    );
    let converted = convert(&project, 1);
    let owner = owner(&converted, "Editable Text");
    assert!(owner.effects.is_empty(), "ambiguous effects are omitted");
    assert!(keys(&converted, owner.id, PropType::Opacity).is_empty());
    let messages = messages(&converted, 16);
    for expected in [
        "duplicate Effect Parade roots; ambiguous effects omitted",
        "frame fade not lowered (duplicate Effect Parade roots)",
    ] {
        assert!(
            messages.iter().any(|message| message.contains(expected)),
            "{expected}: {messages:?}"
        );
    }
}

#[test]
fn frame_fade_is_not_lowered_under_rendered_layer_styles() {
    let mut project = read_project(&crate::test_fixtures::read(
        "layer_styles/styles_static_adobe.aep",
    ))
    .expect("public Layer Styles host");
    graft(
        host_layer(&mut project, 1, 15),
        vec![Preset::default().parade()],
    );
    let converted = convert(&project, 1);
    let owner = owner(&converted, "DropShadow Subject");
    assert!(keys(&converted, owner.id, PropType::Opacity).is_empty());
    let messages = messages(&converted, 15);
    assert!(
        messages
            .iter()
            .any(|message| message.contains("frame fade not lowered (Layer Styles")),
        "{messages:?}"
    );
}
