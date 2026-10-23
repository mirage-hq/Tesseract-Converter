//! Levels' arbitrary-data Histogram control (`pard` kind 11).
//!
//! AE 26.5 saves both Levels plugins (`ADBE Pro Levels2`, `ADBE Easy Levels2`)
//! with one default payload after the Histogram declaration and one static
//! `tdbs` + `aRbs` leaf in the instance. The independently authored
//! `effects/catalog.aep` (both plugins, defaults), `effects/levels_controls.aep`
//! and the graded `color_ae_effect_compositing_options.aep` hold the same
//! 108-byte payload in all of these places, so it carries no levels value. It
//! is written verbatim so AE can load the instance; its layout is not
//! interpreted.

/// The Histogram control of either Levels plugin.
pub(super) const CONTROLS: [&str; 2] = ["ADBE Pro Levels2-0002", "ADBE Easy Levels2-0002"];

/// The native default payload as big-endian words.
const DEFAULT_WORDS: [u32; 27] = [
    0x000f_10a7,
    1,
    0,
    0x3f80_0000,
    0x3f80_0000,
    0,
    0x3f80_0000,
    0,
    0x3f80_0000,
    0x3f80_0000,
    0,
    0x3f80_0000,
    0,
    0x3f80_0000,
    0x3f80_0000,
    0,
    0x3f80_0000,
    0,
    0x3f80_0000,
    0x3f80_0000,
    0,
    0x3f80_0000,
    0,
    0x3f80_0000,
    0x3f80_0000,
    0,
    0x3f80_0000,
];

/// The native default payload of a Levels Histogram control.
pub(super) fn default_payload(control: &str) -> Option<Vec<u8>> {
    CONTROLS.contains(&control).then(|| {
        DEFAULT_WORDS
            .iter()
            .flat_map(|word| word.to_be_bytes())
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_defaults_cover_exactly_the_registry_arbitrary_controls() {
        for plugin in ["ADBE Pro Levels2", "ADBE Easy Levels2"] {
            let definition = crate::effects::definitions::definition(plugin).unwrap();
            let arbitrary: Vec<_> = definition
                .parameters
                .iter()
                .filter(|parameter| parameter.kind == 11)
                .map(|parameter| parameter.match_name.as_str())
                .collect();
            assert_eq!(arbitrary, [format!("{plugin}-0002")]);
            assert_eq!(default_payload(arbitrary[0]).unwrap().len(), 108);
        }
        assert_eq!(default_payload("ADBE Pro Levels2-0006"), None);
    }
}
