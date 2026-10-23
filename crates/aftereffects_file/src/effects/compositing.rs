//! Per-effect AE Compositing Options (`ADBE Effect Built In Params`).
//!
//! The independent AE 26.5 catalog readout names three children: `ADBE Effect
//! Mask Parade` (effect masks), `ADBE Effect Mask Opacity` (Effect Opacity)
//! and `ADBE Force CPU GPU` (render device choice). AE omits the explicit
//! records while they hold their defaults.
//!
//! The natively authored `color_ae_effect_compositing_options.aep` pins the
//! nondefault storage: Effect Opacity is a percentage leaf (`50` with a `100`
//! maximum), and each `ADBE Effect Mask` group holds one `ADBE Effect Path
//! Stream Ref` leaf whose `tdli` word is the referenced layer mask's native
//! identity (the `mkif` identity word, as for Text Path). Identities resolve to
//! FX masks only after the owner's masks are imported.

use fx_schema::{
    LayerEffect,
    effect::{EffectCompositingOptions, UnitInterval},
};

use crate::{
    properties::{self, NumericProperty},
    rifx::Chunk,
};

pub(crate) const MATCH_NAME: &str = "ADBE Effect Built In Params";
const MASK_PARADE: &str = "ADBE Effect Mask Parade";
const MASK: &str = "ADBE Effect Mask";
const MASK_REFERENCE: &str = "ADBE Effect Path Stream Ref";
const OPACITY: &str = "ADBE Effect Mask Opacity";
const RENDER_DEVICE: &str = "ADBE Force CPU GPU";
const GROUP_END: &str = "ADBE Group End";

/// Explicit Compositing Options of one native effect instance.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NativeCompositing {
    /// Explicit Effect Opacity percentage; `None` is AE's omitted default 100.
    pub opacity: Option<NumericProperty>,
    /// One entry per effect mask in parade order: the referenced native mask
    /// identity, or why the entry is unreadable.
    pub mask_references: Vec<Result<u32, String>>,
}

/// Decode the explicit options group. `None` means AE stored no group, which is
/// the default. Duplicate or unknown children are errors, not silently ignored.
pub(crate) fn read(run: Option<&[Chunk]>) -> Result<NativeCompositing, String> {
    let Some(run) = run else {
        return Ok(NativeCompositing::default());
    };
    let options = named_runs(run)?;
    let mut result = NativeCompositing::default();
    let mut seen = std::collections::HashSet::new();
    for (name, run) in options {
        if !seen.insert(name) {
            return Err(format!("duplicate Compositing Options control {name}"));
        }
        match name {
            MASK_PARADE => {
                result.mask_references = named_runs(run)?
                    .into_iter()
                    .map(|(name, run)| match name {
                        MASK => mask_reference(run),
                        other => Err(format!("unknown effect mask entry {other}")),
                    })
                    .collect();
            }
            OPACITY => {
                let leaf = properties::unique_list(run, *b"tdbs").map_err(|e| e.to_string())?;
                result.opacity = Some(
                    properties::read_numeric(leaf).map_err(|e| format!("Effect Opacity: {e}"))?,
                );
            }
            // A CPU/GPU device preference has no pixel semantics in FX.
            RENDER_DEVICE => {}
            _ => return Err(format!("unknown Compositing Options control {name}")),
        }
    }
    Ok(result)
}

/// Named children of one `tdgp` group, excluding its terminator.
fn named_runs(run: &[Chunk]) -> Result<Vec<(&str, &[Chunk])>, String> {
    Ok(
        properties::runs(properties::unique_list(run, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|(name, _)| *name != GROUP_END)
            .collect(),
    )
}

/// The static native mask identity of one `ADBE Effect Mask` group.
fn mask_reference(run: &[Chunk]) -> Result<u32, String> {
    let children = named_runs(run)?;
    let [(MASK_REFERENCE, reference)] = children.as_slice() else {
        return Err("effect mask group does not hold exactly one mask reference".into());
    };
    let leaf = properties::unique_list(reference, *b"tdbs").map_err(|e| e.to_string())?;
    let numeric = properties::read_numeric(leaf).map_err(|e| e.to_string())?;
    if numeric.animated || numeric.expression_enabled {
        return Err("animated or expression-driven mask reference".into());
    }
    let identity: [u8; 4] = properties::data(leaf, *b"tdli")
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "malformed mask reference identity".to_string())?;
    Ok(u32::from_be_bytes(identity))
}

/// Native Compositing Options lowered for one imported FX effect.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct LoweredCompositing {
    /// Options committed with the effect (Effect Opacity); `None` when neutral.
    pub options: Option<EffectCompositingOptions>,
    /// Native mask identities to resolve once the owner's masks are imported.
    pub mask_identities: Vec<u32>,
    pub warnings: Vec<String>,
}

