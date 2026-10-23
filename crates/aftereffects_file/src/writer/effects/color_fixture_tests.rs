//! Record-level completeness of fresh colour-effect plugins against the
//! natively saved AE 26.5 instances (graded fixtures and the colour catalog).
//!
//! Compared byte-for-byte: the whole declaration table (`parT`, including
//! arbitrary-data defaults) and every arbitrary-data instance leaf. AE also
//! saves topic start/end (kinds 13/14) and button (kind 15) leaves, which this
//! writer omits for every effect (Levels, Exposure, Tint, Ramp included); that
//! established omission is asserted explicitly rather than hidden.

use super::{NativeEffect, NativeEffectCompositing, new_effect, plugin_with_clock};
use crate::{effects::definitions, rifx::Chunk, writer::PropertyClock};

/// Property clock of the 30 fps colour fixtures (`0x7800` ticks).
fn fixture_clock() -> PropertyClock {
    PropertyClock::for_rate(crate::timing::FrameRate::new(30.0).unwrap()).unwrap()
}

/// Property clock of the 24 fps effect catalog (`0x6000` ticks).
const CATALOG_CLOCK: PropertyClock = PropertyClock::DEFAULT;

/// Every plugin record of `match_name` (project definition + layer instance).
fn native_plugins(chunks: &[Chunk], match_name: &str, found: &mut Vec<Chunk>) {
    let mut key = match_name.as_bytes().to_vec();
    key.push(0);
    for pair in chunks.windows(2) {
        if pair[0].id() == *b"tdmn"
            && pair[0]
                .data_payload()
                .is_some_and(|name| name.starts_with(&key))
            && pair[1].list_kind() == Some(*b"sspc")
        {
            found.push(pair[1].clone());
        }
    }
    for chunk in chunks {
        if let Some(children) = chunk.children() {
            native_plugins(children, match_name, found);
        }
    }
}

fn child(chunk: &Chunk, kind: [u8; 4]) -> &[Chunk] {
    chunk
        .children()
        .unwrap()
        .iter()
        .find(|c| c.list_kind() == Some(kind))
        .and_then(Chunk::children)
        .unwrap()
}

fn name(chunk: &Chunk) -> Option<&str> {
    (chunk.id() == *b"tdmn")
        .then(|| chunk.data_payload())
        .flatten()
        .and_then(|bytes| std::str::from_utf8(bytes.split(|b| *b == 0).next()?).ok())
}

/// Instance `tdmn` + `tdbs` + `aRbs` runs of arbitrary-data controls.
fn arbitrary_runs(group: &[Chunk]) -> Vec<Vec<Chunk>> {
    group
        .windows(3)
        .filter(|run| run[2].list_kind() == Some(*b"aRbs"))
        .map(<[Chunk]>::to_vec)
        .collect()
}

/// The declaration table with each `pard` label cleared after its NUL. AE
/// leaves stale bytes of a longer earlier label there in four Lumetri
/// checkbox declarations (` \0eative`, ` \0rves`, ` \0lor Wheels`,
/// ` \0gnette`); the registry keeps only the terminated label.
fn terminated_labels(table: &[Chunk]) -> Vec<Chunk> {
    table
        .iter()
        .map(|chunk| match chunk.data_payload() {
            Some(payload) if chunk.id() == *b"pard" && payload.len() >= 48 => {
                let mut payload = payload.to_vec();
                if let Some(end) = payload[16..48].iter().position(|byte| *byte == 0) {
                    payload[16 + end..48].fill(0);
                }
                Chunk::data(*b"pard", payload).unwrap()
            }
            _ => chunk.clone(),
        })
        .collect()
}

fn names(group: &[Chunk]) -> Vec<String> {
    group.iter().filter_map(name).map(str::to_owned).collect()
}

/// A fresh effect at its registry defaults with `values` applied.
fn authored(match_name: &str, values: &[(&str, f64)]) -> NativeEffect {
    let mut effect = new_effect(match_name, true, [320.0, 180.0]).unwrap();
    for (control, value) in values {
        let property = effect
            .properties
            .iter_mut()
            .find(|property| property.match_name == *control)
            .unwrap();
        property.values = vec![*value];
    }
    effect
}

