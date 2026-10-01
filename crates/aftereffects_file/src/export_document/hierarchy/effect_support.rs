//! Native/FX joint support policy for consumer-demand cropping.
//! A finite FX shader footprint alone does not certify native AE support.

use fx_schema::effect::{EffectData, EffectPayload, EffectRecord, LayerEffect};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Support {
    Pointwise,
    /// Native support not yet independently established. Never choose a
    /// finite canvas from the FX effect's nominal UI radius alone.
    Full(&'static str),
}

pub(super) fn effect(record: &EffectRecord) -> Support {
    let payload = match record.data() {
        EffectData::Identified { enabled: false, .. } => return Support::Pointwise,
        EffectData::Identified { effect, .. } | EffectData::Legacy(effect) => effect,
    };
    match payload {
        EffectPayload::Known(LayerEffect::Exposure { .. } | LayerEffect::HueSaturation { .. }) => {
            Support::Pointwise
        }
        EffectPayload::Known(LayerEffect::Glow { .. }) => {
            Support::Full("Glow native finite reach at the crop boundary is not proved")
        }
        EffectPayload::Known(LayerEffect::GaussianBlur { .. }) => {
            Support::Full("Gaussian native finite reach at the crop boundary is not proved")
        }
        EffectPayload::Known(LayerEffect::ChromaticAberration { .. })
            if super::super::effects::unmapped_warning(record).is_some() =>
        {
            // This source control is not present in the generated native
            // consumer; it cannot expand that consumer's sampling demand.
            // Its separate omission diagnostic is still emitted by lowering.
            Support::Pointwise
        }
        _ => Support::Full("Effect has no proven native and FX support preimage"),
    }
}

pub(super) fn stack(records: &[EffectRecord]) -> Result<(), &'static str> {
    for record in records.iter().rev() {
        if let Support::Full(reason) = effect(record) {
            return Err(reason);
        }
    }
    Ok(())
}
