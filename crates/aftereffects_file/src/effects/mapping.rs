//! Adobe Effect Parade match names to persisted FX scalar properties.
//!
//! IDs below come from the independently AE-authored catalog receipt. A row
//! identifies a convertible *control*, not render fidelity or Adobe acceptance.
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Scale {
    Identity,
    Factor(f64),
    Width,
    Height,
    /// Both noise offset axes use the layer width as the denominator.
    WidthPercent,
}

impl Scale {
    /// Multiplier for a native value; host dimensions are layer-content pixels.
    pub(crate) fn factor(self, size: [f64; 2]) -> Option<f64> {
        let factor = match self {
            Self::Identity => 1.0,
            Self::Factor(value) => value,
            Self::Width if size[0] > 0.0 => 1.0 / size[0],
            Self::Height if size[1] > 0.0 => 1.0 / size[1],
            Self::WidthPercent if size[0] > 0.0 => 100.0 / size[0],
            Self::Width | Self::Height | Self::WidthPercent => return None,
        };
        factor.is_finite().then_some(factor)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Field {
    pub native: &'static str,
    pub field: &'static str,
    pub param: &'static str,
    pub component: usize,
    pub scale: Scale,
    pub offset: f64,
    pub boolean: bool,
    pub animated: bool,
}

impl Field {
    /// AE's point origin is the layer center; the shader noise origin is zero.
    /// Callers must use this for both static values and each animation knot.
    pub(crate) fn offset(self, size: [f64; 2]) -> Option<f64> {
        let offset = if self.param == "offsetX" {
            self.offset - 50.0
        } else if self.param == "offsetY" {
            self.offset - 50.0 * size[1] / size[0]
        } else {
            self.offset
        };
        offset.is_finite().then_some(offset)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Mapping {
    pub native: &'static str,
    pub fx_type: &'static str,
    pub fields: &'static [Field],
    /// Non-affine controls, modes and fidelity caveats must be diagnosed by callers.
    pub note: &'static str,
}

macro_rules! field {
    ($native:literal, $field:literal) => {
        Field {
            native: $native,
            field: $field,
            param: $field,
            component: 0,
            scale: Scale::Identity,
            offset: 0.0,
            boolean: false,
            animated: true,
        }
    };
    ($native:literal, $field:literal, $component:expr, $scale:expr, $offset:expr, $boolean:expr, $animated:expr) => {
        Field {
            native: $native,
            field: $field,
            param: $field,
            component: $component,
            scale: $scale,
            offset: $offset,
            boolean: $boolean,
            animated: $animated,
        }
    };
}
macro_rules! mapping {
    ($native:literal => $fx:literal, $note:literal; $($field:expr),* $(,)?) => {
        Mapping { native: $native, fx_type: $fx, note: $note, fields: &[$($field),*] }
    };
}

/// Native IDs are full match names (not suffixes or translated UI labels).
static MAPPINGS: &[Mapping] = &[
    mapping!("ADBE Gaussian Blur 2" => "gaussianBlur", "Blur Dimensions must be Both; repeatEdgePixels is static.";
        field!("ADBE Gaussian Blur 2-0001", "blurriness"),
        field!("ADBE Gaussian Blur 2-0003", "repeatEdgePixels", 0, Scale::Identity, 0.0, true, false)),
    mapping!("ADBE Glo2" => "glow", "Only the default glow source/composite/color mode is modeled; other controls are omitted.";
        field!("ADBE Glo2-0002", "glowThreshold", 0, Scale::Factor(100.0 / 255.0), 0.0, false, true),
        field!("ADBE Glo2-0003", "glowRadius"), field!("ADBE Glo2-0004", "glowIntensity")),
    mapping!("ADBE Motion Blur" => "directionalBlur", "AE direction/length units are already FX units.";
        field!("ADBE Motion Blur-0001", "direction"), field!("ADBE Motion Blur-0002", "blurLength")),
    mapping!("ADBE OFMotionBlur" => "pixelMotionBlur", "Shutter Control popup (1 Manual, 2 Automatic) is static; automatic ignores shutter keys; unsupported popup animation diagnosed.";
        field!("ADBE OFMotionBlur-0002", "shutterAngle"), field!("ADBE OFMotionBlur-0003", "shutterSamples"),
        field!("ADBE OFMotionBlur-0004", "vectorDetail")),
    mapping!("ADBE Mosaic" => "mosaic", "Sharp Colors is static, not an effectProperty target.";
        field!("ADBE Mosaic-0001", "horizontalBlocks"), field!("ADBE Mosaic-0002", "verticalBlocks"),
        field!("ADBE Mosaic-0003", "sharpColors", 0, Scale::Identity, 0.0, true, false)),
    mapping!("ADBE Shift Channels" => "shiftChannels", "Only own-channel or Full On/Full Off red/green/blue routing; alpha/luma/hue/cross-channel routes and popup animation are diagnosed.";),
    mapping!("ADBE Drop Shadow" => "dropShadow", "A static direction projects native distance keys into editable Vector2 offset keys; animated direction or expressions retain an initial offset. Softness is approximated as half the native control in existing FX sigma units, including editable scalar keys. Shadow Only, spread and native shadow kernel/edge phase remain approximate.";
        field!("ADBE Drop Shadow-0005", "blurRadius", 0, Scale::Factor(0.5), 0.0, false, true)),
    mapping!("ADBE Brightness & Contrast 2" => "brightnessContrast", "Legacy/HDR mode is not modeled.";
        field!("ADBE Brightness & Contrast 2-0001", "brightness"), field!("ADBE Brightness & Contrast 2-0002", "contrast")),
    mapping!("ADBE HUE SATURATION" => "hueSaturation", "Only Master channel is represented; other channel ranges are unsupported. FX HSV Master transfer differs from Adobe's native Hue/Saturation on chromatic colors; static values are editable native approximations, not proven render-equivalent.";
        field!("ADBE HUE SATURATION-0004", "hue"), field!("ADBE HUE SATURATION-0005", "saturation"),
        field!("ADBE HUE SATURATION-0006", "lightness"),
        field!("ADBE HUE SATURATION-0007", "colorize", 0, Scale::Identity, 0.0, true, true),
        field!("ADBE HUE SATURATION-0008", "colorizeHue"), field!("ADBE HUE SATURATION-0009", "colorizeSaturation"),
        field!("ADBE HUE SATURATION-0010", "colorizeLightness")),
    mapping!("ADBE Radial Blur" => "radialBlur", "Only default Spin mode; Zoom, quality and random seed are omitted.";
        field!("ADBE Radial Blur-0001", "amount"),
        field!("ADBE Radial Blur-0002", "centerX", 0, Scale::Width, 0.0, false, true),
        field!("ADBE Radial Blur-0002", "centerY", 1, Scale::Height, 0.0, false, true)),
    mapping!("ADBE Easy Levels2" => "levels", "Only master RGB channel, no per-channel controls/clipping; AE normalized 0..1 levels expand to 0..255.";
        field!("ADBE Easy Levels2-0003", "inputBlack", 0, Scale::Factor(255.0), 0.0, false, true),
        field!("ADBE Easy Levels2-0004", "inputWhite", 0, Scale::Factor(255.0), 0.0, false, true),
        field!("ADBE Easy Levels2-0005", "gamma"),
        field!("ADBE Easy Levels2-0006", "outputBlack", 0, Scale::Factor(255.0), 0.0, false, true),
        field!("ADBE Easy Levels2-0007", "outputWhite", 0, Scale::Factor(255.0), 0.0, false, true)),
    mapping!("ADBE Pro Levels2" => "levels", "Only master RGB controls represented; per-channel RGB/alpha levels and clipping omitted. AE normalized 0..1 expanded to 0..255.";
        field!("ADBE Pro Levels2-0004", "inputBlack", 0, Scale::Factor(255.0), 0.0, false, true),
        field!("ADBE Pro Levels2-0005", "inputWhite", 0, Scale::Factor(255.0), 0.0, false, true),
        field!("ADBE Pro Levels2-0006", "gamma"),
        field!("ADBE Pro Levels2-0007", "outputBlack", 0, Scale::Factor(255.0), 0.0, false, true),
        field!("ADBE Pro Levels2-0008", "outputWhite", 0, Scale::Factor(255.0), 0.0, false, true)),
    mapping!("ADBE Bulge" => "bulge", "Taper/antialiasing are omitted; source pixel radii/center are normalized to the composition-sized destination Group plane.";
        field!("ADBE Bulge-0001", "horizontalRadius", 0, Scale::Width, 0.0, false, true),
        field!("ADBE Bulge-0002", "verticalRadius", 0, Scale::Height, 0.0, false, true),
        field!("ADBE Bulge-0003", "centerX", 0, Scale::Width, 0.0, false, true),
        field!("ADBE Bulge-0003", "centerY", 1, Scale::Height, 0.0, false, true),
        field!("ADBE Bulge-0004", "bulgeHeight"),
        field!("ADBE Bulge-0007", "pinning", 0, Scale::Identity, 0.0, true, true)),
    mapping!("ADBE Corner Pin" => "cornerPin", "Points are normalized to content bounds; AE pixels may be outside bounds.";
        field!("ADBE Corner Pin-0001", "upperLeftX", 0, Scale::Width, 0.0, false, true),
        field!("ADBE Corner Pin-0001", "upperLeftY", 1, Scale::Height, 0.0, false, true),
        field!("ADBE Corner Pin-0002", "upperRightX", 0, Scale::Width, 0.0, false, true),
        field!("ADBE Corner Pin-0002", "upperRightY", 1, Scale::Height, 0.0, false, true),
        field!("ADBE Corner Pin-0003", "lowerLeftX", 0, Scale::Width, 0.0, false, true),
        field!("ADBE Corner Pin-0003", "lowerLeftY", 1, Scale::Height, 0.0, false, true),
        field!("ADBE Corner Pin-0004", "lowerRightX", 0, Scale::Width, 0.0, false, true),
        field!("ADBE Corner Pin-0004", "lowerRightY", 1, Scale::Height, 0.0, false, true)),
    mapping!("ADBE Tile" => "motionTile", "Horizontal Phase Shift is not represented.";
        field!("ADBE Tile-0001", "tileCenterX", 0, Scale::Width, 0.0, false, true),
        field!("ADBE Tile-0001", "tileCenterY", 1, Scale::Height, 0.0, false, true),
        field!("ADBE Tile-0002", "tileWidth"), field!("ADBE Tile-0003", "tileHeight"),
        field!("ADBE Tile-0004", "outputWidth"), field!("ADBE Tile-0005", "outputHeight"),
        field!("ADBE Tile-0006", "mirrorEdges", 0, Scale::Identity, 0.0, true, true),
        field!("ADBE Tile-0007", "phase")),
    mapping!("ADBE Posterize" => "posterize", "Posterize color levels.";
        field!("ADBE Posterize-0001", "levels")),
    mapping!("ADBE Posterize Time" => "posterizeTime", "Whole-layer clock warp differs from AE stack-local time effect; frameRate cannot animate.";
        field!("ADBE Posterize Time-0001", "frameRate", 0, Scale::Identity, 0.0, false, false)),
    mapping!("CS Vignette" => "vignette", "CC Vignette is approximated by a centered radial falloff: Amount maps as percent and Angle of View maps linearly to radius. FX feather has no native control; native Center and Pin Highlights use centered/zero replacements; the kernels are not equivalent.";
        field!("CS Vignette-0001", "amount", 0, Scale::Factor(0.01), 0.0, false, true),
        field!("CS Vignette-0002", "radius", 0, Scale::Factor(1.0 / 60.0), 0.0, false, true)),
    mapping!("ADBE Find Edges" => "findEdges", "Blend With Original omitted.";
        field!("ADBE Find Edges-0001", "invert")),
    mapping!("ADBE Exposure2" => "exposure", "Master RGB only; individual-channel and linear-light bypass controls omitted.";
        field!("ADBE Exposure2-0003", "exposure"), field!("ADBE Exposure2-0004", "offset"),
        field!("ADBE Exposure2-0005", "gammaCorrection")),
    mapping!("ADBE Vibrance" => "vibrance", "Engine preset approximates Adobe color math.";
        field!("ADBE Vibrance-0001", "vibrance"), field!("ADBE Vibrance-0002", "saturation")),
    mapping!("ADBE Twirl" => "twirl", "Legacy AE Twirl geometry is approximated by engine shader.";
        field!("ADBE Twirl-0001", "angle"), field!("ADBE Twirl-0002", "radius", 0, Scale::Factor(0.01), 0.0, false, true),
        field!("ADBE Twirl-0003", "centerX", 0, Scale::Width, 0.0, false, true),
        field!("ADBE Twirl-0003", "centerY", 1, Scale::Height, 0.0, false, true)),
    mapping!("ADBE Ripple" => "ripple", "Source pixel amplitude/center/wavelength are mapped to the composition-sized destination Group UV plane. Wavelength is reciprocal and static only; keyed wavelength needs nonlinear animation. After that ordinary lowering, the retained amplitude and every emitted amplitude key are scaled by one factor when the largest exceeds the visual strength limit (amplitude * frequency <= 1.25, beyond the FX ring fold-over threshold of 1, so rings can still fold), approximating the omitted Radius confinement. Speed/radius/conversion mode are omitted.";
        field!("ADBE Ripple-0006", "amplitude", 0, Scale::Width, 0.0, false, true),
        field!("ADBE Ripple-0007", "phase", 0, Scale::Factor(std::f64::consts::PI / 180.0), 0.0, false, true),
        field!("ADBE Ripple-0002", "centerX", 0, Scale::Width, 0.0, false, true),
        field!("ADBE Ripple-0002", "centerY", 1, Scale::Height, 0.0, false, true)),
    mapping!("ADBE Sharpen" => "sharpen", "Engine preset approximates Adobe sharpening.";
        field!("ADBE Sharpen-0001", "amount")),
    mapping!("ADBE Luma Key" => "lumaKey", "Only Luma key type; threshold/tolerance approximate engine threshold/softness. Edge Thin/Feather omitted.";
        field!("ADBE Luma Key-0002", "threshold", 0, Scale::Factor(1.0 / 255.0), 0.0, false, true),
        field!("ADBE Luma Key-0003", "softness", 0, Scale::Factor(0.01), 0.0, false, true)),
    mapping!("ADBE Simple Choker" => "simpleChoker", "Matte view mode omitted.";
        field!("ADBE Simple Choker-0002", "choke")),
    mapping!("VISINF Grain Implant" => "grain", "Only core Add Grain controls; unrepresented channel, application, matching, masking and animation sub-settings use the plugin's untouched raw native state. Preview mode retains its owner-relative default region and guide box.";
        Field { param: "intensity", ..field!("VISINF Grain Implant-0008", "amount") }, field!("VISINF Grain Implant-0007", "size"),
        field!("VISINF Grain Implant-0130", "softness"), field!("VISINF Grain Implant-0030", "aspectRatio"),
        field!("VISINF Grain Implant-0013", "seed")),
    mapping!("ADBE Wave Warp" => "waveWarp", "Source pixel height/wavelength are mapped to the composition-sized destination Group UV plane. Wave Type, Speed, Pinning and Antialiasing are omitted. Wavelength is reciprocal and static only; phase degrees to radians is affine and animatable. Engine approximates warp.";
        field!("ADBE Wave Warp-0002", "waveHeight", 0, Scale::Height, 0.0, false, true),
        field!("ADBE Wave Warp-0004", "direction"),
        field!("ADBE Wave Warp-0007", "phase", 0, Scale::Factor(std::f64::consts::PI / 180.0), 0.0, false, true)),
    mapping!("ADBE Tint" => "tintTritone", "AE map-color RGB and amount; alpha and tritone midtone behavior differ.";
        field!("ADBE Tint-0001", "blackR", 0, Scale::Identity, 0.0, false, true),
        field!("ADBE Tint-0001", "blackG", 1, Scale::Identity, 0.0, false, true),
        field!("ADBE Tint-0001", "blackB", 2, Scale::Identity, 0.0, false, true),
        field!("ADBE Tint-0002", "whiteR", 0, Scale::Identity, 0.0, false, true),
        field!("ADBE Tint-0002", "whiteG", 1, Scale::Identity, 0.0, false, true),
        field!("ADBE Tint-0002", "whiteB", 2, Scale::Identity, 0.0, false, true),
        field!("ADBE Tint-0003", "amount")),
    mapping!("ADBE Ramp" => "gradientRamp", "AE shape popup 1=linear/2=radial mapped to shader 0/1; scatter/color alpha omitted; normalized 0..1 Blend With Original inverted to full-ramp mix.";
        field!("ADBE Ramp-0001", "startX", 0, Scale::Width, 0.0, false, true),
        field!("ADBE Ramp-0001", "startY", 1, Scale::Height, 0.0, false, true),
        field!("ADBE Ramp-0003", "endX", 0, Scale::Width, 0.0, false, true),
        field!("ADBE Ramp-0003", "endY", 1, Scale::Height, 0.0, false, true),
        field!("ADBE Ramp-0002", "startR", 0, Scale::Identity, 0.0, false, true),
        field!("ADBE Ramp-0002", "startG", 1, Scale::Identity, 0.0, false, true),
        field!("ADBE Ramp-0002", "startB", 2, Scale::Identity, 0.0, false, true),
        field!("ADBE Ramp-0004", "endR", 0, Scale::Identity, 0.0, false, true),
        field!("ADBE Ramp-0004", "endG", 1, Scale::Identity, 0.0, false, true),
        field!("ADBE Ramp-0004", "endB", 2, Scale::Identity, 0.0, false, true),
        field!("ADBE Ramp-0007", "blend", 0, Scale::Factor(-1.0), 1.0, false, true),
        field!("ADBE Ramp-0005", "shape", 0, Scale::Identity, -1.0, false, true)),
    mapping!("ADBE AIF Perlin Noise 3D" => "turbulentNoise", "Fractal/Noise Type are static enum popups requiring whole-effect replacement. Offset Turbulence uses width-percent units and a centered origin; overflow/nonuniform scale omitted.";
        field!("ADBE AIF Perlin Noise 3D-0003", "invert"),
        field!("ADBE AIF Perlin Noise 3D-0004", "contrast"), field!("ADBE AIF Perlin Noise 3D-0005", "brightness"),
        field!("ADBE AIF Perlin Noise 3D-0008", "rotation"), field!("ADBE AIF Perlin Noise 3D-0010", "scale"),
        field!("ADBE AIF Perlin Noise 3D-0015", "complexity"), field!("ADBE AIF Perlin Noise 3D-0017", "subInfluence"),
        field!("ADBE AIF Perlin Noise 3D-0020", "evolution"),
        field!("ADBE AIF Perlin Noise 3D-0025", "blend", 0, Scale::Factor(0.01), 0.0, false, false),
        field!("ADBE AIF Perlin Noise 3D-0013", "offsetX", 0, Scale::WidthPercent, 0.0, false, true),
        field!("ADBE AIF Perlin Noise 3D-0013", "offsetY", 1, Scale::WidthPercent, 0.0, false, true)),
];

// Fill is intentionally an import-only alias. Exporting tintTritone continues
// to author ADBE Tint rather than silently changing the native effect identity.
static FILL_IMPORT_MAPPING: Mapping = mapping!("ADBE Fill" => "tintTritone", "AE Fill is approximated with equal black/white Tint colors and percentage amount. Fill Mask selection, invert, horizontal/vertical feather, full-alpha behavior and effect-kernel edge semantics are not represented; source alpha is preserved by Tint rather than claiming exact Fill alpha behavior.";
    field!("ADBE Fill-0002", "blackR", 0, Scale::Identity, 0.0, false, true),
    field!("ADBE Fill-0002", "blackG", 1, Scale::Identity, 0.0, false, true),
    field!("ADBE Fill-0002", "blackB", 2, Scale::Identity, 0.0, false, true),
    field!("ADBE Fill-0002", "whiteR", 0, Scale::Identity, 0.0, false, true),
    field!("ADBE Fill-0002", "whiteG", 1, Scale::Identity, 0.0, false, true),
    field!("ADBE Fill-0002", "whiteB", 2, Scale::Identity, 0.0, false, true),
    field!("ADBE Fill-0005", "amount", 0, Scale::Factor(100.0), 0.0, false, true));

// Import-only: exported Levels continue to use the established native Levels writer.
const INVERT_IMPORT_MAPPING: Mapping = mapping!("ADBE Invert" => "levels", "Static RGB inversion; alpha is preserved. Other channels and animated/expression controls are omitted.";
    field!("ADBE Invert-0002", "outputBlack", 0, Scale::Factor(-2.55), 255.0, false, false),
    field!("ADBE Invert-0002", "outputWhite", 0, Scale::Factor(2.55), 0.0, false, false));

#[cfg(test)]
pub(crate) fn mappings() -> &'static [Mapping] {
    MAPPINGS
}
pub(crate) fn by_native(native: &str) -> Option<&'static Mapping> {
    if native == INVERT_IMPORT_MAPPING.native {
        return Some(&INVERT_IMPORT_MAPPING);
    }
    if native == FILL_IMPORT_MAPPING.native {
        return Some(&FILL_IMPORT_MAPPING);
    }
    MAPPINGS.iter().find(|mapping| mapping.native == native)
}
pub(crate) fn by_fx(fx_type: &str) -> Option<&'static Mapping> {
    // Easy Levels' non-animatable master controls ignored ordinary property
    // overrides in Adobe readback. Individual Controls has
    // the same mapped RGB master controls with native editable numeric keys.
    if fx_type == "levels" {
        return by_native("ADBE Pro Levels2");
    }
    MAPPINGS.iter().find(|mapping| mapping.fx_type == fx_type)
}

/// Fully shaped payload for each mapped FX variant. Optional engine fields are
/// explicit: exporting a missing field must use the shader's default, not AE's.
pub(crate) fn default_effect(fx_type: &str) -> Value {
    match fx_type {
        "gaussianBlur" => json!({"type":fx_type,"blurriness":0,"repeatEdgePixels":false}),
        "glow" => json!({"type":fx_type,"glowThreshold":60,"glowRadius":10,"glowIntensity":1}),
        "directionalBlur" => json!({"type":fx_type,"direction":0,"blurLength":0}),
        "pixelMotionBlur" => {
            json!({"type":fx_type,"shutterControl":"manual","shutterAngle":180,"shutterSamples":16,"vectorDetail":20})
        }
        "mosaic" => {
            json!({"type":fx_type,"horizontalBlocks":10,"verticalBlocks":10,"sharpColors":false})
        }
        "brightnessContrast" => json!({"type":fx_type,"brightness":0,"contrast":0}),
        "shiftChannels" => {
            json!({"type":fx_type,"takeRedFrom":"red","takeGreenFrom":"green","takeBlueFrom":"blue"})
        }
        "dropShadow" => {
            json!({"type":fx_type,"enabled":true,"color":[0,0,0,1],"offset":[0,0],"blurRadius":0,"spreadRadius":0,"blendMode":"normal"})
        }
        "hueSaturation" => {
            json!({"type":fx_type,"hue":0,"saturation":0,"lightness":0,"colorize":false,"colorizeHue":0,"colorizeSaturation":0,"colorizeLightness":0})
        }
        "radialBlur" => json!({"type":fx_type,"centerX":0.5,"centerY":0.5,"amount":0}),
        "levels" => {
            json!({"type":fx_type,"inputBlack":0,"inputWhite":255,"gamma":1,"outputBlack":0,"outputWhite":255})
        }
        "bulge" => {
            json!({"type":fx_type,"centerX":0.5,"centerY":0.5,"horizontalRadius":0.5,"verticalRadius":0.5,"bulgeHeight":0,"pinning":false})
        }
        "cornerPin" => {
            json!({"type":fx_type,"upperLeftX":0,"upperLeftY":0,"upperRightX":1,"upperRightY":0,"lowerLeftX":0,"lowerLeftY":1,"lowerRightX":1,"lowerRightY":1})
        }
        "motionTile" => {
            json!({"type":fx_type,"tileCenterX":0.5,"tileCenterY":0.5,"tileWidth":100,"tileHeight":100,"outputWidth":100,"outputHeight":100,"phase":0,"mirrorEdges":false})
        }
        "posterize" => json!({"type":fx_type,"levels":6}),
        "posterizeTime" => json!({"type":fx_type,"frameRate":8}),
        "vignette" => json!({"type":fx_type,"amount":0.5,"radius":0.75,"feather":0.35}),
        "findEdges" => json!({"type":fx_type,"invert":1}),
        "exposure" => json!({"type":fx_type,"exposure":0,"offset":0,"gammaCorrection":1}),
        "vibrance" => json!({"type":fx_type,"vibrance":25,"saturation":0}),
        "twirl" => json!({"type":fx_type,"angle":120,"radius":0.5,"centerX":0.5,"centerY":0.5}),
        "ripple" => {
            json!({"type":fx_type,"amplitude":0.03,"frequency":30,"phase":0,"centerX":0.5,"centerY":0.5})
        }
        "sharpen" => json!({"type":fx_type,"amount":40}),
        "lumaKey" => json!({"type":fx_type,"threshold":0.3,"softness":0.1,"invert":0}),
        "simpleChoker" => json!({"type":fx_type,"choke":1}),
        "grain" => {
            json!({"type":fx_type,"amount":1,"size":1,"softness":1,"aspectRatio":1,"seed":0})
        }
        "waveWarp" => {
            json!({"type":fx_type,"waveHeight":0.03,"waveWidth":6,"direction":90,"phase":0})
        }
        "tintTritone" => {
            json!({"type":fx_type,"blackR":0,"blackG":0,"blackB":0,"whiteR":1,"whiteG":1,"whiteB":1,"amount":100})
        }
        "gradientRamp" => {
            json!({"type":fx_type,"startX":0,"startY":0,"endX":1,"endY":1,"startR":0,"startG":0,"startB":0,"endR":1,"endG":1,"endB":1,"blend":1,"shape":0})
        }
        "turbulentNoise" => {
            json!({"type":fx_type,"brightness":0,"contrast":100,"scale":100,"complexity":6,"subInfluence":70,"evolution":0,"rotation":0,"offsetX":0,"offsetY":0,"invert":0,"blend":1,"noiseType":"softLinear","fractalType":"basic"})
        }
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_schema::LayerEffect;

    #[test]
    fn mapped_payloads_are_valid_and_fields_are_present() {
        for mapping in mappings() {
            let payload = default_effect(mapping.fx_type);
            assert!(
                serde_json::from_value::<LayerEffect>(payload.clone()).is_ok(),
                "{}",
                mapping.fx_type
            );
            for field in mapping.fields {
                assert!(
                    payload.get(field.field).is_some(),
                    "{} / {}",
                    mapping.fx_type,
                    field.field
                );
            }
        }
    }

    #[test]
    fn fill_is_an_import_only_tint_alias_with_color_and_opacity_units() {
        let import = by_native("ADBE Fill").expect("native Fill import mapping");
        assert_eq!(import.fx_type, "tintTritone");
        assert_eq!(
            import
                .fields
                .iter()
                .filter(|field| field.native == "ADBE Fill-0002")
                .count(),
            6,
            "Fill Color must drive equal black and white RGB"
        );
        let opacity = import
            .fields
            .iter()
            .find(|field| field.native == "ADBE Fill-0005")
            .expect("Fill Opacity mapping");
        assert_eq!(opacity.field, "amount");
        assert_eq!(opacity.scale.factor([320.0, 180.0]), Some(100.0));
        assert_eq!(
            by_fx("tintTritone").expect("Tint export mapping").native,
            "ADBE Tint",
            "Fill alias must not change editable FX export identity"
        );
    }

    #[test]
    fn vignette_mapping_is_bidirectional() {
        let import = by_native("CS Vignette").expect("native CC Vignette import mapping");
        let export = by_fx("vignette").expect("editable FX Vignette export mapping");
        assert_eq!(import.native, export.native);
        assert_eq!(import.fx_type, export.fx_type);
        assert!(serde_json::from_value::<LayerEffect>(default_effect("vignette")).is_ok());
    }

    #[test]
    fn corrected_scalar_units_and_centered_noise_offsets() {
        let field = |kind, param| {
            by_fx(kind)
                .unwrap()
                .fields
                .iter()
                .find(|field| field.param == param)
                .unwrap()
        };
        assert!(!field("findEdges", "invert").boolean);
        assert!(!field("turbulentNoise", "invert").boolean);
        assert_eq!(field("grain", "intensity").field, "amount");
        assert_eq!(
            field("twirl", "radius").scale.factor([200.0, 100.0]),
            Some(0.01)
        );
        assert_eq!(
            field("lumaKey", "threshold").scale.factor([200.0, 100.0]),
            Some(1.0 / 255.0)
        );
        assert_eq!(
            field("gradientRamp", "blend").scale.factor([200.0, 100.0]),
            Some(-1.0)
        );
        assert_eq!(
            field("turbulentNoise", "offsetX").offset([200.0, 100.0]),
            Some(-50.0)
        );
        assert_eq!(
            field("turbulentNoise", "offsetY").offset([200.0, 100.0]),
            Some(-25.0)
        );
        assert_eq!(
            field("turbulentNoise", "offsetY")
                .scale
                .factor([200.0, 100.0]),
            Some(0.5)
        );
        assert_eq!(
            field("waveWarp", "phase").scale.factor([200.0, 100.0]),
            Some(std::f64::consts::PI / 180.0)
        );
    }
}