/// Lower native Compositing Options onto one imported FX effect. Neutral native
/// options return no options, preserving the historical option-free record.
pub(crate) fn lower(
    effect: &LayerEffect,
    native: &Result<NativeCompositing, String>,
) -> LoweredCompositing {
    let native = match native {
        Ok(native) => native,
        Err(reason) => {
            return LoweredCompositing {
                warnings: vec![format!(
                    "Compositing Options unreadable ({reason}); effect applied at full strength to the whole layer"
                )],
                ..LoweredCompositing::default()
            };
        }
    };
    let mut warnings = Vec::new();
    let mut mask_identities = Vec::new();
    for (index, reference) in native.mask_references.iter().enumerate() {
        match reference {
            Ok(identity) => mask_identities.push(*identity),
            Err(reason) => warnings.push(format!(
                "Compositing Options / effect mask {}: {reason}; that mask restriction omitted",
                index + 1
            )),
        }
    }
    let mut opacity = UnitInterval::default();
    if let Some(numeric) = &native.opacity {
        let initial = numeric
            .keyframes
            .first()
            .map(|key| key.values.as_slice())
            .unwrap_or(&numeric.values);
        match initial {
            [percent] => match UnitInterval::new(*percent / 100.0) {
                Some(value) => {
                    opacity = value;
                    if numeric.animated || numeric.expression_enabled {
                        warnings.push("Compositing Options / Effect Opacity: FX effect opacity is static; animation/expression omitted, initial value retained".into());
                    }
                }
                None => warnings.push(format!(
                    "Compositing Options / Effect Opacity {percent}%: outside 0..100; omitted, effect applied at full strength"
                )),
            },
            _ => warnings.push(
                "Compositing Options / Effect Opacity: scalar value required; omitted, effect applied at full strength"
                    .into(),
            ),
        }
    }
    let options = EffectCompositingOptions {
        mask_references: Vec::new(),
        effect_opacity: opacity,
        qualifier: None,
    };
    if options.is_neutral() && mask_identities.is_empty() {
        return LoweredCompositing {
            options: None,
            mask_identities,
            warnings,
        };
    }
    if !effect.compositing_class().allows_compositing() {
        warnings.push(format!(
            "Compositing Options (Effect Opacity / effect masks): FX {:?}-class effects reject compositing options; omitted, effect applied at full strength to the whole layer",
            effect.compositing_class()
        ));
        return LoweredCompositing {
            options: None,
            mask_identities: Vec::new(),
            warnings,
        };
    }
    LoweredCompositing {
        options: (!options.is_neutral()).then_some(options),
        mask_identities,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::properties::NumericValueKind;

    fn scalar(value: f64) -> NumericProperty {
        NumericProperty {
            values: vec![value],
            animated: false,
            expression_enabled: false,
            expression_present: false,
            dimensions_separated: false,
            keyframes: Vec::new(),
            value_kind: NumericValueKind::Continuous,
        }
    }

    fn grade() -> LayerEffect {
        LayerEffect::PrimaryGrade(Default::default())
    }

    fn native(
        opacity: Option<f64>,
        masks: Vec<Result<u32, String>>,
    ) -> Result<NativeCompositing, String> {
        Ok(NativeCompositing {
            opacity: opacity.map(scalar),
            mask_references: masks,
        })
    }

    #[test]
    fn compositing_defaults_keep_the_option_free_record() {
        assert_eq!(
            lower(&grade(), &Ok(NativeCompositing::default())),
            LoweredCompositing::default()
        );
        assert_eq!(
            lower(&grade(), &native(Some(100.0), Vec::new())),
            LoweredCompositing::default()
        );
    }

    #[test]
    fn compositing_effect_opacity_percent_maps_to_pixel_colour_effects_only() {
        let half = native(Some(50.0), Vec::new());
        let lowered = lower(&grade(), &half);
        assert_eq!(lowered.options.unwrap().effect_opacity.value(), 0.5);
        assert!(lowered.warnings.is_empty(), "{:?}", lowered.warnings);
        let matte = LayerEffect::SimpleChoker { choke: None };
        let lowered = lower(&matte, &half);
        assert!(lowered.options.is_none());
        assert!(
            lowered
                .warnings
                .iter()
                .any(|w| w.contains("reject compositing options"))
        );
    }

    #[test]
    fn compositing_mask_identities_wait_for_owner_masks_and_bad_entries_are_diagnosed() {
        let masked = native(None, vec![Ok(7), Err("malformed".into())]);
        let lowered = lower(&grade(), &masked);
        assert_eq!(lowered.options, None);
        assert_eq!(lowered.mask_identities, vec![7]);
        assert!(lowered.warnings[0].contains("effect mask 2: malformed"));
        let matte = LayerEffect::SimpleChoker { choke: None };
        let lowered = lower(&matte, &masked);
        assert!(lowered.mask_identities.is_empty() && lowered.options.is_none());
        let lowered = lower(&grade(), &native(Some(150.0), Vec::new()));
        assert!(lowered.options.is_none());
        assert!(
            lowered
                .warnings
                .iter()
                .any(|w| w.contains("outside 0..100"))
        );
        let lowered = lower(&grade(), &Err("duplicate".into()));
        assert!(lowered.options.is_none() && lowered.warnings[0].contains("unreadable"));
    }

    #[test]
    fn compositing_reader_rejects_unknown_and_duplicate_controls() {
        fn name(value: &str) -> Chunk {
            let mut bytes = vec![0; 40];
            bytes[..value.len()].copy_from_slice(value.as_bytes());
            Chunk::data(*b"tdmn", bytes).unwrap()
        }
        assert_eq!(read(None), Ok(NativeCompositing::default()));
        let masks = vec![
            name(MASK_PARADE),
            Chunk::list(
                *b"tdgp",
                vec![
                    name("ADBE Effect Mask"),
                    Chunk::list(*b"tdgp", vec![]),
                    name("ADBE Group End"),
                ],
            ),
            name(RENDER_DEVICE),
            Chunk::list(*b"tdbs", vec![]),
        ];
        let group = vec![Chunk::list(*b"tdgp", masks.clone())];
        let references = read(Some(&group)).unwrap().mask_references;
        assert_eq!(references.len(), 1);
        assert!(
            references[0].is_err(),
            "a reference-less mask entry is unreadable"
        );
        let mut duplicate = masks.clone();
        duplicate.extend(masks[..2].iter().cloned());
        assert!(read(Some(&[Chunk::list(*b"tdgp", duplicate)])).is_err());
        let unknown = vec![Chunk::list(
            *b"tdgp",
            vec![name("Unknown Option"), Chunk::list(*b"tdgp", vec![])],
        )];
        assert!(read(Some(&unknown)).is_err());
    }
}
