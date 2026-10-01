//! The text shadow slots of the Source Text document table.
//!
//! Slot evidence comes from pinned Premiere projects. The caption style preview
//! (`ArbVideoComponentParam` 41) of `practice_files_transcription_magic.prproj`
//! (SHA-256 `a6aab608caafb9b19ef1900b815c04e3a6c0a32a4dd9d989527d9d8d430e28a1`,
//! saved by Premiere 25.5 in this FlatBuffer layout) and all 42 of its caption
//! cues store an enabled black shadow: slot 10 is the color table, 11 the
//! enabled flag, and slots 12, 14, 15 and 16 store 100, 3, 6 and 12. The 24
//! disabled shadows of `2128_aiedit_en_na_multicreators.prproj` (SHA-256
//! `f093fb0eba054620a1cbcfff6f820d7261e06792d2e2a7a4bf11d2ff06e5830d`,
//! Premiere 26.5.1) store only slots 14 to 16 (28.125, 49.479164 and 250).
//! Slots 12 to 16 are opacity, angle, distance, size and blur, the order of
//! Premiere's Shadow controls. No corpus payload stores slot 13, but AME
//! renders of the crate-encoded `premiere_isolated_text_shadow_stroke`
//! fixture verify all five: Premiere reads slot 13 as the angle, and copies
//! with one of slots 12, 14, 15 or 16 byte-edited change only that shadow
//! property. `push` writes all of them.

use super::{color, color_table, document, slot, Table, TableOffset};
use crate::error::{unsupported, Result};
use crate::schema::{text::PrRgb, text_shadow::PrTextShadow};
use flatbuffers::FlatBufferBuilder;

/// Values of the slots Premiere omits, INFERRED from its legacy JSON defaults
/// (`mShadowColor` 0x3F3F3F, `mShadowOpacity` 75, `mShadowAngle` 135,
/// `mShadowOffset` 7, `mShadowSize` 0, `mShadowBlur` 40 on every untouched
/// title in `lower_third.prproj`, SHA-256
/// `a612c8db1cc74ff75aa9b0d45f04d930bb948692ce60bd9c611adbd24ce284eb`). The
/// FlatBuffer omits values equal to its schema defaults: every stored value in
/// the pinned payloads above differs from these defaults, and Premiere 26.5.1
/// renders of generated graphics show an omitted text size and fill at their
/// legacy defaults (100 px, white). No render verified these shadow defaults.
/// If one differs, an imported shadow with that slot absent changes; for the
/// omitted corpus angle, the offset error is at most twice the distance.
const OMITTED: PrTextShadow = PrTextShadow {
    color: PrRgb([0x3F; 3]),
    opacity: 75.0,
    angle: 135.0,
    distance: 7.0,
    size: 0.0,
    blur: 40.0,
};

/// Decode the document's shadow; `None` when it is disabled.
///
/// Disabled values stay unmodeled. Value ranges are left to the conversion
/// ([`PrTextShadow::validate`]), which omits only an out-of-range shadow.
pub(super) fn decode(doc: Table<'_>) -> Result<Option<PrTextShadow>> {
    match doc.u8(document::SHADOW_ENABLED)?.unwrap_or(0) {
        0 => return Ok(None),
        1 => {}
        other => return Err(unsupported(format!("invalid text shadow flag {other}"))),
    }
    let [opacity, angle, distance, size, blur] = document::SHADOW_VALUES;
    let value = |slot, omitted| -> Result<f32> { Ok(doc.f32(slot)?.unwrap_or(omitted)) };
    Ok(Some(PrTextShadow {
        color: doc
            .table(document::SHADOW_COLOR)?
            .map(color)
            .transpose()?
            .unwrap_or(OMITTED.color),
        opacity: value(opacity, OMITTED.opacity)?,
        angle: value(angle, OMITTED.angle)?,
        distance: value(distance, OMITTED.distance)?,
        size: value(size, OMITTED.size)?,
        blur: value(blur, OMITTED.blur)?,
    }))
}

/// The shadow color table, which must be built before the document table.
///
/// An out-of-range shadow fails the encode: the conversion omits such a
/// shadow before writing, and [`PrText::validate`] leaves it to the conversion.
///
/// [`PrText::validate`]: crate::schema::text::PrText::validate
pub(super) fn color_offset(
    fbb: &mut FlatBufferBuilder<'_>,
    shadow: Option<PrTextShadow>,
) -> crate::format::Result<Option<TableOffset>> {
    let Some(shadow) = shadow else {
        return Ok(None);
    };
    shadow.validate()?;
    Ok(Some(color_table(fbb, shadow.color)))
}

/// Write every shadow slot explicitly, so that Premiere never reads the
/// inferred defaults in place of an exported value.
pub(super) fn push(
    fbb: &mut FlatBufferBuilder<'_>,
    shadow: Option<PrTextShadow>,
    color: Option<TableOffset>,
) {
    let (Some(shadow), Some(color)) = (shadow, color) else {
        return;
    };
    fbb.push_slot_always(slot(document::SHADOW_COLOR), color);
    fbb.push_slot_always(slot(document::SHADOW_ENABLED), 1_u8);
    let values = [
        shadow.opacity,
        shadow.angle,
        shadow.distance,
        shadow.size,
        shadow.blur,
    ];
    for (index, value) in document::SHADOW_VALUES.into_iter().zip(values) {
        fbb.push_slot_always(slot(index), value);
    }
}
