//! Curves' table arbitrary state (`ADBE CurvesCustom-0001`).
//!
//! The definition table carries AE 26.5's default payload after the control's
//! `pard`; the instance property is a static arbitrary-data leaf whose `tdb4`
//! descriptor and record order match the native colour catalog. The payload is
//! built by `effects::curves` from typed FX curves or the default diagonal.

use super::{NativeEffect, arbitrary_descriptor, match_name, views};
use crate::{
    effects::curves::{CONTROL, MATCH_NAME, default_payload},
    rifx::{Chunk, RifxError},
};

pub(super) fn default_data() -> Result<Chunk, RifxError> {
    Chunk::data(*b"aRbp", default_payload())
}

/// Insert the Curves table leaf before the Compositing Options group. An
/// effect without an encoded table (project definitions) writes the default.
pub(super) fn insert_instance(
    group: &mut Chunk,
    effect: &NativeEffect,
    clock: crate::writer::keyframes::PropertyClock,
) -> Result<(), RifxError> {
    let mut payloads = effect
        .arbitrary
        .iter()
        .filter(|data| data.match_name == CONTROL);
    let payload = match (payloads.next(), payloads.next()) {
        (None, _) => default_payload(),
        (Some(data), None) => data.payload.clone(),
        (Some(_), Some(_)) => return Err(RifxError::Invalid("duplicate Curves table payload")),
    };
    let property = Chunk::list(
        *b"tdbs",
        vec![
            Chunk::data(*b"tdsb", 1_u32.to_be_bytes())?,
            views::name_payload("Curves")?,
            Chunk::data(*b"tdb4", arbitrary_descriptor(clock).encode())?,
            Chunk::data(*b"cdat", 0_u32.to_be_bytes())?,
        ],
    );
    let children = group
        .children_mut()
        .ok_or(RifxError::Invalid("missing native effect group"))?;
    let index = children
        .iter()
        .position(|chunk| {
            chunk.id() == *b"tdmn"
                && chunk
                    .data_payload()
                    .is_some_and(|name| name.starts_with(b"ADBE Effect Built In Params\0"))
        })
        .ok_or(RifxError::Invalid(
            "missing Curves property insertion point",
        ))?;
    // aRbs is a sibling of tdbs in this named property run, not its child.
    children.splice(
        index..index,
        [
            match_name(CONTROL)?,
            property,
            Chunk::list(*b"aRbs", vec![Chunk::data(*b"aRbp", payload)?]),
        ],
    );
    Ok(())
}

/// Whether `effect` may carry arbitrary payloads: Curves' table, or a
/// Lumetri arbitrary-data control with a payload of its default's length.
pub(super) fn validate(effect: &NativeEffect) -> Result<(), RifxError> {
    if effect.arbitrary.iter().all(|data| {
        (effect.match_name == MATCH_NAME && data.match_name == CONTROL)
            || (effect.match_name == super::lumetri::MATCH_NAME
                && super::lumetri::default_payload(&data.match_name)
                    .is_some_and(|default| default.len() == data.payload.len()))
    }) {
        Ok(())
    } else {
        Err(RifxError::Invalid(
            "native arbitrary data has no writer for this effect control",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{NativeArbitraryData, new_effect, plugin, plugin_with_clock};
    use super::*;

    fn catalog_plugins(chunks: &[Chunk], found: &mut Vec<Chunk>) {
        for pair in chunks.windows(2) {
            if pair[0].id() == *b"tdmn"
                && pair[0]
                    .data_payload()
                    .is_some_and(|name| name.starts_with(b"ADBE CurvesCustom\0"))
                && pair[1].list_kind() == Some(*b"sspc")
            {
                found.push(pair[1].clone());
            }
        }
        for chunk in chunks {
            if let Some(children) = chunk.children() {
                catalog_plugins(children, found);
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

    /// The `tdmn`, `tdbs` and `aRbs` records of the Curves table control.
    fn table_run(plugin: &Chunk) -> Vec<Chunk> {
        let group = child(plugin, *b"tdgp");
        let start = group
            .iter()
            .position(|c| {
                c.data_payload()
                    .is_some_and(|name| name.starts_with(b"ADBE CurvesCustom-0001\0"))
            })
            .unwrap();
        group[start..start + 3].to_vec()
    }

    #[test]
    fn fresh_curves_records_match_native_catalog_layout() {
        let native = crate::aep::Project::parse(include_bytes!(
            "../../../tests/fixtures/effects/color_catalog.aep"
        ))
        .unwrap();
        let mut found = Vec::new();
        catalog_plugins(&native.chunks, &mut found);
        assert_eq!(found.len(), 2, "project definition + layer instance");
        let effect = new_effect(MATCH_NAME, true, [320.0, 180.0]).unwrap();
        // The colour catalog is a 30 fps project; leaf descriptors carry the clock.
        let clock =
            crate::writer::PropertyClock::for_rate(crate::timing::FrameRate::new(30.0).unwrap())
                .unwrap();
        let fresh = plugin_with_clock(&effect, 0x1d, [320.0, 180.0], clock).unwrap();
        for catalog in &found {
            assert_eq!(child(&fresh, *b"parT"), child(catalog, *b"parT"));
            assert_eq!(table_run(&fresh), table_run(catalog));
        }
    }

    #[test]
    fn curves_instance_writes_the_encoded_table_and_rejects_foreign_payloads() {
        let mut effect = new_effect(MATCH_NAME, true, [320.0, 180.0]).unwrap();
        let mut payload = default_payload();
        payload[4 + 256] = 40;
        effect.arbitrary.push(NativeArbitraryData {
            match_name: CONTROL.into(),
            payload: payload.clone(),
        });
        let fresh = plugin(&effect, 7, [320.0, 180.0]).unwrap();
        let run = table_run(&fresh);
        assert_eq!(
            run[2].children().unwrap()[0].data_payload(),
            Some(payload.as_slice())
        );
        // The definition table keeps AE's default, never the owner's edit.
        let table = child(&fresh, *b"parT");
        assert!(
            table
                .iter()
                .any(|c| c.id() == *b"aRbp"
                    && c.data_payload() == Some(default_payload().as_slice()))
        );
        effect.arbitrary.push(effect.arbitrary[0].clone());
        assert!(plugin(&effect, 7, [320.0, 180.0]).is_err());
        let mut foreign = new_effect("ADBE CHANNEL MIXER", true, [320.0, 180.0]).unwrap();
        foreign.arbitrary.push(NativeArbitraryData {
            match_name: CONTROL.into(),
            payload,
        });
        assert!(plugin(&foreign, 7, [320.0, 180.0]).is_err());
    }
}
