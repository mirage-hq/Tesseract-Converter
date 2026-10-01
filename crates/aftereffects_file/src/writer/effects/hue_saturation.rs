//! Hue/Saturation's Channel Range arbitrary state, constructed from typed values.
//! AE renders Master controls from this state, not the visible `pard` defaults.

use super::{NativeEffect, hue_master_fixed, match_name, views};
use crate::{
    rifx::{Chunk, RifxError},
    schema::view_records::StaticPropertyRecord,
};

const CHANNEL_RANGE: &str = "ADBE HUE SATURATION-0003";

fn data(master: [i32; 3]) -> Result<Chunk, RifxError> {
    // AE26's six default channel ranges (red through magenta). Each contains
    // four hue boundaries and three zero H/S/L offsets. Per-channel edits are
    // unsupported; these are typed defaults, not replayed source-project bytes.
    const RANGES: [[i32; 4]; 6] = [
        [315, 345, 15, 45],
        [15, 45, 75, 105],
        [75, 105, 135, 165],
        [135, 165, 195, 225],
        [195, 225, 255, 285],
        [255, 285, 315, 345],
    ];
    let mut bytes = Vec::with_capacity(180);
    for value in master {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    for range in RANGES {
        for value in range.into_iter().chain([0, 0, 0]) {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
    }
    Chunk::data(*b"aRbp", bytes)
}

pub(super) fn default_data() -> Result<Chunk, RifxError> {
    data([0; 3])
}

pub(super) fn insert_instance(group: &mut Chunk, effect: &NativeEffect) -> Result<(), RifxError> {
    let mut master = [0; 3];
    for (slot, name) in master.iter_mut().zip([
        "ADBE HUE SATURATION-0004",
        "ADBE HUE SATURATION-0005",
        "ADBE HUE SATURATION-0006",
    ]) {
        // The global definition intentionally has no owner properties.
        let Some(property) = effect
            .properties
            .iter()
            .find(|property| property.match_name == name)
        else {
            continue;
        };
        let value = *property
            .values
            .first()
            .ok_or(RifxError::Invalid("missing Hue/Saturation Master value"))?;
        hue_master_fixed(value)?;
        if value.fract() != 0.0 {
            return Err(RifxError::Invalid(
                "fractional Hue/Saturation Master state must be lowered to an integer",
            ));
        }
        // The validated 16:16 UI range is strictly inside i32; no truncation.
        *slot = value as i32;
    }
    let mut descriptor = StaticPropertyRecord::new(1, 7, 1, 0x60007, 0x10008, 0, false);
    descriptor.set_initialized();
    let property = Chunk::list(
        *b"tdbs",
        vec![
            Chunk::data(*b"tdsb", 1_u32.to_be_bytes())?,
            views::name_payload("Channel Range")?,
            Chunk::data(*b"tdb4", descriptor.encode())?,
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
                && chunk.data_payload().is_some_and(|name| {
                    name.starts_with(b"ADBE HUE SATURATION-0004\0")
                        || name.starts_with(b"ADBE Effect Built In Params\0")
                })
        })
        .ok_or(RifxError::Invalid(
            "missing Hue/Saturation property insertion point",
        ))?;
    // aRbs is a sibling of tdbs in this named property run, not its child.
    children.splice(
        index..index,
        [
            match_name(CHANNEL_RANGE)?,
            property,
            Chunk::list(*b"aRbs", vec![data(master)?]),
        ],
    );
    Ok(())
}
