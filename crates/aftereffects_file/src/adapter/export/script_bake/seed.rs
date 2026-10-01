//! Public-schema adapter for the runtime's append-only property seed IDs.

use fx_keyframe_bake::identity::hash_random_seed_part;
use fx_schema::{PropType, PropertyTarget};

pub(super) fn prefix(target: &PropertyTarget) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    match target {
        PropertyTarget::LayerProperty(property) => {
            hash_random_seed_part(&mut hash, property.layer_id().value());
            hash_random_seed_part(&mut hash, property_id(property.property_type()));
        }
        PropertyTarget::EffectProperty(property) => {
            hash_random_seed_part(&mut hash, 31);
            hash_random_seed_part(&mut hash, property.effect_id().value());
            for byte in property.param_name().bytes() {
                hash_random_seed_part(&mut hash, u64::from(byte));
            }
        }
        PropertyTarget::FxItemProperty(property) => {
            hash_random_seed_part(&mut hash, 32);
            hash_random_seed_part(&mut hash, property.item_id().value());
            for byte in property.property_name().bytes() {
                hash_random_seed_part(&mut hash, u64::from(byte));
            }
        }
    }
    hash
}

// This adapter must agree with fx_composition::script's private-model adapter.
// Numeric IDs are append-only, not Rust enum discriminants or sorted names.
fn property_id(property: PropType) -> u64 {
    match property {
        PropType::PositionX => 0,
        PropType::PositionY => 1,
        PropType::PositionZ => 2,
        PropType::Opacity => 3,
        PropType::Rotation => 4,
        PropType::ScaleX => 5,
        PropType::ScaleY => 6,
        PropType::FillColor => 7,
        PropType::TextContent => 8,
        PropType::FontSize => 9,
        PropType::AudioGainLeft => 10,
        PropType::AudioGainRight => 11,
        PropType::AudioGainBoth => 12,
        PropType::MediaColor => 13,
        PropType::MediaLuminance => 14,
        PropType::MediaSourceAssetId => 15,
        PropType::AudioSourceAssetId => 16,
        PropType::RectRoundness => 17,
        PropType::StrokeEnabled => 18,
        PropType::StrokeColor => 19,
        PropType::StrokeWidth => 20,
        PropType::DropShadowEnabled => 21,
        PropType::DropShadowColor => 22,
        PropType::DropShadowOffset => 23,
        PropType::DropShadowBlurRadius => 24,
        PropType::DropShadowSpreadRadius => 25,
        PropType::FontFamily => 26,
        PropType::FontStyle => 27,
        PropType::ActiveRange => 28,
        PropType::SourceRange => 29,
        PropType::AudioVolume => 30,
        PropType::TrimStart => 31,
        PropType::TrimEnd => 32,
        PropType::TrimOffset => 33,
        PropType::RoundCornersRadius => 34,
        PropType::PolyStarPoints => 35,
        PropType::PolyStarPosition => 36,
        PropType::PolyStarRotation => 37,
        PropType::PolyStarOuterRadius => 38,
        PropType::PolyStarInnerRadius => 39,
        PropType::PolyStarOuterRoundness => 40,
        PropType::PolyStarInnerRoundness => 41,
        PropType::RotationX => 42,
        PropType::RotationY => 43,
        PropType::OrientationX => 44,
        PropType::OrientationY => 45,
        PropType::OrientationZ => 46,
        PropType::ShapePath => 47,
        PropType::EllipseSize => 48,
        PropType::EllipsePosition => 49,
        PropType::OffsetPathsAmount => 50,
        PropType::AnchorPointX => 51,
        PropType::AnchorPointY => 52,
        PropType::Tracking => 53,
        PropType::Leading => 54,
        PropType::Underline => 55,
        PropType::Strikethrough => 56,
        PropType::AllCaps => 57,
        PropType::StrokeDashOffset => 58,
        PropType::StrokeJoin => 59,
        PropType::StrokeMiterLimit => 60,
        PropType::Skew => 61,
        PropType::SkewAxis => 62,
        PropType::FillEnabled => 63,
        PropType::RectSize => 64,
        PropType::PaddingTop => 65,
        PropType::PaddingRight => 66,
        PropType::PaddingBottom => 67,
        PropType::PaddingLeft => 68,
        PropType::CornerRadiusTopLeft => 69,
        PropType::CornerRadiusTopRight => 70,
        PropType::CornerRadiusBottomRight => 71,
        PropType::CornerRadiusBottomLeft => 72,
    }
}