/// Compare a fresh plugin carrying the fixture's authored values with every
/// native record of the effect (AE saves the project-level definition with
/// the same state) and return the number of native arbitrary-data leaves.
fn assert_complete(fixture: &[u8], effect: &NativeEffect, clock: PropertyClock) -> usize {
    let match_name = effect.match_name.as_str();
    let fresh = plugin_with_clock(effect, 0x0e, [320.0, 180.0], clock).unwrap();
    let fresh_group = child(&fresh, *b"tdgp");
    let fresh_names = names(fresh_group);
    let definition = definitions::definition(match_name).unwrap();
    let native = crate::aep::Project::parse(fixture).unwrap();
    let mut found = Vec::new();
    native_plugins(&native.chunks, match_name, &mut found);
    assert_eq!(
        found.len(),
        2,
        "{match_name}: project definition + layer instance"
    );
    let mut arbitrary = 0;
    for record in &found {
        assert_eq!(
            terminated_labels(child(record, *b"parT")),
            child(&fresh, *b"parT"),
            "{match_name} declaration table"
        );
        let group = child(record, *b"tdgp");
        let runs = arbitrary_runs(group);
        assert_eq!(
            runs,
            arbitrary_runs(fresh_group),
            "{match_name} arbitrary leaves"
        );
        arbitrary = runs.len();
        // Native leaves absent from the fresh instance are only topic/button
        // leaves; shared leaves keep the native relative order. (AE omits
        // default-valued scalar leaves, so the native list is a subset.)
        let mut remaining = fresh_names.iter();
        for native_name in names(group) {
            if fresh_names.contains(&native_name) {
                assert!(
                    remaining.any(|fresh| *fresh == native_name),
                    "{native_name} out of native order"
                );
                continue;
            }
            let kind = definition
                .parameters
                .iter()
                .find(|parameter| parameter.match_name == native_name)
                .map(|parameter| parameter.kind);
            assert!(
                matches!(kind, Some(13..=15)),
                "{match_name}: native leaf {native_name} (kind {kind:?}) missing from the fresh instance"
            );
        }
    }
    arbitrary
}

#[test]
fn fresh_lumetri_records_match_native_fixtures_byte_for_byte() {
    // Lumetri mirrors each float slider's value into its declaration, so the
    // graded fixture is compared with a fresh effect carrying its values.
    let graded = authored(
        "ADBE Lumetri",
        &[
            ("ADBE Lumetri-0007", 30.0),
            ("ADBE Lumetri-0008", -15.0),
            ("ADBE Lumetri-0020", 130.0),
            ("ADBE Lumetri-0011", 0.5),
            ("ADBE Lumetri-0012", 30.0),
            ("ADBE Lumetri-0013", -25.0),
            ("ADBE Lumetri-0014", 25.0),
            ("ADBE Lumetri-0015", 15.0),
            ("ADBE Lumetri-0016", -15.0),
        ],
    );
    for (fixture, effect) in [
        (
            include_bytes!("../../../tests/fixtures/effects/color_catalog.aep").as_slice(),
            authored("ADBE Lumetri", &[]),
        ),
        (
            include_bytes!("../../../tests/fixtures/color_ae_lumetri_basic.aep").as_slice(),
            graded,
        ),
    ] {
        assert_eq!(assert_complete(fixture, &effect, fixture_clock()), 24);
    }
}

#[test]
fn fresh_color_effect_records_are_complete_against_native_fixtures() {
    // Color Balance, Channel Mixer and Selective Color keep their declarations
    // at the registry defaults even in the graded fixtures.
    for (fixture, match_name, arbitrary) in [
        (
            include_bytes!("../../../tests/fixtures/color_ae_color_balance.aep").as_slice(),
            "ADBE Color Balance 2",
            0,
        ),
        (
            include_bytes!("../../../tests/fixtures/color_ae_channel_mixer.aep").as_slice(),
            "ADBE CHANNEL MIXER",
            0,
        ),
        (
            include_bytes!("../../../tests/fixtures/color_ae_selective_color.aep").as_slice(),
            "ADBE SelectiveColor",
            0,
        ),
        (
            include_bytes!("../../../tests/fixtures/effects/color_catalog.aep").as_slice(),
            "ADBE CurvesCustom",
            1,
        ),
    ] {
        assert_eq!(
            assert_complete(fixture, &authored(match_name, &[]), fixture_clock()),
            arbitrary,
            "{match_name}"
        );
    }
}

#[test]
fn fresh_levels_records_carry_the_native_histogram_default() {
    // Both Levels plugins at their defaults in the independently authored
    // 24 fps effect catalog: the declaration table (with the Histogram
    // payload) and the Histogram instance leaf are byte-equal.
    let catalog = include_bytes!("../../../tests/fixtures/effects/catalog.aep");
    for match_name in ["ADBE Pro Levels2", "ADBE Easy Levels2"] {
        assert_eq!(
            assert_complete(catalog, &authored(match_name, &[]), CATALOG_CLOCK),
            1,
            "{match_name}"
        );
    }
}

/// The `ADBE Effect Built In Params` group and arbitrary-data leaves of a
/// plugin's instance group.
fn compositing_and_arbitrary(plugin: &Chunk) -> (Chunk, Vec<Vec<Chunk>>) {
    let group = child(plugin, *b"tdgp");
    let options = group
        .windows(2)
        .find(|pair| name(&pair[0]) == Some("ADBE Effect Built In Params"))
        .map(|pair| pair[1].clone())
        .expect("Compositing Options group");
    (options, arbitrary_runs(group))
}

#[test]
fn fresh_compositing_options_match_the_native_mask_reference_and_opacity_records() {
    // color_ae_effect_compositing_options.aep: Levels (Individual Controls)
    // with Gamma 0.5, Mask Reference 1 -> layer Mask 1 (identity 1) and
    // Effect Opacity 50, saved by AE 26.5 at 30 fps.
    let fixture = include_bytes!("../../../tests/fixtures/color_ae_effect_compositing_options.aep");
    let mut effect = authored("ADBE Pro Levels2", &[("ADBE Pro Levels2-0006", 0.5)]);
    effect.compositing = NativeEffectCompositing {
        effect_opacity_percent: 50.0,
        mask_identities: vec![crate::writer::native_mask_identity(0).unwrap()],
        unresolved_mask_references: Vec::new(),
    };
    let fresh = plugin_with_clock(&effect, 0x0e, [320.0, 180.0], fixture_clock()).unwrap();
    let native = crate::aep::Project::parse(fixture).unwrap();
    let mut found = Vec::new();
    native_plugins(&native.chunks, "ADBE Pro Levels2", &mut found);
    assert_eq!(found.len(), 2, "project definition + layer instance");
    let (fresh_options, fresh_arbitrary) = compositing_and_arbitrary(&fresh);
    for record in &found {
        let (options, arbitrary) = compositing_and_arbitrary(record);
        assert_eq!(fresh_options, options, "Compositing Options record");
        assert_eq!(fresh_arbitrary, arbitrary, "Levels Histogram leaf");
    }
    // Defaults write the empty group, as before; each control alone keeps
    // its own native record.
    let neutral = authored("ADBE Pro Levels2", &[]);
    let (empty, _) = compositing_and_arbitrary(
        &plugin_with_clock(&neutral, 0x0e, [320.0, 180.0], fixture_clock()).unwrap(),
    );
    assert_eq!(
        empty
            .children()
            .unwrap()
            .iter()
            .filter_map(name)
            .collect::<Vec<_>>(),
        ["ADBE Group End"]
    );
    // A reference must name one of the owner's masks.
    assert!(effect.compositing.validate(0).is_err());
    assert!(effect.compositing.validate(1).is_ok());
    effect.compositing.effect_opacity_percent = 101.0;
    assert!(effect.compositing.validate(1).is_err());
}

#[test]
fn registered_lumetri_definition_keeps_registry_defaults_at_the_default_clock() {
    // The project-level definition `register_definitions` writes for a graded
    // Lumetri instance (Exposure 0.5).
    let graded = authored("ADBE Lumetri", &[("ADBE Lumetri-0011", 0.5)]);
    let mut chunks = vec![
        Chunk::list(*b"Pefl", Vec::new()),
        super::effect_parade(&[graded], 0x0e, [320.0, 180.0]).unwrap(),
    ];
    super::register_definitions(&mut chunks).unwrap();
    let registry = chunks
        .iter()
        .find(|chunk| chunk.list_kind() == Some(*b"EfdG"))
        .unwrap();
    let mut registered = Vec::new();
    native_plugins(
        registry.children().unwrap(),
        "ADBE Lumetri",
        &mut registered,
    );
    assert_eq!(registered.len(), 1, "one Lumetri definition record");
    let registered = &registered[0];

    let native = |fixture: &[u8]| {
        let project = crate::aep::Project::parse(fixture).unwrap();
        let mut found = Vec::new();
        native_plugins(&project.chunks, "ADBE Lumetri", &mut found);
        found.swap_remove(0)
    };
    let catalog =
        native(include_bytes!("../../../tests/fixtures/effects/color_catalog.aep").as_slice());
    let lumetri_basic =
        native(include_bytes!("../../../tests/fixtures/color_ae_lumetri_basic.aep").as_slice());
    // Declarations stay at the registry defaults (the catalog's table); AE
    // mirrors the graded Exposure into its own definition, the writer does not.
    let declarations = child(registered, *b"parT");
    assert_eq!(declarations, terminated_labels(child(&catalog, *b"parT")));
    assert_ne!(
        declarations,
        terminated_labels(child(&lumetri_basic, *b"parT"))
    );

    // The 24 default arbitrary-data leaves carry the 24 fps clock (0x6000)
    // whatever the composition rate; the 30 fps native saves store 0x7800.
    let mut defaults = new_effect("ADBE Lumetri", true, [1.0; 2]).unwrap();
    defaults.properties.clear();
    let leaves = |clock| {
        arbitrary_runs(child(
            &plugin_with_clock(&defaults, 0, [1.0; 2], clock).unwrap(),
            *b"tdgp",
        ))
    };
    let registered_leaves = arbitrary_runs(child(registered, *b"tdgp"));
    assert_eq!(registered_leaves.len(), 24);
    assert_eq!(registered_leaves, leaves(CATALOG_CLOCK));
    assert_eq!(
        arbitrary_runs(child(&catalog, *b"tdgp")),
        leaves(fixture_clock())
    );
    assert_ne!(registered_leaves, leaves(fixture_clock()));
}
