use crate::{
    format::{inspect_project_with_omissions, writer::project_xml, FrameRate},
    schema::{
        records::MediaPathField, MediaId, PrBrightnessContrast, PrColour, PrColourKeyframe,
        PrCornerPin, PrEffect, PrEffectParamAnimation, PrEffectParamKeys, PrEffectParams,
        PrFilmImpactBlur, PrFilmImpactDirectionalBlur, PrGaussianBlur, PrInvert, PrKeyframeEasing,
        PrLevels, PrMedia, PrMosaic, PrPointKeyframe, PrPosterize, PrProjectFile, PrRamp,
        PrReplicate, PrScalarKeyframe, PrSequence, PrTint, PrTransform, PrVideoOccurrence,
        PrVideoStream, PrVideoTrack, BRIGHTNESS_CONTRAST_BRIGHTNESS, BRIGHTNESS_CONTRAST_CONTRAST,
        CORNER_PIN, FILM_IMPACT_BLUR_AMOUNT, GAUSSIAN_BLUR_BLURRINESS, INVERT_BLEND, LEVELS,
        MOSAIC_HORIZONTAL_BLOCKS, MOSAIC_VERTICAL_BLOCKS, POSTERIZE_LEVEL, RAMP_BLEND, RAMP_END,
        RAMP_START_COLOR, REPLICATE_COUNT, TICKS, TINT_AMOUNT, TINT_MAP_BLACK_TO,
        TINT_MAP_WHITE_TO, TRANSFORM_OPACITY, TRANSFORM_POSITION, TRANSFORM_ROTATION,
        TRANSFORM_SCALE_HEIGHT, TRANSFORM_SHUTTER_ANGLE,
    },
    tests::support::{directional_blur, keyed_directional, transform_effect, DEFAULT_PR_TRANSFORM},
    Omission, OmissionKind, OmissionScope,
};
use base64::{engine::general_purpose::STANDARD, Engine};

pub(super) const SOURCE: &str = include_str!("../../../tests/fixtures/one-clip.xml");
const DEFAULT_CHAIN: &str = "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>";
/// Chain flags of a clip whose Motion and Opacity are both default.
pub(super) const DEFAULT_FLAGS: &str = "<DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><DefaultMotionComponentID>1</DefaultMotionComponentID><DefaultOpacityComponentID>2</DefaultOpacityComponentID>";
pub(super) const ACTIVE: &str = "<Bypass>false</Bypass>";
const BYPASSED: &str = "<Bypass>true</Bypass>";
const STATIC_BLUR: &str = "<StartKeyframe>-91445760000000000,25.,0,0,0,0,0,0</StartKeyframe>";
const BOTH_AXES: &str = "<StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe>";

/// An `AE.ADBE Gaussian Blur 2` component and its three parameter records,
/// with a static Blurriness of 25. Records use ObjectIDs `id..id + 3`.
///
/// The component and the static Blur Dimensions and Repeat Edge Pixels
/// records copy `transition_countdown` (Premiere 14.4). The static Blurriness
/// record is inferred from those two, because every corpus Blurriness is
/// keyframed.
pub(super) fn blur(id: u32) -> String {
    let (blurriness, dimensions, repeat_edge) = (id + 1, id + 2, id + 3);
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"8\"><Component Version=\"6\"><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"{blurriness}\"/><Param Index=\"1\" ObjectRef=\"{dimensions}\"/><Param Index=\"2\" ObjectRef=\"{repeat_edge}\"/></Params><ID>4</ID><DisplayName>Gaussian Blur</DisplayName>{ACTIVE}<Intrinsic>false</Intrinsic><ArchivedType>0</ArchivedType></Component><MatchName>AE.ADBE Gaussian Blur 2</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>\
<VideoComponentParam ObjectID=\"{blurriness}\" ClassID=\"a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542\" Version=\"9\"><Name>Blurriness</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>8</ParameterControlType>{STATIC_BLUR}<LowerBound>0</LowerBound><UpperBound>30000</UpperBound><ParameterID>1</ParameterID><UpperUIBound>50</UpperUIBound></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{dimensions}\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"9\"><Name>Blur Dimensions</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType>{BOTH_AXES}<LowerBound>0</LowerBound><UpperBound>2</UpperBound><ParameterID>2</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{repeat_edge}\" ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\" Version=\"9\"><Name> </Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>4</ParameterControlType><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound><ParameterID>3</ParameterID></VideoComponentParam>"
    )
}

/// [`blur`] with keyed Blurriness: `fields` replaces the static record's
/// `IsTimeVarying`, `ParameterControlType` and `StartKeyframe`.
fn keyed_blur(id: u32, fields: &str) -> String {
    let static_blur = blur(id);
    let keyed = static_blur.replace(
        &format!("<IsTimeVarying>false</IsTimeVarying><ParameterControlType>8</ParameterControlType>{STATIC_BLUR}"),
        fields,
    );
    assert_ne!(keyed, static_blur);
    keyed
}

/// The keyed Blurriness of `food_lower_third` (Premiere 14.4): a Linear
/// 10 to 0 reveal from the clip In with automatic handles, next to the static
/// value 0 and the cached value 10.
const CORPUS_KEYED_BLURRINESS: &str = "<ParameterControlType>8</ParameterControlType><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><Keyframes>914456685542400,10.,0,0,0,0.16666666666666666,-66.046909906632067,0.16666666666666666;914495145479490,0.,0,0,-66.046909906632067,0.16666666666666666,0,0.16666666666666666;</Keyframes><CurrentValue>10</CurrentValue>";

/// The real Premiere 12.1 `PR.ADBE Black & White` of `mixkit-399`
/// (`VideoFilterComponent:1170`, on a nest): `VideoFilterType` 1 and one
/// unnamed `ArbVideoComponentParam`. It is not Premiere 26.5.1's
/// `AE.ADBE Black & White` and has no mapping.
fn black_white_pr(id: u32) -> String {
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"7\"><Component Version=\"5\"><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"{}\"/></Params><ID>4</ID><DisplayName>Black &amp; White</DisplayName>{ACTIVE}<Intrinsic>false</Intrinsic></Component><MatchName>PR.ADBE Black &amp; White</MatchName><VideoFilterType>1</VideoFilterType></VideoFilterComponent>\
<ArbVideoComponentParam ObjectID=\"{}\" ClassID=\"313e54d4-6903-49ad-b0bf-8262cdd10f4e\" Version=\"2\"><IsTimeVarying>false</IsTimeVarying><ParameterControlType>9</ParameterControlType><StartKeyframePosition>-91445760000000000</StartKeyframePosition><ParameterID>-1</ParameterID></ArbVideoComponentParam>",
        id + 1,
        id + 1
    )
}

/// A real Premiere 12.1 `AE.ADBE Tint` component and its parameters
/// (`abstract_slideshow`): Map Black To (163, 247, 143), Map White To
/// (240, 242, 22), Amount 100, the colours of `premiere_isolated_effect_stack`.
pub(super) fn tint(id: u32) -> String {
    let (black, white, amount) = (id + 1, id + 2, id + 3);
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"7\"><Component Version=\"5\"><Node Version=\"1\"><Properties Version=\"1\"><ECP.Filter.Expanded>false</ECP.Filter.Expanded></Properties></Node><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"{black}\"/><Param Index=\"1\" ObjectRef=\"{white}\"/><Param Index=\"2\" ObjectRef=\"{amount}\"/></Params><ID>4</ID><DisplayName>Tint</DisplayName>{ACTIVE}<Intrinsic>false</Intrinsic></Component><MatchName>AE.ADBE Tint</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>\
<VideoComponentParam ObjectID=\"{black}\" ClassID=\"0fde4e9f-f895-4ba3-b0fe-9a6feafda583\" Version=\"9\"><Name>Map Black To</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>5</ParameterControlType><StartKeyframe>-91445760000000000,18374865704210960128,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>18446744073709551615</UpperBound><ParameterID>1</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{white}\" ClassID=\"0fde4e9f-f895-4ba3-b0fe-9a6feafda583\" Version=\"9\"><Name>Map White To</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>5</ParameterControlType><StartKeyframe>-91445760000000000,18374950366522381824,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>18446744073709551615</UpperBound><ParameterID>2</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{amount}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"9\"><Name>Amount to Tint</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>2</ParameterControlType><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound><ParameterID>3</ParameterID></VideoComponentParam>"
    )
}

/// The real Premiere 14.4 `AE.ADBE Invert` of `horror_title`
/// `VideoClipTrackItem:89` and its parameters, without its binary private data.
pub(super) fn invert(id: u32) -> String {
    let (channel, blend) = (id + 1, id + 2);
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"8\"><Component Version=\"6\"><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"{channel}\"/><Param Index=\"1\" ObjectRef=\"{blend}\"/></Params><ID>4</ID><DisplayName>Invert</DisplayName>{ACTIVE}<Intrinsic>false</Intrinsic><ArchivedType>0</ArchivedType></Component><MatchName>AE.ADBE Invert</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>\
<VideoComponentParam ObjectID=\"{channel}\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"9\"><Name>Channel</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>15</UpperBound><ParameterID>1</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{blend}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"9\"><Name>Blend With Original</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>2</ParameterControlType><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound><ParameterID>2</ParameterID></VideoComponentParam>"
    )
}

/// The real Premiere 14.4 `AE.ADBE Legacy Key Track Matte` that follows
/// [`invert`] on `horror_title` `VideoClipTrackItem:89`, and its parameters:
/// Matte 7 (the track ID of `Index` 5), Matte Alpha, Reverse off.
pub(super) fn track_matte_key(id: u32) -> String {
    super::animation::animation_fixture::track_matte_key_xml(id, 7, 0, false)
}

/// The real Premiere 12.1 `AE.ADBE AECrop` of `abstract_slideshow`
/// (`VideoFilterComponent:2410`) and its parameters, without the mask
/// sub-component that it carries there.
fn crop(id: u32) -> String {
    let [left, top, right, bottom, edge, feather] = [1, 2, 3, 4, 5, 6].map(|offset| id + offset);
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"7\"><Component Version=\"5\"><Node Version=\"1\"><Properties Version=\"1\"><ECP.Filter.Expanded>false</ECP.Filter.Expanded></Properties></Node><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"{left}\"/><Param Index=\"1\" ObjectRef=\"{top}\"/><Param Index=\"2\" ObjectRef=\"{right}\"/><Param Index=\"3\" ObjectRef=\"{bottom}\"/><Param Index=\"4\" ObjectRef=\"{edge}\"/><Param Index=\"5\" ObjectRef=\"{feather}\"/></Params><ID>3</ID><DisplayName>Crop</DisplayName>{ACTIVE}<Intrinsic>false</Intrinsic></Component><MatchName>AE.ADBE AECrop</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>\
<VideoComponentParam ObjectID=\"{left}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"9\"><Name>Left</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>2</ParameterControlType><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound><ParameterID>1</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{top}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"9\"><Name>Top</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>2</ParameterControlType><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><CurrentValue>100</CurrentValue><LowerBound>0</LowerBound><UpperBound>100</UpperBound><ParameterID>2</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{right}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"9\"><Name>Right</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>2</ParameterControlType><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound><ParameterID>3</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{bottom}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"9\"><Name>Bottom</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>2</ParameterControlType><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound><ParameterID>4</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{edge}\" ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\" Version=\"9\"><Name></Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>4</ParameterControlType><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound><ParameterID>5</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{feather}\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"9\"><Name>Edge Feather</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>1</ParameterControlType><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe><LowerBound>-30000</LowerBound><UpperBound>30000</UpperBound><ParameterID>6</ParameterID><LowerUIBound>-100</LowerUIBound><UpperUIBound>100</UpperUIBound></VideoComponentParam>"
    )
}

/// [`crop`] with Top 15 and the blank Zoom checkbox name that the Crop reader
/// requires: an active Crop that converts.
pub(super) fn top_crop(id: u32) -> String {
    crop(id)
        .replace("<Name></Name>", "<Name> </Name>")
        .replace(",100.,", ",15.,")
        .replace(
            "<CurrentValue>100</CurrentValue>",
            "<CurrentValue>15</CurrentValue>",
        )
}

/// The records with ObjectIDs `ids` of a pinned fixture, verbatim.
pub(super) fn fixture_records(fixture: &str, ids: &[&str]) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let source = crate::format::read_xml(&path).unwrap();
    let native = roxmltree::Document::parse(&source).unwrap();
    native
        .root_element()
        .children()
        .filter(|node| {
            node.attribute("ObjectID")
                .is_some_and(|id| ids.contains(&id))
        })
        .map(|node| &source[node.range()])
        .collect()
}

/// The Premiere 26.3 Linear Wipe of `feature_linear_wipe_strict.prproj`
/// (`VideoFilterComponent:154` and its parameters 155-157): Wipe Angle 270,
/// Feather 5 and two Transition Completion keys.
fn adobe_linear_wipe() -> String {
    fixture_records(
        "feature_linear_wipe_strict.prproj",
        &["154", "155", "156", "157"],
    )
}

/// Premiere 26.5.1's `AE.ADBE AECrop` from the native Crop sample
/// (`p0_crop_wipe_probe.prproj`, save `51835d1c…882d554e`; `VideoFilterComponent:107`
/// and its parameters 124-129), verbatim but for ObjectIDs: no `Bypass` or
/// `Intrinsic`, and parameters without `ParameterControlType` (except Edge
/// Feather), `IsTimeVarying` or Zoom bounds. Left 20, Top 15, Right 0, Bottom 10,
/// Edge Feather 0. Records use ObjectIDs `id..id + 7`.
fn crop_26_5(id: u32) -> String {
    let [left, top, right, bottom, zoom, feather] = [1, 2, 3, 4, 5, 6].map(|offset| id + offset);
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"><Component Version=\"7\"><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"{left}\"/><Param Index=\"1\" ObjectRef=\"{top}\"/><Param Index=\"2\" ObjectRef=\"{right}\"/><Param Index=\"3\" ObjectRef=\"{bottom}\"/><Param Index=\"4\" ObjectRef=\"{zoom}\"/><Param Index=\"5\" ObjectRef=\"{feather}\"/></Params><ID>3</ID><DisplayName>Crop</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE AECrop</MatchName></VideoFilterComponent>\
<VideoComponentParam ObjectID=\"{left}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"10\"><Name>Left</Name><ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,20.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{top}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"10\"><Name>Top</Name><ParameterID>2</ParameterID><StartKeyframe>-91445760000000000,15.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{right}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"10\"><Name>Right</Name><ParameterID>3</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{bottom}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"10\"><Name>Bottom</Name><ParameterID>4</ParameterID><StartKeyframe>-91445760000000000,10.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{zoom}\" ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\" Version=\"10\"><Name> </Name><ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{feather}\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"10\"><Name>Edge Feather</Name><LowerUIBound>-100</LowerUIBound><ParameterControlType>1</ParameterControlType><ParameterID>6</ParameterID><UpperUIBound>100</UpperUIBound><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe><LowerBound>-30000</LowerBound><UpperBound>30000</UpperBound></VideoComponentParam>"
    )
}

/// Premiere 26.5.1's "Linear Wipe (Legacy)" from the same probe
/// (`VideoFilterComponent:109` and its parameters 130-132), verbatim but for
/// ObjectIDs: Wipe Angle 90, Feather 0, Transition Completion keyed 0 at 0.5 s
/// to 60 at 1.5 s. Records use ObjectIDs `id..id + 4`.
fn linear_wipe_26_5(id: u32) -> String {
    let [completion, angle, feather] = [1, 2, 3].map(|offset| id + offset);
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"><Component Version=\"7\"><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"{completion}\"/><Param Index=\"1\" ObjectRef=\"{angle}\"/><Param Index=\"2\" ObjectRef=\"{feather}\"/></Params><ID>3</ID><DisplayName>Linear Wipe (Legacy)</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Linear Wipe</MatchName></VideoFilterComponent>\
<VideoComponentParam ObjectID=\"{completion}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"10\"><Name>Transition Completion</Name><IsTimeVarying>true</IsTimeVarying><ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><Keyframes>127008000000,0.,0,0,0,0.16666666666666666,60,0.16666666666666666;381024000000,60.,0,0,60,0.16666666666666666,0,0.16666666666666666;</Keyframes><LowerBound>0</LowerBound><UpperBound>100</UpperBound></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{angle}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"10\"><Name>Wipe Angle</Name><ParameterControlType>3</ParameterControlType><ParameterID>2</ParameterID><StartKeyframe>-91445760000000000,90.,0,0,0,0,0,0</StartKeyframe><LowerBound>-32768</LowerBound><UpperBound>32767</UpperBound></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{feather}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"10\"><Name>Feather</Name><ParameterID>3</ParameterID><UpperUIBound>100</UpperUIBound><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>32000</UpperBound></VideoComponentParam>"
    )
}

/// Premiere 26.5.1's "Gaussian Blur (Legacy)" that the same probe's third clip
/// holds at `Index` 1 after its Crop (`VideoFilterComponent:112` and its
/// parameters 139-141), verbatim but for ObjectIDs: Blurriness 40, Repeat Edge
/// Pixels on. Records use ObjectIDs `id..id + 4`.
fn blur_26_5(id: u32) -> String {
    let [blurriness, dimensions, repeat_edge] = [1, 2, 3].map(|offset| id + offset);
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"><Component Version=\"7\"><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"{blurriness}\"/><Param Index=\"1\" ObjectRef=\"{dimensions}\"/><Param Index=\"2\" ObjectRef=\"{repeat_edge}\"/></Params><ID>3</ID><DisplayName>Gaussian Blur (Legacy)</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Gaussian Blur 2</MatchName></VideoFilterComponent>\
<VideoComponentParam ObjectID=\"{blurriness}\" ClassID=\"a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542\" Version=\"10\"><Name>Blurriness</Name><ParameterID>1</ParameterID><UpperUIBound>50</UpperUIBound><StartKeyframe>-91445760000000000,40.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>30000</UpperBound></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{dimensions}\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"10\"><Name>Blur Dimensions</Name><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterID>2</ParameterID><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>2</UpperBound></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{repeat_edge}\" ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\" Version=\"10\"><Name> </Name><ParameterID>3</ParameterID><StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe></VideoComponentParam>"
    )
}

/// The Motion of clip S1 of `feature_motion_opacity_26_5_strict.prproj` as
/// Premiere 26.5.1 saved it (`VideoFilterComponent:199` and its parameters
/// 238-248: Position 0.625:0.65, Motion Crop 0), with Motion Crop Left `left`:
/// a derived edit of the save, not an Adobe save.
pub(super) fn motion_26_5(left: &str) -> String {
    let motion = fixture_records(
        "feature_motion_opacity_26_5_strict.prproj",
        &[
            "199", "238", "239", "240", "241", "242", "243", "244", "245", "246", "247", "248",
        ],
    );
    let (head, crop_left) = motion.split_at(motion.find("<Name>Crop Left</Name>").unwrap());
    head.to_owned()
        + &crop_left.replacen(
            "<StartKeyframe>-91445760000000000,0.,",
            &format!("<StartKeyframe>-91445760000000000,{left},"),
            1,
        )
}

/// The chain flags that Premiere 26.5.1 saves with clip S1's explicit Motion.
const EXPLICIT_MOTION_FLAGS: &str =
    "<DefaultOpacity>true</DefaultOpacity><DefaultOpacityComponentID>2</DefaultOpacityComponentID>";

/// An `AE.ADBE AEMask` record as `vhs_slideshow` attaches one, without its
/// parameters, UI node and private data.
fn mask(id: u32) -> String {
    format!("<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"7\"><Component Version=\"5\"><ID>0</ID><DisplayName>Mask</DisplayName><InstanceName>1</InstanceName>{ACTIVE}<Intrinsic>false</Intrinsic></Component><MatchName>AE.ADBE AEMask</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>")
}

/// [`tint`] with one [`mask`], as `vhs_slideshow` attaches it: a
/// `SubComponents` reference. Records use ObjectIDs `id..id + 4`.
pub(super) fn masked_tint(id: u32) -> String {
    let mask_id = id + 4;
    tint(id).replace(
        "</Component><MatchName>AE.ADBE Tint</MatchName>",
        &format!("</Component><SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"{mask_id}\"/></SubComponents><MatchName>AE.ADBE Tint</MatchName>"),
    ) + &mask(mask_id)
}

/// A real Premiere 14.4 wipe clip effect with its parameters elided, which the
/// coverage rule does not read: the `AE.ADBE Linear Wipe` of `type_title`
/// (`VideoFilterComponent:106`, component ID 5) or the `AE.ADBE Radial Wipe`
/// of `transition_countdown` (`VideoFilterComponent:156`, component ID 3).
fn wipe(id: u32, display_name: &str) -> String {
    format!("<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"8\"><Component Version=\"6\"><ID>3</ID><DisplayName>{display_name}</DisplayName>{ACTIVE}<Intrinsic>false</Intrinsic><ArchivedType>0</ArchivedType></Component><MatchName>AE.ADBE {display_name}</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>")
}

/// An intrinsic component (`Intrinsic` true) with its parameters elided: the
/// chain checks reject these shapes before any parameter is read.
pub(super) fn intrinsic(
    id: u32,
    component_id: u32,
    display_name: &str,
    match_name: &str,
) -> String {
    format!("<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"8\"><Component Version=\"6\"><ID>{component_id}</ID><DisplayName>{display_name}</DisplayName>{ACTIVE}<Intrinsic>true</Intrinsic><ArchivedType>0</ArchivedType></Component><MatchName>{match_name}</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>")
}

/// `source` whose first occurrence chain (`VideoComponentChain:4`) has `flags`
/// and holds `components` in chain order, each at the `Index` of its position.
/// Each entry is the ObjectID of its component record and all of its records.
pub(super) fn with_chain(source: &str, flags: &str, components: &[(u32, String)]) -> String {
    let references: String = components
        .iter()
        .enumerate()
        .map(|(index, (id, _))| format!("<Component Index=\"{index}\" ObjectRef=\"{id}\"/>"))
        .collect();
    let chain = format!("<VideoComponentChain ObjectID=\"4\">{flags}<ComponentChain><Components>{references}</Components></ComponentChain></VideoComponentChain>");
    let xml = source.replace(DEFAULT_CHAIN, &chain);
    assert_ne!(xml, source);
    let records: String = components
        .iter()
        .map(|(_, records)| records.as_str())
        .collect();
    xml.replace("</PremiereData>", &format!("{records}</PremiereData>"))
}

/// `one-clip.xml` whose occurrence chain holds `effects` in chain order (the
/// first at `Index` 0), with default Motion and Opacity.
fn with_effects(effects: &[(u32, String)]) -> String {
    with_chain(SOURCE, DEFAULT_FLAGS, effects)
}

/// `one-clip.xml` with a second occurrence (`VideoClipTrackItem:9`) of the same
/// source at 5 to 10 s and a default chain, so that the sequence stays valid
/// when the first occurrence is omitted.
pub(super) fn with_second_clip(source: &str) -> String {
    source
        .replace(
            "<TrackItem ObjectRef=\"3\"/>",
            "<TrackItem ObjectRef=\"3\"/><TrackItem ObjectRef=\"9\"/>",
        )
        .replace(
            "</PremiereData>",
            "<VideoClipTrackItem ObjectID=\"9\"><ClipTrackItem><ComponentOwner><Components ObjectRef=\"10\"/></ComponentOwner><TrackItem><Start>1270080000000</Start><End>2540160000000</End></TrackItem><SubClip ObjectRef=\"11\"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio></VideoClipTrackItem>\
<VideoComponentChain ObjectID=\"10\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>\
<SubClip ObjectID=\"11\"><Clip ObjectRef=\"12\"/><Name>Second</Name></SubClip>\
<VideoClip ObjectID=\"12\"><Clip><Source ObjectRef=\"7\"/><InPoint>1270080000000</InPoint><OutPoint>2540160000000</OutPoint></Clip></VideoClip></PremiereData>",
        )
}

/// Reads a two-clip source whose first occurrence (`VideoClipTrackItem:3`) has
/// the chain `flags` and `components`, and returns the reason that omitted that
/// occurrence. The second occurrence has no effects and converts.
pub(super) fn omitted_occurrence_reason(flags: &str, components: &[(u32, String)]) -> String {
    let xml = with_chain(&with_second_clip(SOURCE), flags, components);
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    let kept: Vec<_> = project.sequences[0]
        .video_occurrences()
        .map(|clip| clip.id.as_deref())
        .collect();
    assert_eq!(kept, [Some("VideoClipTrackItem:9")]);
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert_eq!(omissions[0].scope, OmissionScope::Occurrence);
    assert_eq!(omissions[0].record, "3");
    omissions[0].reason.clone()
}

pub(super) fn read(xml: &str) -> (PrVideoOccurrence, Vec<Omission>) {
    let (project, omissions) = inspect_project_with_omissions(xml, Some("sequence-1")).unwrap();
    let occurrence = project.sequences[0].video_tracks[0].clip(0).clone();
    (occurrence, omissions)
}

fn gaussian_blur(enabled: bool, blurriness: f64, repeat_edge_pixels: bool) -> PrEffect {
    PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::GaussianBlur(PrGaussianBlur {
            blurriness,
            repeat_edge_pixels,
        }),
        animations: Vec::new(),
    }
}

fn key(source_ticks: i64, value: f64) -> PrScalarKeyframe {
    PrScalarKeyframe {
        source_ticks,
        value,
        easing: PrKeyframeEasing::Linear,
    }
}

/// `effect` with keyed Blurriness, whose static value is its first key's.
fn with_blurriness_keys(mut effect: PrEffect, keys: Vec<PrScalarKeyframe>) -> PrEffect {
    let PrEffectParams::GaussianBlur(blur) = &mut effect.params else {
        panic!("expected a Gaussian Blur");
    };
    blur.blurriness = keys[0].value;
    effect.animations = vec![PrEffectParamAnimation {
        param: &GAUSSIAN_BLUR_BLURRINESS,
        keys: PrEffectParamKeys::Scalar(keys),
    }];
    effect
}

/// Reads one effect that must be omitted and returns the omission reason.
fn omitted_reason(records: String) -> String {
    let (occurrence, omissions) = read(&with_effects(&[(20, records)]));
    assert!(occurrence.effects.is_empty(), "{:?}", occurrence.effects);
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert_eq!(omissions[0].scope, OmissionScope::Feature);
    assert_eq!(omissions[0].record, "VideoFilterComponent:20");
    omissions[0].reason.clone()
}

/// Static identity corners in native order, without keys.
const IDENTITY_CORNERS: [(&str, &str); 4] = [("0:0", ""), ("1:0", ""), ("0:1", ""), ("1:1", "")];

/// An `AE.ADBE Corner Pin` component and its four corner records in the
/// shape Premiere 26.5.1 saves (`feature_corner_pin_strict`):
/// no `Bypass` or `Intrinsic`, and per corner its static `x:y` and, when keyed,
/// `IsTimeVarying` and `Keyframes`. Records use ObjectIDs `id..id + 4`.
pub(super) fn corner_pin(id: u32, corners: [(&str, &str); 4]) -> String {
    let params: String = (1..=4)
        .map(|index| {
            format!(
                "<Param Index=\"{}\" ObjectRef=\"{}\"/>",
                index - 1,
                id + index
            )
        })
        .collect();
    let mut records = format!("<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"><Component Version=\"7\"><Params Version=\"1\">{params}</Params><ID>3</ID><DisplayName>Corner Pin</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Corner Pin</MatchName></VideoFilterComponent>");
    for ((index, name), (corner, keys)) in (1..)
        .zip(["Upper Left", "Upper Right", "Lower Left", "Lower Right"])
        .zip(corners)
    {
        let (time_varying, keyframes) = if keys.is_empty() {
            (String::new(), String::new())
        } else {
            (
                "<IsTimeVarying>true</IsTimeVarying>".to_owned(),
                format!("<Keyframes>{keys}</Keyframes>"),
            )
        };
        records.push_str(&format!("<PointComponentParam ObjectID=\"{}\" ClassID=\"ca81d347-309b-44d2-acc7-1c572efb973c\" Version=\"4\"><Name>{name}</Name>{time_varying}<ParameterID>{index}</ParameterID><StartKeyframe>-91445760000000000,{corner},0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe>{keyframes}</PointComponentParam>", id + index));
    }
    records
}

/// The Upper Left keys of clip C of `feature_corner_pin_strict` (Premiere
/// 26.5.1): 0.2:0.2 at source 0.5 s, 0:0 at 1.5 s and 0.3:0.2 at 2 s, Linear,
/// then a Hold to 0.1:0.1 at 3 s, with linear spatial interpolation.
const FIXTURE_CORNER_KEYS: &str = "127008000000,0.20000000000000001:0.20000000000000001,0,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;381024000000,0:0,0,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;508032000000,0.29999999999999999:0.20000000000000001,4,0,0,0.16666666666666666,0,0.33333333333333331,0,0,0,0,0,0;762048000000,0.10000000000000001:0.10000000000000001,0,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;";

/// The Corner Pin of the corpus `mixkit-41` "3D Spin Transition"
/// (`VideoFilterComponent:142`, Premiere 12.1) with its parameters: Upper
/// Left 0:0 to -0.692:0 and back, Upper Right 1:0 to 1.696:0 and back, with
/// temporal Bezier keys and automatic spatial tangents, which carry float noise
/// across the straight path (1.4e-17). Records use ObjectIDs `id..id + 4`.
fn corpus_corner_pin(id: u32) -> String {
    let [upper_left, upper_right, lower_left, lower_right] = [1, 2, 3, 4].map(|offset| id + offset);
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"7\"><Component Version=\"5\"><Node Version=\"1\"><Properties Version=\"1\"><ECP.Filter.Expanded>false</ECP.Filter.Expanded></Properties></Node><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"{upper_left}\"/><Param Index=\"1\" ObjectRef=\"{upper_right}\"/><Param Index=\"2\" ObjectRef=\"{lower_left}\"/><Param Index=\"3\" ObjectRef=\"{lower_right}\"/></Params><ID>3</ID><DisplayName>Corner Pin</DisplayName>{ACTIVE}<Intrinsic>false</Intrinsic></Component><MatchName>AE.ADBE Corner Pin</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>\
<PointComponentParam ObjectID=\"{upper_left}\" ClassID=\"ca81d347-309b-44d2-acc7-1c572efb973c\" Version=\"3\"><Name>Upper Left</Name><ParameterControlType>6</ParameterControlType><StartKeyframe>-91445760000000000,-0.24114583432674408:0,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe><Keyframes>0,0:0,5,0,0,0.16666666666666666,0,0.83484848462301586,5,4,0,0,-0.11536458134651184,1.4128086528098017e-17;101606400000,-0.69218748807907104:0,5,0,0,0.14641873277846493,0,0.14545454546170319,5,4,-0,-0,0,0;262483200000,0:0,5,0,0,0.80303030280432275,0,0.16666666666666666,5,4,-0.11536458134651184,-0,0,0;</Keyframes><ParameterID>1</ParameterID></PointComponentParam>\
<PointComponentParam ObjectID=\"{upper_right}\" ClassID=\"ca81d347-309b-44d2-acc7-1c572efb973c\" Version=\"3\"><Name>Upper Right</Name><ParameterControlType>6</ParameterControlType><StartKeyframe>-91445760000000000,1.3526041507720947:0,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe><Keyframes>0,1:0,5,0,0,0.16666666666666666,0,0.81460055075628734,5,4,0,0,0.11597222089767456,0;101606400000,1.6958333253860474:0,5,0,0,0.16666666666666666,0,0.16666666666666666,5,4,-0,-0,0,0;262483200000,1:0,5,0,0,0.73939393920068031,0,0.16666666666666666,5,4,0.11597222089767456,-1.4202500911234692e-17,0,0;</Keyframes><ParameterID>2</ParameterID></PointComponentParam>\
<PointComponentParam ObjectID=\"{lower_left}\" ClassID=\"ca81d347-309b-44d2-acc7-1c572efb973c\" Version=\"3\"><Name>Lower Left</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>6</ParameterControlType><StartKeyframe>-91445760000000000,0:1,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe><ParameterID>3</ParameterID></PointComponentParam>\
<PointComponentParam ObjectID=\"{lower_right}\" ClassID=\"ca81d347-309b-44d2-acc7-1c572efb973c\" Version=\"3\"><Name>Lower Right</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>6</ParameterControlType><StartKeyframe>-91445760000000000,1:1,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe><ParameterID>4</ParameterID></PointComponentParam>"
    )
}

fn corner_pin_effect(
    enabled: bool,
    corners: [[f64; 2]; 4],
    animations: Vec<PrEffectParamAnimation>,
) -> PrEffect {
    PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::CornerPin(PrCornerPin { corners }),
        animations,
    }
}

fn point_key(source_ticks: i64, value: [f64; 2], easing: PrKeyframeEasing) -> PrPointKeyframe {
    PrPointKeyframe {
        source_ticks,
        value,
        easing,
        spatial_in_tangent: None,
        spatial_out_tangent: None,
    }
}

/// The keys of corner `index` in native order (0 is Upper Left).
fn corner_keys(index: usize, keys: Vec<PrPointKeyframe>) -> PrEffectParamAnimation {
    PrEffectParamAnimation {
        param: &CORNER_PIN.params[index],
        keys: PrEffectParamKeys::Point(keys),
    }
}

/// An `AE.ADBE Motion Blur` component and its Direction and Blur Length
/// records in the shape Premiere 26.5.1 saves (`feature_directional_blur_strict`): no `Bypass` or `Intrinsic`, and per
/// parameter its static value and, when keyed, `IsTimeVarying` and
/// `Keyframes`. Records use ObjectIDs `id..id + 2`.
fn directional_blur_xml(id: u32, direction: (&str, &str), blur_length: (&str, &str)) -> String {
    let (direction_param, length_param) = (id + 1, id + 2);
    let keyed = |keys: &str| {
        if keys.is_empty() {
            (String::new(), String::new())
        } else {
            (
                "<IsTimeVarying>true</IsTimeVarying>".to_owned(),
                format!("<Keyframes>{keys}</Keyframes>"),
            )
        }
    };
    let ((direction, direction_keys), (length, length_keys)) = (direction, blur_length);
    let (direction_varying, direction_keys) = keyed(direction_keys);
    let (length_varying, length_keys) = keyed(length_keys);
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"><Component Version=\"7\"><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"{direction_param}\"/><Param Index=\"1\" ObjectRef=\"{length_param}\"/></Params><ID>3</ID><DisplayName>Directional Blur (Legacy)</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Motion Blur</MatchName></VideoFilterComponent>\
<VideoComponentParam ObjectID=\"{direction_param}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"10\"><Name>Direction</Name>{direction_varying}<ParameterControlType>3</ParameterControlType><ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,{direction},0,0,0,0,0,0</StartKeyframe>{direction_keys}<LowerBound>-32768</LowerBound><UpperBound>32767</UpperBound></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{length_param}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"10\"><Name>Blur Length</Name>{length_varying}<ParameterID>2</ParameterID><UpperUIBound>20</UpperUIBound><StartKeyframe>-91445760000000000,{length},0,0,0,0,0,0</StartKeyframe>{length_keys}<LowerBound>0</LowerBound><UpperBound>1000</UpperBound></VideoComponentParam>"
    )
}

/// An `AE.ADBE Brightness & Contrast 2` component and its Brightness and
/// Contrast records in the shape Premiere 26.5.1 saves (`feature_brightness_contrast_strict`): no `Bypass`, `Intrinsic` or control
/// type, and per parameter its static value and, when keyed, `IsTimeVarying`
/// and `Keyframes`. Records use ObjectIDs `id..id + 2`.
fn brightness_contrast_xml(id: u32, [brightness, contrast]: [(&str, &str); 2]) -> String {
    let params: String = (1..)
        .zip([("Brightness", brightness), ("Contrast", contrast)])
        .map(|(index, (name, (value, keys)))| {
            let (time_varying, keyframes) = if keys.is_empty() {
                (String::new(), String::new())
            } else {
                (
                    "<IsTimeVarying>true</IsTimeVarying>".to_owned(),
                    format!("<Keyframes>{keys}</Keyframes>"),
                )
            };
            format!("<VideoComponentParam ObjectID=\"{}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"10\"><Name>{name}</Name>{time_varying}<ParameterID>{index}</ParameterID><StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0</StartKeyframe>{keyframes}<LowerBound>-100</LowerBound><UpperBound>100</UpperBound></VideoComponentParam>", id + index)
        })
        .collect();
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"><Component Version=\"7\"><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"{}\"/><Param Index=\"1\" ObjectRef=\"{}\"/></Params><ID>3</ID><DisplayName>Brightness &amp; Contrast</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Brightness &amp; Contrast 2</MatchName></VideoFilterComponent>{params}",
        id + 1,
        id + 2
    )
}

/// A Brightness & Contrast with the native `values` and the Brightness and
/// Contrast `keys`, either possibly empty. A keyed static value is its first
/// key's.
fn brightness_contrast(
    enabled: bool,
    [brightness, contrast]: [f64; 2],
    keys: [Vec<PrScalarKeyframe>; 2],
) -> PrEffect {
    let params = [
        &BRIGHTNESS_CONTRAST_BRIGHTNESS,
        &BRIGHTNESS_CONTRAST_CONTRAST,
    ];
    PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::BrightnessContrast(PrBrightnessContrast {
            brightness,
            contrast,
        }),
        animations: params
            .into_iter()
            .zip(keys)
            .filter(|(_, keys)| !keys.is_empty())
            .map(|(param, keys)| PrEffectParamAnimation {
                param,
                keys: PrEffectParamKeys::Scalar(keys),
            })
            .collect(),
    }
}

/// The first bytes of clip A's `PremiereFilterPrivateData` in the run E5
/// fixture (`FF FF FF FF`, then process memory), which import ignores.
const INVERT_PRIVATE_DATA: &str = "<PremiereFilterPrivateData Encoding=\"base64\" BinaryHash=\"08668877-8cea-2512-01d0-ae5d0000241c\">/////2ZhbHNlCWZhbHNl</PremiereFilterPrivateData>";

/// An `AE.ADBE Invert` component and its Channel and Blend With Original
/// records in the shape Premiere 26.5.1 saves (`feature_invert_strict`): no `Bypass`, `Intrinsic` or control type, a
/// `PremiereFilterPrivateData`, the popup `channel` and per Blend its static
/// value and, when keyed, `IsTimeVarying` and `Keyframes`. Records use
/// ObjectIDs `id..id + 2`.
fn invert_26_5_xml(id: u32, channel: &str, (blend, keys): (&str, &str)) -> String {
    let (time_varying, keyframes) = if keys.is_empty() {
        ("", String::new())
    } else {
        (
            "<IsTimeVarying>true</IsTimeVarying>",
            format!("<Keyframes>{keys}</Keyframes>"),
        )
    };
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"><Component Version=\"7\"><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"{}\"/><Param Index=\"1\" ObjectRef=\"{}\"/></Params><ID>3</ID><DisplayName>Invert</DisplayName></Component>{INVERT_PRIVATE_DATA}<VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Invert</MatchName></VideoFilterComponent>\
<VideoComponentParam ObjectID=\"{}\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"10\"><Name>Channel</Name><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,{channel},0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>15</UpperBound></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"10\"><Name>Blend With Original</Name>{time_varying}<ParameterID>2</ParameterID><StartKeyframe>-91445760000000000,{blend},0,0,0,0,0,0</StartKeyframe>{keyframes}<LowerBound>0</LowerBound><UpperBound>100</UpperBound></VideoComponentParam>",
        id + 1,
        id + 2,
        id + 1,
        id + 2
    )
}

/// An Invert of every channel with the native `blend` and its `keys`, possibly
/// empty. A keyed static value is its first key's.
fn invert_effect(enabled: bool, blend: f64, keys: Vec<PrScalarKeyframe>) -> PrEffect {
    PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Invert(PrInvert { blend, channel: 0 }),
        animations: (!keys.is_empty())
            .then_some(PrEffectParamAnimation {
                param: &INVERT_BLEND,
                keys: PrEffectParamKeys::Scalar(keys),
            })
            .into_iter()
            .collect(),
    }
}

#[test]
fn static_blur_stack_keeps_order_values_and_bypass() {
    let second = blur(30)
        .replace(ACTIVE, BYPASSED)
        .replace(",25.,", ",80.,")
        .replace(",false,0,0,0,0,0,0", ",true,0,0,0,0,0,0");
    let (occurrence, omissions) = read(&with_effects(&[(20, blur(20)), (30, second)]));
    assert!(omissions.is_empty(), "{omissions:?}");
    // Premiere applies the chain in descending `Index`:
    // the bypassed blur at Index 1 comes first in the stack.
    assert_eq!(
        occurrence.effects,
        [
            gaussian_blur(false, 80.0, true),
            gaussian_blur(true, 25.0, false)
        ]
    );
}

#[test]
fn blur_without_bypass_or_intrinsic_flags_reads_as_an_active_standard_effect() {
    // Inferred shape: only the derived `premiere_isolated_text_point` fixture
    // (a py-premiere text record on a 26.3 scaffold) omits these
    // flags, with versions 9 and 7. Real 24.3 and 25.5 projects write both.
    let current = blur(20)
        .replace(
            "Version=\"8\"><Component Version=\"6\">",
            "Version=\"9\"><Component Version=\"7\">",
        )
        .replace(
            &format!("{ACTIVE}<Intrinsic>false</Intrinsic><ArchivedType>0</ArchivedType>"),
            "",
        );
    let (occurrence, omissions) = read(&with_effects(&[(20, current)]));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(occurrence.effects, [gaussian_blur(true, 25.0, false)]);
}

#[test]
fn premiere_25_component_with_false_unique_is_read() {
    // The shape of the one `Unique` in the corpus, on a Premiere 25.5
    // component (`practice_files_transcription_magic`): versions 9 and 6,
    // `Bypass` before `DisplayName`, and a trailing `<Unique>false</Unique>`.
    let modern = blur(20)
        .replace(
            "Version=\"8\"><Component Version=\"6\">",
            "Version=\"9\"><Component Version=\"6\">",
        )
        .replace(
            &format!("<DisplayName>Gaussian Blur</DisplayName>{ACTIVE}<Intrinsic>false</Intrinsic><ArchivedType>0</ArchivedType>"),
            &format!("{ACTIVE}<DisplayName>Gaussian Blur</DisplayName><Intrinsic>false</Intrinsic><ArchivedType>0</ArchivedType><Unique>false</Unique>"),
        );
    assert!(modern.contains("<Unique>false</Unique>"), "{modern}");
    let (occurrence, omissions) = read(&with_effects(&[(20, modern.clone())]));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(occurrence.effects, [gaussian_blur(true, 25.0, false)]);
    // The meaning of `Unique` is unknown, so only the observed value converts.
    let reason = omitted_reason(modern.replace("<Unique>false</Unique>", "<Unique>true</Unique>"));
    assert!(
        reason.ends_with("unsupported Unique Some(\"true\")"),
        "{reason}"
    );
}

#[test]
fn unknown_active_effect_is_omitted_with_identity_clip_track_and_time() {
    let (occurrence, omissions) = read(&with_effects(&[(20, black_white_pr(20))]));
    assert!(occurrence.effects.is_empty());
    assert_eq!(occurrence.id.as_deref(), Some("VideoClipTrackItem:3"));
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    let omission = &omissions[0];
    assert_eq!(omission.scope, OmissionScope::Feature);
    assert_eq!(omission.record, "VideoFilterComponent:20");
    for context in [
        "unknown active effect \"Black & White\"",
        "match name \"PR.ADBE Black & White\"",
        "VideoFilterComponent version 7, Component version 5",
        "stack position 1",
        "clip \"Source\" (VideoClipTrackItem:3, V1, 0.000 s to 5.000 s): no Tesseract effect mapping",
    ] {
        assert!(omission.reason.contains(context), "{context}: {omission}");
    }
}

#[test]
fn unknown_bypassed_effect_is_reported_as_bypassed() {
    let reason = omitted_reason(black_white_pr(20).replace(ACTIVE, BYPASSED));
    assert!(
        reason.starts_with("unknown bypassed effect \"Black & White\""),
        "{reason}"
    );
}

#[test]
fn unknown_effect_between_blurs_keeps_the_remaining_stack_order() {
    let later = blur(40).replace(",25.,", ",40.,");
    let (occurrence, omissions) = read(&with_effects(&[
        (20, blur(20)),
        (30, black_white_pr(30)),
        (40, later),
    ]));
    // In descending `Index`, the stack is [blur 40, unknown, blur 25].
    assert_eq!(
        occurrence.effects,
        [
            gaussian_blur(true, 40.0, false),
            gaussian_blur(true, 25.0, false)
        ]
    );
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert!(
        omissions[0].reason.contains("stack position 2"),
        "{omissions:?}"
    );
}

#[test]
fn alpha_glow_native_records_keep_size_keys_crop_and_sibling() {
    let source = include_str!("../../../tests/fixtures/alpha-glow-native.xml");
    let doc = roxmltree::Document::parse(source).unwrap();
    let record = |id: &str| {
        let node = doc
            .root_element()
            .children()
            .find(|node| node.attribute("ObjectID") == Some(id))
            .unwrap();
        &source[node.range()]
    };
    // The saved occurrence owns this chain, not the master clip. Index 1
    // applies before Index 0; the native Crop creates the glow's alpha edge.
    assert!(record("328").contains("<Components ObjectRef=\"392\"/>"));
    let chain = record("392");
    assert!(chain.contains("<Component Index=\"0\" ObjectRef=\"553\"/>"));
    assert!(chain.contains("<Component Index=\"1\" ObjectRef=\"554\"/>"));
    let glow: String = ["553", "759", "760", "761", "762", "763", "764"]
        .map(record)
        .concat();
    let crop: String = ["554", "765", "766", "767", "768", "769", "770"]
        .map(record)
        .concat();
    // Only the surrounding media harness is synthetic; effect records and
    // their relative order are unchanged. The disabled blur is a sibling probe.
    let sibling = blur(20).replace(ACTIVE, BYPASSED);
    let (clip, omissions) = read(&with_effects(&[(20, sibling), (553, glow), (554, crop)]));
    assert_eq!(
        (
            clip.crop.left,
            clip.crop.top,
            clip.crop.right,
            clip.crop.bottom
        ),
        (25.0, 25.0, 0.0, 0.0)
    );
    assert_eq!(clip.effects.len(), 2, "{omissions:?}");
    assert_eq!(clip.effects[1], gaussian_blur(false, 25.0, false));
    assert_eq!(
        clip.effects[0].params,
        PrEffectParams::AlphaGlow {
            size: 30.0,
            brightness: 150.0,
            color: crate::schema::PrColour { rgb: [192; 3] },
        }
    );
    let keys = clip.effects[0].animations[0].keys.scalar().unwrap();
    assert_eq!(
        keys.iter()
            .map(|key| (key.source_ticks, key.value))
            .collect::<Vec<_>>(),
        [(0, 30.0), (723455222095, 100.0)]
    );
    assert_eq!(clip.effects_above_mask, 0);
    assert!(omissions.is_empty(), "{omissions:?}");
}

#[test]
fn lens_distortion_native_keys_and_siblings_are_retained() {
    let lens = fixture_records(
        "lens-distortion-native.xml",
        &["546", "722", "723", "724", "725", "726", "727", "728"],
    );
    for bypassed in [false, true] {
        let native = if bypassed {
            lens.replace("<ID>3</ID>", "<ID>3</ID><Bypass>true</Bypass>")
        } else {
            lens.clone()
        };
        let (occurrence, omissions) = read(&with_effects(&[
            (20, blur(20)),
            (546, native),
            (40, blur(40).replace(",25.,", ",40.,")),
        ]));
        assert!(omissions.is_empty(), "{omissions:?}");
        assert_eq!(occurrence.effects.len(), 3);
        assert_eq!(occurrence.effects[0], gaussian_blur(true, 40.0, false));
        assert_eq!(occurrence.effects[2], gaussian_blur(true, 25.0, false));
        let effect = &occurrence.effects[1];
        assert_eq!(effect.enabled, !bypassed);
        assert_eq!(effect.params, PrEffectParams::LensDistortion(-40.0));
        let keys = effect.animations[0].keys.scalar().unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!((keys[0].source_ticks, keys[0].value), (0, -40.0));
        assert_eq!((keys[1].source_ticks, keys[1].value), (510674274420, 40.0));
        assert!(keys
            .iter()
            .all(|key| key.easing == PrKeyframeEasing::Linear));
        let mut sequence = crate::tests::support::video_sequence();
        sequence.video_tracks[0].clip_mut(0).effects = occurrence.effects.clone();
        let wire = crate::tests::support::project_document(&sequence);
        let mapped = &wire["composition"]["layers"][0]["effects"][1];
        assert_eq!(mapped["enabled"], !bypassed);
        assert_eq!(mapped["effect"]["type"], "lensDistortion");
        assert_eq!(mapped["effect"]["amount"], 0.4);
    }
    let opaque = lens.replace(",true,0,0,0,0,0,0", ",false,0,0,0,0,0,0");
    let (clip, omissions) = read(&with_effects(&[(20, blur(20)), (546, opaque)]));
    assert_eq!(clip.effects, [gaussian_blur(true, 25.0, false)]);
    assert!(omissions[0].reason.contains("Fill Alpha on"));
    let decentered = lens.replace(
        ",0,0,0,0,0,0,0</StartKeyframe>",
        ",10,0,0,0,0,0,0</StartKeyframe>",
    );
    let (_, omissions) = read(&with_effects(&[(546, decentered)]));
    assert!(omissions[0].reason.contains("must be zero"));
}

#[test]
fn corpus_keyed_blurriness_imports_its_keys() {
    let (occurrence, omissions) = read(&with_effects(&[(
        20,
        keyed_blur(20, CORPUS_KEYED_BLURRINESS),
    )]));
    assert!(omissions.is_empty(), "{omissions:?}");
    // The keys keep their source times; the Linear keys drop their automatic
    // handles, as Motion keys do. StartKeyframe 0 and CurrentValue 10 are
    // ignored, and the static value is the first key's.
    assert_eq!(
        occurrence.effects,
        [with_blurriness_keys(
            gaussian_blur(true, 0.0, false),
            vec![key(914456685542400, 10.0), key(914495145479490, 0.0)]
        )]
    );
}

#[test]
fn delayed_first_key_imports_unchanged_whatever_the_start_keyframe() {
    // Clip A of the run D fixture (Premiere 26.5.1): StartKeyframe keeps the
    // earlier static 25, and the first key, 80, is 0.5 s after the clip In.
    // AME renders 80 before that key.
    let fields = "<IsTimeVarying>true</IsTimeVarying><StartKeyframe>-91445760000000000,25.,0,0,0,0,0,0</StartKeyframe><Keyframes>127008000000,80.,0,0,0,0.16666666666666666,-8,0.16666666666666666;381024000000,0.,0,0,-8,0.16666666666666666,0,0.16666666666666666;</Keyframes>";
    let (occurrence, omissions) = read(&with_effects(&[(20, keyed_blur(20, fields))]));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        occurrence.effects,
        [with_blurriness_keys(
            gaussian_blur(true, 0.0, false),
            vec![key(TICKS / 2, 80.0), key(3 * TICKS / 2, 0.0)]
        )]
    );
}

#[test]
fn blurriness_time_varying_flag_must_agree_with_its_keys() {
    let time_varying = blur(20).replacen(
        "<IsTimeVarying>false</IsTimeVarying>",
        "<IsTimeVarying>true</IsTimeVarying>",
        1,
    );
    assert!(omitted_reason(time_varying).ends_with("empty time-varying Blurriness"));
    let disabled = keyed_blur(
        20,
        &format!("<IsTimeVarying>false</IsTimeVarying>{CORPUS_KEYED_BLURRINESS}"),
    );
    assert!(omitted_reason(disabled)
        .ends_with("Blurriness keyframes conflict with disabled IsTimeVarying"));
}

#[test]
fn blurriness_keys_outside_premiere_range_omit_only_that_blur() {
    // Linear keys at source 0 and 1 s. The out-of-range value is on the second
    // key, so the check covers more than the first key's static value.
    let linear_keys = |first: &str, second: &str| {
        format!("<IsTimeVarying>true</IsTimeVarying>{STATIC_BLUR}<Keyframes>0,{first},0,0,0,0.16666666666666666,0,0.16666666666666666;254016000000,{second},0,0,0,0.16666666666666666,0,0.16666666666666666;</Keyframes>")
    };
    // A keyed sibling on the same clip, at the bounds themselves, stays.
    let sibling = keyed_blur(40, &linear_keys("0.", "30000."));
    for value in ["-0.5", "30000.5"] {
        let outside = keyed_blur(20, &linear_keys("10.", value));
        let (occurrence, omissions) = read(&with_effects(&[(20, outside), (40, sibling.clone())]));
        assert_eq!(
            occurrence.effects,
            [with_blurriness_keys(
                gaussian_blur(true, 0.0, false),
                vec![key(0, 0.0), key(TICKS, 30000.0)]
            )]
        );
        assert_eq!(omissions.len(), 1, "{omissions:?}");
        assert_eq!(omissions[0].scope, OmissionScope::Feature);
        assert_eq!(omissions[0].record, "VideoFilterComponent:20");
        assert!(
            omissions[0].reason.ends_with(&format!(
                "Blurriness key value {value} is outside Premiere's 0 to 30000 range"
            )),
            "{omissions:?}"
        );
    }
}

#[test]
fn keyed_blur_dimensions_or_repeat_edge_pixels_omit_the_blur() {
    for (start, keys, label) in [
        (
            BOTH_AXES,
            "0,0,4,0,0,0,0,0;254016000000,1,4,0,0,0,0,0;",
            "Blur Dimensions",
        ),
        (
            "<StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe>",
            "0,false,4,0,0,0,0,0;254016000000,true,4,0,0,0,0,0;",
            "Repeat Edge Pixels",
        ),
    ] {
        let keyed = keyed_blur(20, CORPUS_KEYED_BLURRINESS)
            .replace(start, &format!("{start}<Keyframes>{keys}</Keyframes>"));
        let reason = omitted_reason(keyed);
        assert!(
            reason.ends_with(&format!(
                "keyframed {label} is not supported; only static values convert"
            )),
            "{reason}"
        );
    }
}

#[test]
fn film_impact_blur_without_its_parameters_is_not_guessed() {
    let film_impact = "<VideoFilterComponent ObjectID=\"20\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"><Component Version=\"7\"><ID>3</ID><DisplayName>Gaussian Blur</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.Impact_Blur_FX</MatchName></VideoFilterComponent>";
    let reason = omitted_reason(film_impact.to_owned());
    assert!(
        reason.starts_with("active effect \"Gaussian Blur\" (match name \"AE.Impact_Blur_FX\", VideoFilterComponent version 9, Component version 7)"),
        "{reason}"
    );
    assert!(
        reason.ends_with("expected 22 parameters, found 0"),
        "{reason}"
    );
}

#[test]
fn single_axis_blur_dimensions_are_rejected() {
    for blurriness in [blur(20), keyed_blur(20, CORPUS_KEYED_BLURRINESS)] {
        for value in ["1", "2"] {
            let single_axis = blurriness.replace(
                BOTH_AXES,
                &format!("<StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0</StartKeyframe>"),
            );
            let reason = omitted_reason(single_axis);
            assert!(
                reason.contains(&format!(
                    "Blur Dimensions \"{value}\" is not Horizontal and Vertical"
                )),
                "{reason}"
            );
        }
    }
}

#[test]
fn static_parameter_uses_authored_start_instead_of_cached_current_value() {
    let with_current = |current: &str| {
        blur(20).replace(
            STATIC_BLUR,
            &format!("{STATIC_BLUR}<CurrentValue>{current}</CurrentValue>"),
        )
    };
    // Native static Crop records establish that StartKeyframe is authored
    // state while CurrentValue can retain an unrelated UI readback.
    for current in ["25", "7"] {
        let (occurrence, omissions) = read(&with_effects(&[(20, with_current(current))]));
        assert!(omissions.is_empty(), "{current}: {omissions:?}");
        assert_eq!(
            occurrence.effects,
            [gaussian_blur(true, 25.0, false)],
            "{current}"
        );
    }
}

#[test]
fn unexpected_blur_shapes_are_rejected_with_a_precise_reason() {
    let base = blur(20);
    for (changed, expected) in [
        (
            base.replace("<ArchivedType>0</ArchivedType>", "<InstanceName>Soft</InstanceName>"),
            "Component/InstanceName is not supported",
        ),
        (
            base.replace("</Component><MatchName>", "</Component><SubComponents/><MatchName>"),
            "0 masks on Opacity are not converted",
        ),
        (
            base.replace(
                "<Params Version=\"1\">",
                "<Node Version=\"1\"><Properties Version=\"1\"><BE.VideoComponentChain.ParentPinID>0</BE.VideoComponentChain.ParentPinID></Properties></Node><Params Version=\"1\">",
            ),
            "Component/Node/Properties/BE.VideoComponentChain.ParentPinID is not supported",
        ),
        (
            base.replace("<ArchivedType>0</ArchivedType>", "<ArchivedType>1</ArchivedType>"),
            "unsupported ArchivedType",
        ),
        (
            base.replace("<VideoFilterType>2</VideoFilterType>", "<VideoFilterType>1</VideoFilterType>"),
            "unsupported VideoFilterType",
        ),
        (
            base.replace("<Param Index=\"2\" ObjectRef=\"23\"/>", ""),
            "expected 3 parameters, found 2",
        ),
        (
            base.replace("<ParameterID>3</ParameterID>", "<ParameterID>4</ParameterID>"),
            "unknown ParameterID 4",
        ),
        (
            base.replace("<ParameterID>3</ParameterID>", "<ParameterID>2</ParameterID>"),
            "duplicate ParameterID 2",
        ),
        (
            base.replace(
                "<Name> </Name>",
                "<Name>Blur Dimensions</Name>",
            )
            .replace("<ParameterID>3</ParameterID>", "<ParameterID>2</ParameterID>"),
            "duplicate ParameterID 2",
        ),
        (base.replace(ACTIVE, "<Bypass>maybe</Bypass>"), "invalid Bypass \"maybe\""),
        (base.replace(",25.,", ",-1.,"), "Blurriness \"-1.\" is not a number from 0 to 30000"),
        (
            base.replace(",false,0,0,0,0,0,0", ",yes,0,0,0,0,0,0"),
            "invalid Repeat Edge Pixels value \"yes\"",
        ),
        (
            base.replace(STATIC_BLUR, "<StartKeyframe>-91445760000000000,25.</StartKeyframe>"),
            "unexpected Blurriness StartKeyframe shape",
        ),
        (
            base.replace(
                "<UpperUIBound>50</UpperUIBound>",
                "<UpperUIBound>50</UpperUIBound><Expression>wiggle(2, 5)</Expression>",
            ),
            "parameter Expression is not supported",
        ),
    ] {
        assert_ne!(changed, base, "{expected}");
        let reason = omitted_reason(changed);
        assert!(reason.contains(expected), "{expected}: {reason}");
    }
}

#[test]
fn premiere_26_corner_pins_read_static_and_keyed_corners() {
    use PrKeyframeEasing::{Hold, Linear};
    // Clip A of the run E1 fixture: a static skew.
    let skew = corner_pin(
        20,
        [
            ("0.10000000000000001:0.050000000000000003", ""),
            ("0.94999999999999996:0", ""),
            ("0:1", ""),
            ("0.84999999999999998:0.90000000000000002", ""),
        ],
    );
    // Clip C: Upper Left keyed while its StartKeyframe keeps 0:0.
    let mut keyed = IDENTITY_CORNERS;
    keyed[0].1 = FIXTURE_CORNER_KEYS;
    for (records, expected) in [
        (
            skew,
            corner_pin_effect(
                true,
                [[0.1, 0.05], [0.95, 0.0], [0.0, 1.0], [0.85, 0.9]],
                Vec::new(),
            ),
        ),
        (
            corner_pin(20, keyed),
            // A keyed corner starts at its first key, not at StartKeyframe.
            corner_pin_effect(
                true,
                [[0.2, 0.2], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
                vec![corner_keys(
                    0,
                    vec![
                        point_key(TICKS / 2, [0.2, 0.2], Linear),
                        point_key(3 * TICKS / 2, [0.0, 0.0], Linear),
                        point_key(2 * TICKS, [0.3, 0.2], Linear),
                        point_key(3 * TICKS, [0.1, 0.1], Hold),
                    ],
                )],
            ),
        ),
    ] {
        let (occurrence, omissions) = read(&with_effects(&[(20, records)]));
        assert!(omissions.is_empty(), "{omissions:?}");
        assert_eq!(occurrence.effects, [expected]);
    }
}

#[test]
fn corpus_corner_pin_spin_reads_its_straight_bezier_keys() {
    let (occurrence, omissions) = read(&with_effects(&[(20, corpus_corner_pin(20))]));
    assert!(omissions.is_empty(), "{omissions:?}");
    let [effect] = occurrence.effects.as_slice() else {
        panic!("expected one effect: {:?}", occurrence.effects);
    };
    // The keyed corners start at their first keys, not at the StartKeyframes
    // -0.241:0 and 1.353:0.
    let PrEffectParams::CornerPin(pin) = &effect.params else {
        panic!("expected a Corner Pin: {effect:?}");
    };
    assert_eq!(
        pin.corners,
        [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]
    );
    // Every native speed is zero, so each Bezier keeps its influences.
    let ease = |outgoing: f64, incoming: f64| PrKeyframeEasing::CubicBezier {
        x1: outgoing,
        y1: 0.0,
        x2: 1.0 - incoming,
        y2: 1.0,
    };
    let keys: Vec<_> = effect
        .animations
        .iter()
        .map(|animation| {
            let keys = animation.keys.point().unwrap();
            let keys: Vec<_> = keys
                .iter()
                .map(|key| (key.source_ticks, key.value, key.easing))
                .collect();
            (animation.param.label, keys)
        })
        .collect();
    assert_eq!(
        keys,
        [
            (
                "Upper Left",
                vec![
                    (0, [0.0, 0.0], PrKeyframeEasing::Linear),
                    (
                        101606400000,
                        [-0.692187488079071, 0.0],
                        ease(0.8348484846230159, 0.14641873277846493)
                    ),
                    (
                        262483200000,
                        [0.0, 0.0],
                        ease(0.1454545454617032, 0.8030303028043227)
                    ),
                ]
            ),
            (
                "Upper Right",
                vec![
                    (0, [1.0, 0.0], PrKeyframeEasing::Linear),
                    (
                        101606400000,
                        [1.6958333253860474, 0.0],
                        ease(0.8146005507562873, 0.16666666666666666)
                    ),
                    (
                        262483200000,
                        [1.0, 0.0],
                        ease(0.16666666666666666, 0.7393939392006803)
                    ),
                ]
            ),
        ]
    );
}

#[test]
fn curved_corner_paths_omit_the_corner_pin() {
    // The spin's first outgoing tangent, -0.115:1.4e-17, lies on its path to
    // -0.692:0 within float noise; these tangents do not.
    let noisy = "-0.11536458134651184,1.4128086528098017e-17;";
    for tangent in ["-0.11536458134651184,0.000001;", "-0.8,0;"] {
        let reason = omitted_reason(corpus_corner_pin(20).replacen(noisy, tangent, 1));
        assert!(
            reason.ends_with("Upper Left moves on a curved spatial path between its keys at source times 0.000 s and 0.400 s; only a straight path converts, because FX keys each coordinate separately"),
            "{tangent}: {reason}"
        );
    }
}

#[test]
fn degenerate_or_nonconvex_corner_quads_omit_the_corner_pin() {
    let static_quad = "the static corners form a degenerate or non-convex quad, which no perspective warp of the clip frame produces";
    // Upper Left moves inside the quad, Linear from 0:0 at 0 s to 0.9:0.9 at 1 s.
    let mut inward = IDENTITY_CORNERS;
    inward[0].1 = "0,0:0,0,0,0,0,0,0,0,0,0,0,0,0;254016000000,0.9:0.9,0,0,0,0,0,0,0,0,0,0,0,0;";
    for (corners, expected) in [
        // Lower Left and Lower Right swapped: the edges cross.
        (
            [("0:0", ""), ("1:0", ""), ("1:1", ""), ("0:1", "")],
            static_quad.to_owned(),
        ),
        // Two corners on one point.
        (
            [("0:0", ""), ("0:0", ""), ("0:1", ""), ("1:1", "")],
            static_quad.to_owned(),
        ),
        // Lower Right inside the other three.
        (
            [("0:0", ""), ("1:0", ""), ("0:1", ""), ("0.3:0.3", "")],
            static_quad.to_owned(),
        ),
        (
            inward,
            "the corners form a degenerate or non-convex quad at the key at source time 1.000 s, which no perspective warp of the clip frame produces".to_owned(),
        ),
        (
            [("a:0", ""), ("1:0", ""), ("0:1", ""), ("1:1", "")],
            "Upper Left \"a:0\" is not a finite point".to_owned(),
        ),
    ] {
        let reason = omitted_reason(corner_pin(20, corners));
        assert!(reason.ends_with(&expected), "{expected}: {reason}");
    }
    // A mirrored quad is convex: it converts.
    let mirrored = [("1:0", ""), ("0:0", ""), ("1:1", ""), ("0:1", "")];
    let (occurrence, omissions) = read(&with_effects(&[(20, corner_pin(20, mirrored))]));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        occurrence.effects,
        [corner_pin_effect(
            true,
            [[1.0, 0.0], [0.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            Vec::new()
        )]
    );
}

/// The easing of [`straight_corner_keys`]: the first key's outgoing mode, and
/// its outgoing and the second key's incoming influence. With zero speeds, a
/// Bezier's influences normalize to the handles `(outgoing, 0, 1 - incoming,
/// 1)` on a path of any length.
type NativeEasing = (u8, [f64; 2]);

const NATIVE_LINEAR: NativeEasing = (0, [1.0 / 6.0, 1.0 / 6.0]);
const NATIVE_HOLD: NativeEasing = (4, [1.0 / 6.0, 1.0 / 6.0]);
const NATIVE_BEZIER: NativeEasing = (5, [0.3, 0.4]);

/// Point keys from `from` at source 0 s to `to` at 1 s on a straight path.
fn straight_corner_keys(
    from: &str,
    to: &str,
    (mode, [outgoing, incoming]): NativeEasing,
) -> String {
    let sixth = 1.0 / 6.0;
    format!("0,{from},{mode},0,0,{sixth},0,{outgoing},0,0,0,0,0,0;{TICKS},{to},0,0,0,{incoming},0,{sixth},0,0,0,0,0,0;")
}

/// Each corner of the identity quad keyed to the opposite corner: the quad is
/// the square at both keys, but all four corners meet at 0.5:0.5 halfway.
const ROTATED_MOVES: [(&str, &str); 4] = [
    ("0:0", "1:1"),
    ("1:0", "0:1"),
    ("0:1", "1:0"),
    ("1:1", "0:0"),
];

/// Each corner of the identity quad keyed to its mirror image, which turns
/// the other way: moving there, the quad folds flat halfway.
const MIRRORED_MOVES: [(&str, &str); 4] = [
    ("0:0", "1:0"),
    ("1:0", "0:0"),
    ("0:1", "1:1"),
    ("1:1", "0:1"),
];

/// A Corner Pin whose corners have the native `keys`, static at the identity
/// quad when empty.
fn keyed_corner_pin(keys: &[String; 4]) -> String {
    corner_pin(
        20,
        std::array::from_fn(|index| (IDENTITY_CORNERS[index].0, keys[index].as_str())),
    )
}

#[test]
fn corner_quads_that_degenerate_between_keys_omit_the_corner_pin() {
    let between = "the corners form a degenerate or non-convex quad between the keys at source times 0.000 s and 1.000 s, which no perspective warp of the clip frame produces";
    // The rotation, Linear and with one Bezier easing that the four corners
    // share, and the mirroring, which only the orientation check catches: no
    // turn has a vertex between the keys.
    for (moves, easing) in [
        (ROTATED_MOVES, NATIVE_LINEAR),
        (ROTATED_MOVES, NATIVE_BEZIER),
        (MIRRORED_MOVES, NATIVE_LINEAR),
    ] {
        let keys = moves.map(|(from, to)| straight_corner_keys(from, to, easing));
        let reason = omitted_reason(keyed_corner_pin(&keys));
        assert!(reason.ends_with(between), "{moves:?} {easing:?}: {reason}");
    }
    // Upper Left 0:0 to 1:0 and Upper Right 1:0 to 2:0 keep a convex quad at
    // both keys. With different easings each corner's progress is bounded on
    // its own, which admits Upper Left at 1:0 while Upper Right is still
    // there, so the quad cannot be proven convex.
    let unproven = "Upper Left and Upper Right move with different easings between the keys at source times 0.000 s and 1.000 s, so their quad there cannot be proven convex";
    for (upper_left, upper_right) in [
        (NATIVE_BEZIER, (5, [0.5, 0.2])),
        (NATIVE_LINEAR, NATIVE_BEZIER),
    ] {
        let keys = [
            straight_corner_keys("0:0", "1:0", upper_left),
            straight_corner_keys("1:0", "2:0", upper_right),
            String::new(),
            String::new(),
        ];
        let reason = omitted_reason(keyed_corner_pin(&keys));
        assert!(
            reason.ends_with(unproven),
            "{upper_left:?} {upper_right:?}: {reason}"
        );
    }
}

#[test]
fn corner_quads_that_stay_convex_between_keys_convert() {
    let translated = [
        ("0:0", "2:0"),
        ("1:0", "3:0"),
        ("0:1", "2:1"),
        ("1:1", "3:1"),
    ];
    for (moves, easing) in [
        // The square translated by 2:0, Linear and with one Bezier easing that
        // the four corners share.
        (translated, NATIVE_LINEAR),
        (translated, NATIVE_BEZIER),
        // Every corner holds, then jumps to the mirrored square: no quad in
        // between is degenerate.
        (MIRRORED_MOVES, NATIVE_HOLD),
    ] {
        let keys = moves.map(|(from, to)| straight_corner_keys(from, to, easing));
        let (occurrence, omissions) = read(&with_effects(&[(20, keyed_corner_pin(&keys))]));
        assert!(omissions.is_empty(), "{moves:?} {easing:?}: {omissions:?}");
        let [effect] = occurrence.effects.as_slice() else {
            panic!("expected one effect: {:?}", occurrence.effects);
        };
        assert_eq!(effect.animations.len(), 4, "{moves:?} {easing:?}");
    }
}

#[test]
fn corner_bezier_keys_premiere_velocity_cannot_hold_omit_the_corner_pin() {
    for (keys, expected) in [
        // A rise between two keys on one point.
        (
            "0,0:0,5,0,0,0.16666666666666666,2,0.5,0,0,0,0,0,0;254016000000,0:0,0,0,0,0.5,0,0.16666666666666666,0,0,0,0,0,0;",
            "Bezier between equal values cannot preserve Premiere velocity",
        ),
        (
            "0,0:0,5,0,0,0.16666666666666666,0,1.5,0,0,0,0,0,0;254016000000,0.5:0,0,0,0,0.5,0,0.16666666666666666,0,0,0,0,0,0;",
            "Bezier influence must be between zero and one",
        ),
    ] {
        let mut corners = IDENTITY_CORNERS;
        corners[0].1 = keys;
        let reason = omitted_reason(corner_pin(20, corners));
        assert!(reason.ends_with(expected), "{expected}: {reason}");
    }
}

#[test]
fn corner_pin_keeps_its_bypass_and_order_around_a_blur() {
    let skew = [
        ("0.1:0.05", ""),
        ("0.95:0", ""),
        ("0:1", ""),
        ("0.85:0.9", ""),
    ];
    // Premiere 26.5.1 writes a bypassed effect's `Bypass` after its `ID`.
    let bypassed = corner_pin(20, skew).replace("<ID>3</ID>", &format!("<ID>3</ID>{BYPASSED}"));
    let pin = |enabled| {
        corner_pin_effect(
            enabled,
            [[0.1, 0.05], [0.95, 0.0], [0.0, 1.0], [0.85, 0.9]],
            Vec::new(),
        )
    };
    // Premiere applies the chain in descending `Index`, so the
    // component written second applies first.
    for (components, expected) in [
        (
            vec![(20, corner_pin(20, skew)), (30, blur(30))],
            [gaussian_blur(true, 25.0, false), pin(true)],
        ),
        (
            vec![(30, blur(30)), (20, bypassed)],
            [pin(false), gaussian_blur(true, 25.0, false)],
        ),
    ] {
        let (occurrence, omissions) = read(&with_effects(&components));
        assert!(omissions.is_empty(), "{omissions:?}");
        assert_eq!(occurrence.effects, expected);
    }
}

#[test]
fn directional_blurs_read_static_keyed_and_bypassed_values_in_stack_order() {
    let hold = |source_ticks, value| PrScalarKeyframe {
        easing: PrKeyframeEasing::Hold,
        ..key(source_ticks, value)
    };
    // A Direction keyed as Premiere 26.5.1 keys a Blur Length: 90, Linear to
    // 45 at 1 s, which holds until -30 at 2 s, whose `StartKeyframe` differs
    // from its first key, as some corpus records' do.
    let direction_keys = "0,90.,0,0,0,0.16666666666666666,0,0.16666666666666666;254016000000,45.,4,0,0,0.16666666666666666,0,0.16666666666666666;508032000000,-30.,0,0,0,0.16666666666666666,0,0.16666666666666666;";
    // Premiere 26.5.1 writes a bypassed effect's `Bypass` after its `ID`.
    let bypassed = directional_blur_xml(30, ("-45.", ""), ("5.5", ""))
        .replace("<ID>3</ID>", &format!("<ID>3</ID>{BYPASSED}"));
    let (occurrence, omissions) = read(&with_effects(&[
        (20, directional_blur_xml(20, ("90.", ""), ("10.", ""))),
        (30, bypassed),
        (40, blur(40)),
        (
            50,
            directional_blur_xml(50, ("0.", direction_keys), ("10.", "")),
        ),
    ]));
    assert!(omissions.is_empty(), "{omissions:?}");
    // Premiere applies the chain in descending `Index`, so the
    // stack starts with the component written last: the keyed Direction, whose
    // keys keep their source times, whose Linear keys drop their automatic
    // handles as Motion keys do, and whose static value is its first key's;
    // a Gaussian Blur; a bypassed blur along the other diagonal; and clip A of
    // the run E2 fixture (the defaults).
    assert_eq!(
        occurrence.effects,
        [
            keyed_directional(
                directional_blur(true, 0.0, 10.0),
                vec![key(0, 90.0), key(TICKS, 45.0), hold(2 * TICKS, -30.0)],
                Vec::new(),
            ),
            gaussian_blur(true, 25.0, false),
            directional_blur(false, -45.0, 5.5),
            directional_blur(true, 90.0, 10.0),
        ]
    );
}

#[test]
fn unconvertible_directional_blurs_are_omitted_with_a_reason() {
    // Premiere 26's default "Directional Blur" is Film Impact's, shown without
    // its 20 parameters, which are not guessed.
    let film_impact = "<VideoFilterComponent ObjectID=\"20\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"><Component Version=\"7\"><ID>3</ID><DisplayName>Directional Blur</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.Impact_Directional_Blur_FX</MatchName></VideoFilterComponent>";
    let keys = "0,10.,0,0,0,0.16666666666666666,0,0.16666666666666666;254016000000,1500.,0,0,0,0.16666666666666666,0,0.16666666666666666;";
    #[rustfmt::skip]
    let blurs = [
        (film_impact.to_owned(), "expected 20 parameters, found 0"),
        (directional_blur_xml(20, ("40000.", ""), ("10.", "")), "Direction \"40000.\" is not a number from -32768 to 32767"),
        (directional_blur_xml(20, ("90.", ""), ("-1.", "")), "Blur Length \"-1.\" is not a number from 0 to 1000"),
        (directional_blur_xml(20, ("90.", ""), ("10.", keys)), "Blur Length key value 1500 is outside Premiere's 0 to 1000 range"),
    ];
    for (records, expected) in blurs {
        let reason = omitted_reason(records);
        assert!(reason.contains(expected), "{expected}: {reason}");
    }
}

#[test]
fn brightness_contrast_reads_static_keyed_and_bypassed_values_in_stack_order() {
    let hold = |source_ticks, value| PrScalarKeyframe {
        easing: PrKeyframeEasing::Hold,
        ..key(source_ticks, value)
    };
    let bezier = |source_ticks, value, [x1, y1, x2, y2]: [f64; 4]| PrScalarKeyframe {
        easing: PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 },
        ..key(source_ticks, value)
    };
    // Clip D of the run E3 fixture: Brightness 0 at source 1 s, Linear to 60
    // at 1.5 s, which holds until 0 at 2.5 s.
    let fixture_keys = "254016000000,0.,0,0,0,0.16666666666666666,12,0.16666666666666666;381024000000,60.,4,0,12,0.16666666666666666,0,0.33333333333333331;635040000000,0.,0,0,-6,0.16666666666666666,0,0.16666666666666666;";
    // Premiere 26.5.1 writes a bypassed effect's `Bypass` after its `ID`.
    let bypassed = brightness_contrast_xml(30, [("37.", ""), ("-25.", "")])
        .replace("<ID>3</ID>", &format!("<ID>3</ID>{BYPASSED}"));
    // The corpus `mixkit-49` (Premiere 12.1.1): a Bezier Brightness pulse and
    // Linear Contrast keys, whose cached values are ignored.
    let corpus = "<VideoFilterComponent ObjectID=\"60\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"7\"><Component Version=\"5\"><Node Version=\"1\"><Properties Version=\"1\"><ECP.Filter.Expanded>false</ECP.Filter.Expanded></Properties></Node><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"61\"/><Param Index=\"1\" ObjectRef=\"62\"/></Params><ID>136</ID><DisplayName>Brightness &amp; Contrast</DisplayName><Bypass>false</Bypass><Intrinsic>false</Intrinsic></Component><MatchName>AE.ADBE Brightness &amp; Contrast 2</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>\
<VideoComponentParam ObjectID=\"61\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"9\"><Name>Brightness</Name><ParameterControlType>2</ParameterControlType><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><Keyframes>914457600000000,0.,5,0,0,0.16666666666666666,3.7459925840439041,1;914529661276595,81.,5,0,-44.613277989816822,0.021764662797867686,-23.542576700475685,0.079265457989522975;914590913361702,0.,5,0,-2.5000000000003837,1,0,0.16666666666666666;</Keyframes><CurrentValue>81</CurrentValue><LowerBound>-100</LowerBound><UpperBound>100</UpperBound><ParameterID>1</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"62\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"9\"><Name>Contrast</Name><ParameterControlType>2</ParameterControlType><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><Keyframes>914457600000000,0.,0,0,0,0.16666666666666666,151.57500000164222,0.16666666666666666;914529661276595,43.,0,0,151.57500000164222,0.16666666666666666,-178.32352940998067,0.16666666666666666;914590913361702,0.,0,0,-178.32352940998067,0.16666666666666666,0,0.16666666666666666;</Keyframes><CurrentValue>43</CurrentValue><LowerBound>-100</LowerBound><UpperBound>100</UpperBound><ParameterID>2</ParameterID></VideoComponentParam>";
    let (occurrence, omissions) = read(&with_effects(&[
        (20, brightness_contrast_xml(20, [("-40.", ""), ("85.", "")])),
        (30, bypassed),
        (40, blur(40)),
        (
            50,
            brightness_contrast_xml(50, [("0.", fixture_keys), ("20.", "")]),
        ),
        (60, corpus.to_owned()),
    ]));
    assert!(omissions.is_empty(), "{omissions:?}");
    // Premiere applies the chain in descending `Index`, so the
    // stack starts with the component written last: the corpus effect, clip D,
    // a Gaussian Blur, a bypassed one and clip C. Keys keep their source times,
    // Linear keys drop their automatic handles and Bezier keys become cubic
    // easing, as Motion keys do.
    let [start, peak, end] = [914457600000000, 914529661276595, 914590913361702];
    #[rustfmt::skip]
    let expected = [
        brightness_contrast(true, [0.0, 0.0], [
            vec![
                key(start, 0.0),
                bezier(peak, 81.0, [1.0, 0.013119665822625665, 0.9782353372021323, 1.0034007283136184]),
                bezier(end, 0.0, [0.07926545798952297, 0.00555536697581702, 0.0, 0.9925575693896446]),
            ],
            vec![key(start, 0.0), key(peak, 43.0), key(end, 0.0)],
        ]),
        brightness_contrast(true, [0.0, 20.0], [
            vec![key(TICKS, 0.0), key(3 * TICKS / 2, 60.0), hold(5 * TICKS / 2, 0.0)],
            vec![],
        ]),
        gaussian_blur(true, 25.0, false),
        brightness_contrast(false, [37.0, -25.0], [vec![], vec![]]),
        brightness_contrast(true, [-40.0, 85.0], [vec![], vec![]]),
    ];
    assert_eq!(occurrence.effects, expected);
}

#[test]
fn unknown_brightness_contrast_forms_are_omitted_with_a_reason() {
    let base = brightness_contrast_xml(20, [("37.", ""), ("-25.", "")]);
    // AE's "Use Legacy" checkbox, which no Premiere record has.
    let third = base.replace("<Param Index=\"1\" ObjectRef=\"22\"/>", "<Param Index=\"1\" ObjectRef=\"22\"/><Param Index=\"2\" ObjectRef=\"23\"/>")
        + "<VideoComponentParam ObjectID=\"23\" ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\" Version=\"10\"><Name>Use Legacy</Name><ParameterID>3</ParameterID><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound></VideoComponentParam>";
    #[rustfmt::skip]
    let records = [
        (third, "expected 2 parameters, found 3"),
        (base.replace("<ParameterID>2</ParameterID>", "<ParameterID>3</ParameterID>"), "unknown ParameterID 3"),
        (base.replace("</Component>", "</Component><PremiereFilterPrivateData Encoding=\"base64\" BinaryHash=\"c40c6399-6b26-8c2c-feaf-d01b0000000d\">AA==</PremiereFilterPrivateData>"), "PremiereFilterPrivateData is not supported"),
    ];
    for (changed, expected) in records {
        assert_ne!(changed, base, "{expected}");
        let reason = omitted_reason(changed);
        assert!(
            reason.starts_with("active effect \"Brightness & Contrast\" (match name \"AE.ADBE Brightness & Contrast 2\", VideoFilterComponent version 9, Component version 7) at stack position 1")
                && reason.ends_with(expected),
            "{expected}: {reason}"
        );
    }
}

/// Tint's default colours: Map Black To 0 (alpha 0) and Map White To
/// 0x0000FF00FF00FF00 (alpha 0), as Premiere 26.5.1 saves them.
const TINT_DEFAULT_BLACK: &str = "0";
const TINT_DEFAULT_WHITE: &str = "280379743338240";
/// Opaque (163, 247, 143) and (240, 242, 22), the `abstract_slideshow` pair
/// that clip B of the run E6 fixture reuses.
const TINT_GREEN: &str = "18374865704210960128";
const TINT_YELLOW: &str = "18374950366522381824";
/// Opaque (255, 128, 0), clips C and D of the run E6 fixture.
const TINT_ORANGE: &str = "18374966857284190208";

/// An `AE.ADBE Tint` component and its three parameter records in the shape
/// Premiere 26.5.1 saves (`feature_tint_strict`): no `Bypass`,
/// `Intrinsic`, control type or colour bounds; per parameter its static
/// value and, when keyed, `IsTimeVarying` and `Keyframes`. Records use
/// ObjectIDs `id..id + 3`.
fn tint_26_5_xml(id: u32, [black, white, amount]: [(&str, &str); 3]) -> String {
    let param = |object: u32,
                 class: &str,
                 name: &str,
                 parameter_id: u32,
                 bounds: &str,
                 (value, keys): (&str, &str)| {
        let (time_varying, keyframes) = if keys.is_empty() {
            ("", String::new())
        } else {
            (
                "<IsTimeVarying>true</IsTimeVarying>",
                format!("<Keyframes>{keys}</Keyframes>"),
            )
        };
        format!(
            "<VideoComponentParam ObjectID=\"{object}\" ClassID=\"{class}\" Version=\"10\"><Name>{name}</Name>{time_varying}<ParameterID>{parameter_id}</ParameterID><StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0</StartKeyframe>{keyframes}{bounds}</VideoComponentParam>"
        )
    };
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"><Component Version=\"7\"><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"{}\"/><Param Index=\"1\" ObjectRef=\"{}\"/><Param Index=\"2\" ObjectRef=\"{}\"/></Params><ID>3</ID><DisplayName>Tint</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Tint</MatchName></VideoFilterComponent>{}{}{}",
        id + 1,
        id + 2,
        id + 3,
        param(id + 1, "0fde4e9f-f895-4ba3-b0fe-9a6feafda583", "Map Black To", 1, "", black),
        param(id + 2, "0fde4e9f-f895-4ba3-b0fe-9a6feafda583", "Map White To", 2, "", white),
        param(id + 3, "fe47129e-6c94-4fc0-95d5-c056a517aaf3", "Amount to Tint", 3, "<LowerBound>0</LowerBound><UpperBound>100</UpperBound>", amount),
    )
}

/// An `AE.ADBE Black & White` component as Premiere 26.5.1 saves it (`feature_black_white_strict`, verbatim apart from the ids): no
/// `Params`, `Bypass` or `Intrinsic`.
fn black_white_26_5_xml(id: u32) -> String {
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"><Component Version=\"7\"><ID>3</ID><DisplayName>Black &amp; White</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Black &amp; White</MatchName></VideoFilterComponent>"
    )
}

fn colour(rgb: [u8; 3]) -> PrColour {
    PrColour { rgb }
}

fn colour_key(source_ticks: i64, rgb: [u8; 3], easing: PrKeyframeEasing) -> PrColourKeyframe {
    PrColourKeyframe {
        source_ticks,
        value: colour(rgb),
        easing,
    }
}

/// A Tint with the static `black`, `white` and `amount` and the keys of its
/// parameters in native order, possibly empty. A keyed static value is its
/// first key's.
fn tint_effect(
    enabled: bool,
    (black, white, amount): ([u8; 3], [u8; 3], f64),
    (black_keys, white_keys, amount_keys): (
        Vec<PrColourKeyframe>,
        Vec<PrColourKeyframe>,
        Vec<PrScalarKeyframe>,
    ),
) -> PrEffect {
    let mut animations = Vec::new();
    for (param, keys) in [
        (&TINT_MAP_BLACK_TO, black_keys),
        (&TINT_MAP_WHITE_TO, white_keys),
    ] {
        if !keys.is_empty() {
            animations.push(PrEffectParamAnimation {
                param,
                keys: PrEffectParamKeys::Colour(keys),
            });
        }
    }
    if !amount_keys.is_empty() {
        animations.push(PrEffectParamAnimation {
            param: &TINT_AMOUNT,
            keys: PrEffectParamKeys::Scalar(amount_keys),
        });
    }
    PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Tint(PrTint {
            black: colour(black),
            white: colour(white),
            amount,
        }),
        animations,
    }
}

fn black_white_effect(enabled: bool) -> PrEffect {
    PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::BlackWhite,
        animations: Vec::new(),
    }
}

#[test]
fn tints_and_black_whites_read_static_keyed_and_bypassed_values_in_stack_order() {
    // Clip D of the run E6 fixture: Amount 0 at source 1 s, Linear to 100 at
    // 1.5 s, which holds until 50 at 2.5 s. Clip E: Map White To white at
    // source 0.5 s, Linear to (0, 128, 255) at 1 s; its StartKeyframe keeps
    // the default.
    let amount_keys = "254016000000,0.,0,0,0,0.16666666666666666,20,0.16666666666666666;381024000000,100.,4,0,20,0.16666666666666666,0,0.33333333333333331;635040000000,50.,0,0,-5,0.16666666666666666,0,0.16666666666666666;";
    let white_keys = "127008000000,18374966859414961920,0,0,0,0.16666666666666666,0,0.16666666666666666;254016000000,18374686481819172608,0,0,0,0.16666666666666666,0,0.16666666666666666;";
    let (occurrence, omissions) = read(&with_effects(&[
        (
            20,
            tint_26_5_xml(
                20,
                [
                    (TINT_DEFAULT_BLACK, ""),
                    (TINT_DEFAULT_WHITE, ""),
                    ("100.", ""),
                ],
            ),
        ),
        (
            30,
            tint_26_5_xml(30, [(TINT_GREEN, ""), (TINT_YELLOW, ""), ("100.", "")]),
        ),
        (
            40,
            tint_26_5_xml(
                40,
                [(TINT_DEFAULT_BLACK, ""), (TINT_ORANGE, ""), ("50.", "")],
            ),
        ),
        (
            50,
            tint_26_5_xml(
                50,
                [
                    (TINT_DEFAULT_BLACK, ""),
                    (TINT_ORANGE, ""),
                    ("100.", amount_keys),
                ],
            ),
        ),
        (
            60,
            tint_26_5_xml(
                60,
                [
                    (TINT_DEFAULT_BLACK, ""),
                    (TINT_DEFAULT_WHITE, white_keys),
                    ("100.", ""),
                ],
            ),
        ),
        (70, black_white_26_5_xml(70)),
        (80, blur(80)),
        // The corpus form (`abstract_slideshow`, Premiere 12.1) and it bypassed.
        (90, tint(90)),
        (100, tint(100).replace(ACTIVE, BYPASSED)),
    ]));
    assert!(omissions.is_empty(), "{omissions:?}");
    // Premiere applies the chain in descending `Index`, so the
    // stack starts with the component written last. The alpha of a colour is
    // ignored, a keyed parameter starts at its first key, and the Hold out of
    // the 1.5 s key is the easing into the 2.5 s key, as for Motion keys.
    let linear = PrKeyframeEasing::Linear;
    let hold = PrScalarKeyframe {
        easing: PrKeyframeEasing::Hold,
        ..key(5 * TICKS / 2, 50.0)
    };
    let (black, white, green, yellow, orange) = (
        [0, 0, 0],
        [255, 255, 255],
        [163, 247, 143],
        [240, 242, 22],
        [255, 128, 0],
    );
    #[rustfmt::skip]
    let expected = [
        tint_effect(false, (green, yellow, 100.0), (vec![], vec![], vec![])),
        tint_effect(true, (green, yellow, 100.0), (vec![], vec![], vec![])),
        gaussian_blur(true, 25.0, false),
        black_white_effect(true),
        tint_effect(true, (black, white, 100.0), (vec![], vec![colour_key(TICKS / 2, white, linear), colour_key(TICKS, [0, 128, 255], linear)], vec![])),
        tint_effect(true, (black, orange, 0.0), (vec![], vec![], vec![key(TICKS, 0.0), key(3 * TICKS / 2, 100.0), hold])),
        tint_effect(true, (black, orange, 50.0), (vec![], vec![], vec![])),
        tint_effect(true, (green, yellow, 100.0), (vec![], vec![], vec![])),
        tint_effect(true, (black, white, 100.0), (vec![], vec![], vec![])),
    ];
    assert_eq!(occurrence.effects, expected);
}

#[test]
fn unconvertible_tints_and_black_whites_are_omitted_with_a_reason() {
    let base = tint_26_5_xml(20, [(TINT_GREEN, ""), (TINT_YELLOW, ""), ("100.", "")]);
    let bezier_white = tint_26_5_xml(20, [(TINT_DEFAULT_BLACK, ""), (TINT_DEFAULT_WHITE, "127008000000,18374966859414961920,5,0,0,0.3,0,0.4;254016000000,18374686481819172608,0,0,0,0.3,0,0.4;"), ("100.", "")]);
    let tint_prefix = "active effect \"Tint\" (match name \"AE.ADBE Tint\", VideoFilterComponent version 9, Component version 7) at stack position 1";
    #[rustfmt::skip]
    let records = [
        // A nonzero low byte in the green channel: no 8-bit colour.
        (base.replace(TINT_YELLOW, "18374950366522381825"), tint_prefix, "Map White To colour 0xff00f000f2001601 has a nonzero low byte in a channel; only 8-bit colours convert"),
        (base.replace(TINT_GREEN, "18374865704210960128.5"), tint_prefix, "Map Black To \"18374865704210960128.5\" is not a native colour value"),
        (tint_26_5_xml(20, [(TINT_DEFAULT_BLACK, ""), (TINT_DEFAULT_WHITE, "127008000000,18374966859414961921,0,0,0,0,0,0;"), ("100.", "")]), tint_prefix, "key colour 0xff00ff00ff00ff01 has a nonzero low byte in a channel; only 8-bit colours convert"),
        (bezier_white, tint_prefix, "Bezier keys are not supported; Premiere's Bezier interpolation between colours is unverified"),
        (base.replace(",100.,", ",150.,"), tint_prefix, "Amount to Tint \"150.\" is not a number from 0 to 100"),
        (tint_26_5_xml(20, [(TINT_DEFAULT_BLACK, ""), (TINT_DEFAULT_WHITE, ""), ("100.", "254016000000,150.,0,0,0,0.16666666666666666,0,0.16666666666666666;")]), tint_prefix, "Amount to Tint key value 150 is outside Premiere's 0 to 100 range"),
        (base.replace("<ParameterID>3</ParameterID>", "<ParameterID>4</ParameterID>"), tint_prefix, "unknown ParameterID 4"),
        (base.replace("<Param Index=\"2\" ObjectRef=\"23\"/>", ""), tint_prefix, "expected 3 parameters, found 2"),
        // A Black & White with a parameter is not the record Premiere saves.
        (black_white_26_5_xml(20).replace("<ID>3</ID>", "<Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"21\"/></Params><ID>3</ID>") + "<VideoComponentParam ObjectID=\"21\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"10\"><Name>Amount</Name><ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound></VideoComponentParam>", "active effect \"Black & White\" (match name \"AE.ADBE Black & White\", VideoFilterComponent version 9, Component version 7) at stack position 1", "expected 0 parameters, found 1"),
    ];
    for (changed, prefix, expected) in records {
        assert_ne!(changed, base, "{expected}");
        let reason = omitted_reason(changed);
        assert!(
            reason.starts_with(prefix) && reason.ends_with(expected),
            "{expected}: {reason}"
        );
    }
    // The corpus's older `PR.ADBE Black & White` is another effect and stays
    // unknown.
    let reason = omitted_reason(black_white_pr(20));
    assert!(
        reason.starts_with("unknown active effect \"Black & White\" (match name \"PR.ADBE Black & White\", VideoFilterComponent version 7, Component version 5)")
            && reason.ends_with("no Tesseract effect mapping"),
        "{reason}"
    );
}

/// Native colours of the run E10 fixture: opaque black and white (the
/// defaults), B's (200, 40, 40) and (40, 40, 200), E's yellow and blue.
const RAMP_BLACK: &str = "18374686479671623680";
const RAMP_WHITE: &str = "18374966859414961920";
const RAMP_RED: &str = "18374906382668277760";
const RAMP_BLUE_GREY: &str = "18374730460807874560";
const RAMP_YELLOW: &str = "18374966859414896640";
const RAMP_BLUE: &str = "18374686479671688960";

/// The static point and colour parameters of a Ramp with its `Blend With
/// Original`: each is `(value, keys)`, and Shape and Scatter are the given
/// static values.
struct RampXml<'a> {
    start: (&'a str, &'a str),
    start_colour: (&'a str, &'a str),
    end: (&'a str, &'a str),
    end_colour: (&'a str, &'a str),
    shape: &'a str,
    scatter: &'a str,
    blend: (&'a str, &'a str),
}

/// The default vertical black-to-white ramp, Blend 0.
const DEFAULT_RAMP: RampXml<'static> = RampXml {
    start: ("0.5:0", ""),
    start_colour: (RAMP_BLACK, ""),
    end: ("0.5:1", ""),
    end_colour: (RAMP_WHITE, ""),
    shape: "0",
    scatter: "0.",
    blend: ("0.", ""),
};

/// An `AE.ADBE Ramp` component and its seven parameter records as Premiere
/// 26.5.1 saves them (`feature_ramp_strict`, verbatim apart
/// from the ids): no `Bypass` or `Intrinsic`; points `PointComponentParam`
/// version 4 without a control type, colours class `0fde4e9f` version 10
/// without bounds, the Shape popup, Scatter 0–512 and Blend 0–1. Records use
/// ObjectIDs `id..id + 7`.
fn ramp_26_5_xml(id: u32, ramp: RampXml<'_>) -> String {
    let time_varying = |keys: &str| {
        if keys.is_empty() {
            return (String::new(), String::new());
        }
        (
            "<IsTimeVarying>true</IsTimeVarying>".to_owned(),
            format!("<Keyframes>{keys}</Keyframes>"),
        )
    };
    let point = |object: u32, name: &str, parameter_id: u32, (value, keys): (&str, &str)| {
        let (time_varying, keyframes) = time_varying(keys);
        format!(
            "<PointComponentParam ObjectID=\"{object}\" ClassID=\"ca81d347-309b-44d2-acc7-1c572efb973c\" Version=\"4\"><Name>{name}</Name>{time_varying}<ParameterID>{parameter_id}</ParameterID><StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe>{keyframes}</PointComponentParam>"
        )
    };
    let scalar = |object: u32,
                  class: &str,
                  name: &str,
                  parameter_id: u32,
                  extra: &str,
                  bounds: &str,
                  (value, keys): (&str, &str)| {
        let (time_varying, keyframes) = time_varying(keys);
        format!(
            "<VideoComponentParam ObjectID=\"{object}\" ClassID=\"{class}\" Version=\"10\"><Name>{name}</Name>{time_varying}{extra}<ParameterID>{parameter_id}</ParameterID><StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0</StartKeyframe>{keyframes}{bounds}</VideoComponentParam>"
        )
    };
    let colour = "0fde4e9f-f895-4ba3-b0fe-9a6feafda583";
    let number = "fe47129e-6c94-4fc0-95d5-c056a517aaf3";
    let params: String = (0..7)
        .map(|index| {
            format!(
                "<Param Index=\"{index}\" ObjectRef=\"{}\"/>",
                id + 1 + index
            )
        })
        .collect();
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"><Component Version=\"7\"><Params Version=\"1\">{params}</Params><ID>3</ID><DisplayName>Ramp</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Ramp</MatchName></VideoFilterComponent>{}{}{}{}{}{}{}",
        point(id + 1, "Start of Ramp", 1, ramp.start),
        scalar(id + 2, colour, "Start Color", 2, "", "", ramp.start_colour),
        point(id + 3, "End of Ramp", 3, ramp.end),
        scalar(id + 4, colour, "End Color", 4, "", "", ramp.end_colour),
        scalar(id + 5, "6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8", "Ramp Shape", 5, "<DiscontinuousInterpolate>true</DiscontinuousInterpolate>", "<LowerBound>0</LowerBound><UpperBound>1</UpperBound>", (ramp.shape, "")),
        scalar(id + 6, number, "Ramp Scatter", 6, "<UpperUIBound>50</UpperUIBound>", "<LowerBound>0</LowerBound><UpperBound>512</UpperBound>", (ramp.scatter, "")),
        scalar(id + 7, number, "Blend With Original", 7, "", "<LowerBound>0</LowerBound><UpperBound>1</UpperBound>", ramp.blend),
    )
}

/// The Ramp of the corpus `corporate_slideshow` (Premiere 12.1, 7/5 records
/// with `Bypass`, `Intrinsic`, control types and colour bounds): the default
/// vertical ramp.
fn corpus_ramp(id: u32) -> String {
    let point = |object: u32, name: &str, parameter_id: u32, value: &str| {
        format!(
            "<PointComponentParam ObjectID=\"{object}\" ClassID=\"ca81d347-309b-44d2-acc7-1c572efb973c\" Version=\"3\"><Name>{name}</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>6</ParameterControlType><StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe><ParameterID>{parameter_id}</ParameterID></PointComponentParam>"
        )
    };
    let scalar = |object: u32,
                  class: &str,
                  name: &str,
                  parameter_id: u32,
                  control: &str,
                  extra: &str,
                  bounds: [&str; 2],
                  value: &str| {
        format!(
            "<VideoComponentParam ObjectID=\"{object}\" ClassID=\"{class}\" Version=\"9\"><Name>{name}</Name><IsTimeVarying>false</IsTimeVarying>{extra}<ParameterControlType>{control}</ParameterControlType><StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0</StartKeyframe><LowerBound>{}</LowerBound><UpperBound>{}</UpperBound><ParameterID>{parameter_id}</ParameterID></VideoComponentParam>",
            bounds[0], bounds[1]
        )
    };
    let colour = "0fde4e9f-f895-4ba3-b0fe-9a6feafda583";
    let number = "fe47129e-6c94-4fc0-95d5-c056a517aaf3";
    let params: String = (0..7)
        .map(|index| {
            format!(
                "<Param Index=\"{index}\" ObjectRef=\"{}\"/>",
                id + 1 + index
            )
        })
        .collect();
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"7\"><Component Version=\"5\"><Params Version=\"1\">{params}</Params><ID>3</ID><DisplayName>Ramp</DisplayName>{ACTIVE}<Intrinsic>false</Intrinsic></Component><MatchName>AE.ADBE Ramp</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>{}{}{}{}{}{}{}",
        point(id + 1, "Start of Ramp", 1, "0.5:0"),
        scalar(id + 2, colour, "Start Color", 2, "5", "", ["0", "18446744073709551615"], RAMP_BLACK),
        point(id + 3, "End of Ramp", 3, "0.5:1"),
        scalar(id + 4, colour, "End Color", 4, "5", "", ["0", "18446744073709551615"], RAMP_WHITE),
        scalar(id + 5, "6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8", "Ramp Shape", 5, "7", "<DiscontinuousInterpolate>true</DiscontinuousInterpolate>", ["0", "1"], "0"),
        scalar(id + 6, number, "Ramp Scatter", 6, "2", "", ["0", "512"], "0.").replace("<ParameterID>6</ParameterID>", "<ParameterID>6</ParameterID><UpperUIBound>50</UpperUIBound>"),
        scalar(id + 7, number, "Blend With Original", 7, "2", "", ["0", "1"], "0."),
    )
}

/// A Ramp with static `start`, `end`, colours and `blend`, and the keys of its
/// keyed parameters in native order (a keyed static value is its first key's).
fn ramp_effect(
    enabled: bool,
    (start, end): ([f64; 2], [f64; 2]),
    (start_colour, end_colour): ([u8; 3], [u8; 3]),
    blend: f64,
    animations: Vec<PrEffectParamAnimation>,
) -> PrEffect {
    PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Ramp(PrRamp {
            start,
            start_colour: colour(start_colour),
            end,
            end_colour: colour(end_colour),
            blend,
        }),
        animations,
    }
}

#[test]
fn ramps_read_static_keyed_and_bypassed_values_in_stack_order() {
    // Clips B, C, D and E of the run E10 fixture (verbatim keys): B a
    // horizontal ramp with colours and Blend 0.3; C Blend 1 at source 1 s,
    // Linear to 0 at 1.5 s, held until 0.5 at 2.5 s; D's End of Ramp 0.5:1 at
    // 0.5 s, Linear to 0.5:0.6 at 1.5 s; E a reversed vertical ramp.
    let blend_keys = "254016000000,1.,0,0,0,0.16666666666666666,-2,0.16666666666666666;381024000000,0.,4,0,-2,0.16666666666666666,0,0.33333333333333331;635040000000,0.5,0,0,0.5,0.16666666666666666,0,0.16666666666666666;";
    let end_keys = "127008000000,0.5:1,0,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;381024000000,0.5:0.59999999999999998,0,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;";
    let (occurrence, omissions) = read(&with_effects(&[
        (20, ramp_26_5_xml(20, DEFAULT_RAMP)),
        (
            30,
            ramp_26_5_xml(
                30,
                RampXml {
                    start: ("0.20000000000000001:0.5", ""),
                    start_colour: (RAMP_RED, ""),
                    end: ("0.80000000000000004:0.5", ""),
                    end_colour: (RAMP_BLUE_GREY, ""),
                    blend: ("0.300000011921", ""),
                    ..DEFAULT_RAMP
                },
            ),
        ),
        (
            40,
            ramp_26_5_xml(
                40,
                RampXml {
                    blend: ("0.", blend_keys),
                    ..DEFAULT_RAMP
                },
            ),
        ),
        (
            50,
            ramp_26_5_xml(
                50,
                RampXml {
                    end: ("0.5:1", end_keys),
                    ..DEFAULT_RAMP
                },
            ),
        ),
        (
            60,
            ramp_26_5_xml(
                60,
                RampXml {
                    start: ("0.5:0.90000000000000002", ""),
                    start_colour: (RAMP_YELLOW, ""),
                    end: ("0.5:0.10000000000000001", ""),
                    end_colour: (RAMP_BLUE, ""),
                    ..DEFAULT_RAMP
                },
            ),
        ),
        (70, blur(70)),
        (80, corpus_ramp(80)),
        (90, corpus_ramp(90).replace(ACTIVE, BYPASSED)),
    ]));
    assert!(omissions.is_empty(), "{omissions:?}");
    // Premiere applies the chain in descending `Index`, so the
    // stack starts with the component written last. A keyed parameter starts
    // at its first key, and the Hold out of the 1.5 s key is the easing into
    // the 2.5 s key, as for Motion keys.
    let linear = PrKeyframeEasing::Linear;
    let (black, white) = ([0, 0, 0], [255, 255, 255]);
    let default = ([0.5, 0.0], [0.5, 1.0]);
    let hold = PrScalarKeyframe {
        easing: PrKeyframeEasing::Hold,
        ..key(5 * TICKS / 2, 0.5)
    };
    #[rustfmt::skip]
    let expected = [
        ramp_effect(false, default, (black, white), 0.0, vec![]),
        ramp_effect(true, default, (black, white), 0.0, vec![]),
        gaussian_blur(true, 25.0, false),
        ramp_effect(true, ([0.5, 0.9], [0.5, 0.1]), ([255, 255, 0], [0, 0, 255]), 0.0, vec![]),
        ramp_effect(true, default, (black, white), 0.0, vec![PrEffectParamAnimation {
            param: &RAMP_END,
            keys: PrEffectParamKeys::Point(vec![point_key(TICKS / 2, [0.5, 1.0], linear), point_key(3 * TICKS / 2, [0.5, 0.6], linear)]),
        }]),
        ramp_effect(true, default, (black, white), 1.0, vec![PrEffectParamAnimation {
            param: &RAMP_BLEND,
            keys: PrEffectParamKeys::Scalar(vec![key(TICKS, 1.0), key(3 * TICKS / 2, 0.0), hold]),
        }]),
        ramp_effect(true, ([0.2, 0.5], [0.8, 0.5]), ([200, 40, 40], [40, 40, 200]), 0.300000011921, vec![]),
        ramp_effect(true, default, (black, white), 0.0, vec![]),
    ];
    assert_eq!(occurrence.effects, expected);
}

#[test]
fn unconvertible_ramps_are_omitted_with_a_reason() {
    let base = ramp_26_5_xml(20, DEFAULT_RAMP);
    let prefix = "active effect \"Ramp\" (match name \"AE.ADBE Ramp\", VideoFilterComponent version 9, Component version 7) at stack position 1";
    let keyed = |end: &str, start: &str| {
        ramp_26_5_xml(
            20,
            RampXml {
                start: ("0.5:0", start),
                end: ("0.5:1", end),
                ..DEFAULT_RAMP
            },
        )
    };
    // Straight point keys: (time, x:y, mode 0 Linear, then zero handles and
    // spatial fields).
    let point_keys = |keys: &[(&str, &str)]| {
        keys.iter()
            .map(|(time, value)| {
                format!(
                    "{time},{value},0,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;"
                )
            })
            .collect::<String>()
    };
    let diagonal_reason = "is not aligned with the frame at every time; Premiere measures a ramp in clip pixels and the FX gradientRamp in frame UV, which agree only along the frame's axes";
    #[rustfmt::skip]
    let records = [
        (base.replace("<StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe>", "<StartKeyframe>-91445760000000000,1,0,0,0,0,0,0</StartKeyframe>"), "Ramp Shape \"1\" is not linear (0); a radial ramp is not converted, because Premiere measures its radius in clip pixels and the FX gradientRamp in frame UV".to_owned()),
        (ramp_26_5_xml(20, RampXml { scatter: "12.", ..DEFAULT_RAMP }), "Ramp Scatter 12 is not converted; FX has no scatter".to_owned()),
        // The corpus diagonal 0.3406:0.4426 to 0.5693:0.7602.
        (ramp_26_5_xml(20, RampXml { start: ("0.3406:0.4426", ""), end: ("0.5693:0.7602", ""), ..DEFAULT_RAMP }), format!("Start of Ramp 0.3406:0.4426 to End of Ramp 0.5693:0.7602 {diagonal_reason}")),
        (ramp_26_5_xml(20, RampXml { end: ("0.5:0", ""), ..DEFAULT_RAMP }), "Start of Ramp and End of Ramp are both 0.5:0; a ramp of zero length is not converted".to_owned()),
        // The end's keys leave the axis.
        (keyed(&point_keys(&[("127008000000", "0.5:1"), ("381024000000", "0.6:0.6")]), ""), format!("Start of Ramp 0.5:0 to End of Ramp 0.5:1 {diagonal_reason}")),
        // The end reaches the start along the axis.
        (keyed(&point_keys(&[("127008000000", "0.5:1"), ("381024000000", "0.5:0")]), ""), "Start of Ramp and End of Ramp meet: their y coordinates reach 0..0 and 0..1 over their keys, and a ramp of zero length is not converted".to_owned()),
        (ramp_26_5_xml(20, RampXml { end: ("0.5:0.001", ""), ..DEFAULT_RAMP }), "Start of Ramp and End of Ramp come within 0.0010 of the frame of each other along y (their coordinates reach 0..0 and 0.001..0.001 over their keys); a ramp shorter than 0.0032 of the frame is not converted, because the FX gradientRamp floors its squared length at 1e-5 and would stretch it over 0.0032 of the frame".to_owned()),
        // A curved spatial path (mode 5 with tangents), as for a corner.
        (keyed("127008000000,0.5:1,5,0,0,0.16666666666666666,0,0.16666666666666666,5,4,0,0,0.1,0;381024000000,0.5:0.6,0,0,0,0.16666666666666666,0,0.16666666666666666,5,4,0,0,0,0;", ""), "End of Ramp moves on a curved spatial path between its keys at source times 0.500 s and 1.500 s; only a straight path converts, because FX keys each coordinate separately".to_owned()),
        (base.replace(RAMP_WHITE, "18374966859414961921"), "End Color colour 0xff00ff00ff00ff01 has a nonzero low byte in a channel; only 8-bit colours convert".to_owned()),
        (ramp_26_5_xml(20, RampXml { blend: ("1.5", ""), ..DEFAULT_RAMP }), "Blend With Original \"1.5\" is not a number from 0 to 1".to_owned()),
        (ramp_26_5_xml(20, RampXml { blend: ("0.", "254016000000,2.,0,0,0,0.16666666666666666,0,0.16666666666666666;"), ..DEFAULT_RAMP }), "Blend With Original key value 2 is outside Premiere's 0 to 1 range".to_owned()),
        (base.replace("<ParameterID>7</ParameterID>", "<ParameterID>8</ParameterID>"), "unknown ParameterID 8".to_owned()),
        (base.replace("<Param Index=\"6\" ObjectRef=\"27\"/>", ""), "expected 7 parameters, found 6".to_owned()),
    ];
    for (changed, expected) in records {
        assert_ne!(changed, base, "{expected}");
        let reason = omitted_reason(changed);
        assert!(
            reason.starts_with(prefix) && reason.ends_with(&expected),
            "{expected}: {reason}"
        );
    }
    // The Film Impact gradient and AE's 4-Color Gradient stay unknown effects.
    let reason = omitted_reason(base.replace("AE.ADBE Ramp", "AE.ADBE 4ColorGradient"));
    assert!(
        reason.starts_with("unknown active effect \"Ramp\" (match name \"AE.ADBE 4ColorGradient\"")
            && reason.ends_with("no Tesseract effect mapping"),
        "{reason}"
    );
}

/// The Hold count keys of clip D of the run E8 fixture (verbatim): 10 at
/// source 1 s, 40 (or 30) at 1.5 s, 20 at 2.5 s, every key mode 4.
const MOSAIC_HORIZONTAL_KEYS: &str = "254016000000,10,4,0,0,0.16666666666666666,0,0.33333333333333331;381024000000,40,4,0,60,0.16666666666666666,0,0.33333333333333331;635040000000,20,4,0,-2,0.16666666666666666,0,0.33333333333333331;";
const MOSAIC_VERTICAL_KEYS: &str = "254016000000,10,4,0,0,0.16666666666666666,0,0.33333333333333331;381024000000,30,4,0,40,0.16666666666666666,0,0.33333333333333331;635040000000,20,4,0,-1,0.16666666666666666,0,0.33333333333333331;";

/// An `AE.ADBE Mosaic` component and its three parameter records as Premiere
/// 26.5.1 saves them (`feature_mosaic_strict`, verbatim apart
/// from the ids): no `Bypass` or `Intrinsic`; the counts class `6e02e8bb`
/// version 10 with control type 1, bounds 1–4000 and UI bound 200; the Sharp
/// Colors checkbox class `cc12343e` version 10 **without a `Name`**, control
/// type or bounds. Each count is `(value, keys)`. Records use ObjectIDs
/// `id..id + 3`.
fn mosaic_26_5_xml(
    id: u32,
    horizontal: (&str, &str),
    vertical: (&str, &str),
    sharp_colors: &str,
) -> String {
    let count = |object: u32, name: &str, parameter_id: u32, (value, keys): (&str, &str)| {
        let (time_varying, keyframes) = if keys.is_empty() {
            (String::new(), String::new())
        } else {
            (
                "<IsTimeVarying>true</IsTimeVarying>".to_owned(),
                format!("<Keyframes>{keys}</Keyframes>"),
            )
        };
        format!(
            "<VideoComponentParam ObjectID=\"{object}\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"10\"><Name>{name}</Name><ParameterControlType>1</ParameterControlType>{time_varying}<ParameterID>{parameter_id}</ParameterID><UpperUIBound>200</UpperUIBound><StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0</StartKeyframe>{keyframes}<LowerBound>1</LowerBound><UpperBound>4000</UpperBound></VideoComponentParam>"
        )
    };
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"><Component Version=\"7\"><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"{}\"/><Param Index=\"1\" ObjectRef=\"{}\"/><Param Index=\"2\" ObjectRef=\"{}\"/></Params><ID>3</ID><DisplayName>Mosaic (Legacy)</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Mosaic</MatchName></VideoFilterComponent>{}{}<VideoComponentParam ObjectID=\"{}\" ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\" Version=\"10\"><ParameterID>3</ParameterID><StartKeyframe>-91445760000000000,{sharp_colors},0,0,0,0,0,0</StartKeyframe></VideoComponentParam>",
        id + 1,
        id + 2,
        id + 3,
        count(id + 1, "Horizontal Blocks", 1, horizontal),
        count(id + 2, "Vertical Blocks", 2, vertical),
        id + 3,
    )
}

/// A corpus-generation Mosaic (`VideoFilterComponent` 7 / `Component` 5 with
/// `Bypass` and `Intrinsic`, version-9 parameters with control types and
/// bounds), the 10 × 10 defaults with Sharp Colors on. The scout's scan found
/// the corpus checkbox without a `Name` too; `name` is the checkbox's `Name`
/// element, if any, so the corpus Gaussian Blur form `<Name> </Name>` and a
/// named checkbox can be tried.
fn corpus_mosaic(id: u32, name: &str) -> String {
    let count = |object: u32, name: &str, parameter_id: u32, value: &str| {
        format!(
            "<VideoComponentParam ObjectID=\"{object}\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"9\"><Name>{name}</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>1</ParameterControlType><StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0</StartKeyframe><LowerBound>1</LowerBound><UpperBound>4000</UpperBound><ParameterID>{parameter_id}</ParameterID><UpperUIBound>200</UpperUIBound></VideoComponentParam>"
        )
    };
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"7\"><Component Version=\"5\"><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"{}\"/><Param Index=\"1\" ObjectRef=\"{}\"/><Param Index=\"2\" ObjectRef=\"{}\"/></Params><ID>3</ID><DisplayName>Mosaic</DisplayName>{ACTIVE}<Intrinsic>false</Intrinsic></Component><MatchName>AE.ADBE Mosaic</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>{}{}<VideoComponentParam ObjectID=\"{}\" ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\" Version=\"9\">{name}<IsTimeVarying>false</IsTimeVarying><ParameterControlType>4</ParameterControlType><StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound><ParameterID>3</ParameterID></VideoComponentParam>",
        id + 1,
        id + 2,
        id + 3,
        count(id + 1, "Horizontal Blocks", 1, "10"),
        count(id + 2, "Vertical Blocks", 2, "10"),
        id + 3,
    )
}

/// A Mosaic with Sharp Colors on, static counts and the keys of its keyed
/// counts in native order (a keyed static value is its first key's).
fn mosaic_effect(
    enabled: bool,
    (horizontal, vertical): (u32, u32),
    animations: Vec<PrEffectParamAnimation>,
) -> PrEffect {
    PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Mosaic(PrMosaic {
            horizontal,
            vertical,
            sharp_colors: true,
        }),
        animations,
    }
}

/// Clip D's keys as read: Hold into the second and third keys.
fn mosaic_hold_keys(second: f64) -> Vec<PrScalarKeyframe> {
    let hold = |ticks: i64, value: f64| PrScalarKeyframe {
        easing: PrKeyframeEasing::Hold,
        ..key(ticks, value)
    };
    vec![
        key(TICKS, 10.0),
        hold(3 * TICKS / 2, second),
        hold(5 * TICKS / 2, 20.0),
    ]
}

#[test]
fn mosaics_read_static_keyed_and_bypassed_values_in_stack_order() {
    // Clips A, B, D and E of the run E8 fixture (verbatim keys on D), a corpus
    // Mosaic without and with the Gaussian Blur checkbox's `<Name> </Name>`,
    // and a bypassed one.
    let (occurrence, omissions) = read(&with_effects(&[
        (20, mosaic_26_5_xml(20, ("16", ""), ("9", ""), "true")),
        (30, mosaic_26_5_xml(30, ("48", ""), ("27", ""), "true")),
        (
            40,
            mosaic_26_5_xml(
                40,
                ("10", MOSAIC_HORIZONTAL_KEYS),
                ("10", MOSAIC_VERTICAL_KEYS),
                "true",
            ),
        ),
        (50, blur(50)),
        (60, mosaic_26_5_xml(60, ("96", ""), ("54", ""), "true")),
        (70, corpus_mosaic(70, "")),
        (80, corpus_mosaic(80, "<Name> </Name>")),
        (90, corpus_mosaic(90, "").replace(ACTIVE, BYPASSED)),
    ]));
    assert!(omissions.is_empty(), "{omissions:?}");
    // Premiere applies the chain in descending `Index`, so the
    // stack starts with the component written last.
    let expected = [
        mosaic_effect(false, (10, 10), vec![]),
        mosaic_effect(true, (10, 10), vec![]),
        mosaic_effect(true, (10, 10), vec![]),
        mosaic_effect(true, (96, 54), vec![]),
        gaussian_blur(true, 25.0, false),
        mosaic_effect(
            true,
            (10, 10),
            vec![
                PrEffectParamAnimation {
                    param: &MOSAIC_HORIZONTAL_BLOCKS,
                    keys: PrEffectParamKeys::Scalar(mosaic_hold_keys(40.0)),
                },
                PrEffectParamAnimation {
                    param: &MOSAIC_VERTICAL_BLOCKS,
                    keys: PrEffectParamKeys::Scalar(mosaic_hold_keys(30.0)),
                },
            ],
        ),
        mosaic_effect(true, (48, 27), vec![]),
        mosaic_effect(true, (16, 9), vec![]),
    ];
    assert_eq!(occurrence.effects, expected);
}

#[test]
fn unconvertible_mosaics_are_omitted_with_a_reason() {
    let prefix = "active effect \"Mosaic (Legacy)\" (match name \"AE.ADBE Mosaic\", VideoFilterComponent version 9, Component version 7) at stack position 1";
    let hold_reason = "; only Hold keys convert, because the FX mosaic renders fractional block counts between keys and Premiere's stepping there is unmeasured";
    let whole = |value: &str| {
        format!("{value} is not a whole number of blocks; Premiere counts whole blocks and no rounding is applied")
    };
    let mosaic = |horizontal: (&str, &str), vertical: (&str, &str), sharp: &str| {
        mosaic_26_5_xml(20, horizontal, vertical, sharp)
    };
    let default = ("10", "");
    // Clip D's keys with the second key's mode (the easing into the third)
    // Linear (0) or Bézier (5), or a fractional second value.
    let keys = |second: &str, mode: &str| {
        MOSAIC_HORIZONTAL_KEYS.replace(
            "381024000000,40,4,",
            &format!("381024000000,{second},{mode},"),
        )
    };
    #[rustfmt::skip]
    let records = [
        (mosaic(default, default, "false"), PrMosaic::SHARP_COLORS_OFF.to_owned()),
        (mosaic(("12.5", ""), default, "true"), format!("Horizontal Blocks {}", whole("12.5"))),
        (mosaic(default, ("0", ""), "true"), "Vertical Blocks \"0\" is not a number from 1 to 4000".to_owned()),
        (mosaic(("4001", ""), default, "true"), "Horizontal Blocks \"4001\" is not a number from 1 to 4000".to_owned()),
        (mosaic(("10", &keys("40", "0")), default, "true"), format!("Horizontal Blocks keys are Linear between source times 1.500 s and 2.500 s{hold_reason}")),
        (mosaic(("10", &keys("40", "5")), default, "true"), format!("Horizontal Blocks keys are Bézier between source times 1.500 s and 2.500 s{hold_reason}")),
        (mosaic(("10", &keys("40.5", "4")), default, "true"), format!("Horizontal Blocks key value {}", whole("40.5"))),
        (mosaic(("10", &keys("5000", "4")), default, "true"), "Horizontal Blocks key value 5000 is outside Premiere's 1 to 4000 range".to_owned()),
        (mosaic(default, default, "yes"), "invalid Sharp Colors value \"yes\"".to_owned()),
        (mosaic(default, default, "true").replace("<ParameterID>3</ParameterID><StartKeyframe>-91445760000000000,true", "<IsTimeVarying>true</IsTimeVarying><ParameterID>3</ParameterID><StartKeyframe>-91445760000000000,true"), "keyframed Sharp Colors is not supported; only static values convert".to_owned()),
        (mosaic(default, default, "true").replace("<ParameterID>3</ParameterID>", "<ParameterID>4</ParameterID>"), "unknown ParameterID 4".to_owned()),
    ];
    for (records, expected) in records {
        let reason = omitted_reason(records);
        assert!(
            reason.starts_with(prefix) && reason.ends_with(&expected),
            "{expected}: {reason}"
        );
    }
    // Premiere 26's default "Mosaic" is Film Impact's, an unknown effect.
    let film_impact = mosaic_26_5_xml(20, ("10", ""), ("10", ""), "true")
        .replace("AE.ADBE Mosaic", "AE.Impact_Mosaic_FX")
        .replace("Mosaic (Legacy)", "Mosaic");
    let reason = omitted_reason(film_impact);
    assert!(
        reason.starts_with("unknown active effect \"Mosaic\" (match name \"AE.Impact_Mosaic_FX\"")
            && reason.ends_with("no Tesseract effect mapping"),
        "{reason}"
    );
}

/// Hold Count keys in the form of the run E8 Mosaic count keys, the same
/// parameter class: 2 at source 1 s, 4 at 1.5 s and 3 at 2.5 s, every key
/// mode 4. Synthetic: no Premiere save of a keyed Replicate exists.
const REPLICATE_HOLD_KEYS: &str = "254016000000,2,4,0,0,0.16666666666666666,0,0.33333333333333331;381024000000,4,4,0,0,0.16666666666666666,0,0.33333333333333331;635040000000,3,4,0,0,0.16666666666666666,0,0.33333333333333331;";

/// The `AE.ADBE Replicate` of `feature_replicate_26_5_derived.prproj`
/// (`VideoFilterComponent:220` and its Count, `VideoComponentParam:410`),
/// verbatim but for its ObjectIDs `id` and `id + 1`, the static `count` and,
/// unless empty, the Count `keys`: as Premiere 26.5.1 saves its default, no
/// `Bypass` or `Intrinsic`, and the Count class `6e02e8bb` version 10 with
/// control type 1, bounds 2 to 16 and no UI bounds.
fn replicate_26_5_xml(id: u32, count: &str, keys: &str) -> String {
    let records = fixture_records("feature_replicate_26_5_derived.prproj", &["220", "410"]);
    assert_eq!(records.matches("ObjectID=").count(), 2, "{records}");
    let mut records = records
        .replace("ObjectID=\"220\"", &format!("ObjectID=\"{id}\""))
        .replace("ObjectID=\"410\"", &format!("ObjectID=\"{}\"", id + 1))
        .replace("ObjectRef=\"410\"", &format!("ObjectRef=\"{}\"", id + 1))
        .replace(
            "<StartKeyframe>-91445760000000000,2,",
            &format!("<StartKeyframe>-91445760000000000,{count},"),
        );
    if !keys.is_empty() {
        records = records
            .replace(
                "<ParameterID>1</ParameterID>",
                "<IsTimeVarying>true</IsTimeVarying><ParameterID>1</ParameterID>",
            )
            .replace(
                "<LowerBound>2</LowerBound>",
                &format!("<Keyframes>{keys}</Keyframes><LowerBound>2</LowerBound>"),
            );
    }
    records
}

/// A Replicate of a static `count`, or of keys that start at it.
fn replicate_effect(enabled: bool, count: u8, keys: Vec<PrScalarKeyframe>) -> PrEffect {
    PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Replicate(PrReplicate { count }),
        animations: if keys.is_empty() {
            Vec::new()
        } else {
            vec![PrEffectParamAnimation {
                param: &REPLICATE_COUNT,
                keys: PrEffectParamKeys::Scalar(keys),
            }]
        },
    }
}

/// [`REPLICATE_HOLD_KEYS`] as read: Hold into the second and third keys.
fn replicate_hold_keys() -> Vec<PrScalarKeyframe> {
    let hold = |ticks: i64, value: f64| PrScalarKeyframe {
        easing: PrKeyframeEasing::Hold,
        ..key(ticks, value)
    };
    vec![
        key(TICKS, 2.0),
        hold(3 * TICKS / 2, 4.0),
        hold(5 * TICKS / 2, 3.0),
    ]
}

#[test]
fn replicates_read_static_keyed_and_bypassed_counts_in_stack_order() {
    // The fixture's default Count 2, Count 16, Hold keys, a bypassed Count 3
    // and a Gaussian Blur written last. No Premiere save of a bypassed
    // Replicate exists: its `Bypass` and `Intrinsic` are the corpus effect
    // records'.
    let bypassed = replicate_26_5_xml(50, "3", "").replace(
        "<DisplayName>Replicate</DisplayName>",
        &format!("<DisplayName>Replicate</DisplayName>{BYPASSED}<Intrinsic>false</Intrinsic>"),
    );
    let (occurrence, omissions) = read(&with_effects(&[
        (20, replicate_26_5_xml(20, "2", "")),
        (30, replicate_26_5_xml(30, "16", "")),
        (40, replicate_26_5_xml(40, "2", REPLICATE_HOLD_KEYS)),
        (50, bypassed),
        (60, blur(60)),
    ]));
    assert!(omissions.is_empty(), "{omissions:?}");
    // The stack starts with the component written last.
    assert_eq!(
        occurrence.effects,
        [
            gaussian_blur(true, 25.0, false),
            replicate_effect(false, 3, vec![]),
            replicate_effect(true, 2, replicate_hold_keys()),
            replicate_effect(true, 16, vec![]),
            replicate_effect(true, 2, vec![]),
        ]
    );
}

#[test]
fn unconvertible_replicates_are_omitted_with_a_reason() {
    let prefix = "active effect \"Replicate\" (match name \"AE.ADBE Replicate\", VideoFilterComponent version 9, Component version 7) at stack position 1";
    let hold_rule = "; only Hold keys convert, because Premiere's Count between interpolated keys is unmeasured and the FX motionTile would interpolate the tile size and centre, reciprocals of the Count, linearly";
    let whole = " is not a whole number; Premiere counts whole copies and no rounding is applied";
    // The second key's mode (the easing into the third) Linear (0) or
    // Bézier (5), or another second value.
    let keys = |second: &str, mode: &str| {
        REPLICATE_HOLD_KEYS.replace(
            "381024000000,4,4,",
            &format!("381024000000,{second},{mode},"),
        )
    };
    let replicate = |count: &str, keys: &str| replicate_26_5_xml(20, count, keys);
    #[rustfmt::skip]
    let records = [
        (replicate("2.5", ""), format!("Count 2.5{whole}")),
        (replicate("1", ""), "Count \"1\" is not a number from 2 to 16".to_owned()),
        (replicate("17", ""), "Count \"17\" is not a number from 2 to 16".to_owned()),
        (replicate("2", &keys("4", "0")), format!("Count keys are Linear between source times 1.500 s and 2.500 s{hold_rule}")),
        (replicate("2", &keys("4", "5")), format!("Count keys are Bézier between source times 1.500 s and 2.500 s{hold_rule}")),
        (replicate("2", &keys("4.5", "4")), format!("Count key value 4.5{whole}")),
        (replicate("2", &keys("17", "4")), "Count key value 17 is outside Premiere's 2 to 16 range".to_owned()),
        (replicate("2", "").replace("<ParameterID>1</ParameterID>", "<ParameterID>2</ParameterID>"), "unknown ParameterID 2".to_owned()),
    ];
    for (records, expected) in records {
        let reason = omitted_reason(records);
        assert!(
            reason.starts_with(prefix) && reason.ends_with(&expected),
            "{expected}: {reason}"
        );
    }
}

/// Both original Noise effects on one owner, in native chain order.
fn fixture_noise() -> String {
    let legacy = fixture_records("noise-native-records.xml", &["549", "730", "731", "732"]);
    let modern_ids: Vec<_> = std::iter::once(550)
        .chain(733..=757)
        .map(|id| id.to_string())
        .collect();
    let modern_ids: Vec<_> = modern_ids.iter().map(String::as_str).collect();
    let modern = fixture_records("noise-native-records.xml", &modern_ids);
    // Original chain 388: Legacy Index 0, modern Index 1 (rendered first).
    with_effects(&[(549, legacy), (550, modern)])
}

#[test]
fn noise_native_siblings_keep_owner_and_import_editable_grain() {
    let xml = fixture_noise();
    let (clip, omissions) = read(&xml);
    assert_eq!(clip.effects.len(), 2);
    assert_eq!(
        clip.effects[0].params,
        PrEffectParams::ModernNoise {
            amount: 50.0,
            seed: 0.0
        }
    );
    assert_eq!(
        clip.effects[1].params,
        PrEffectParams::Noise { amount: 5.0 }
    );
    assert!(clip.effects[0].enabled);
    assert!(omissions.is_empty(), "{omissions:?}");
    let (project, _) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    let sequence = &project.sequences[0];
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    let mut warnings = Vec::new();
    let wire = crate::convert::premiere_to_tesseract(sequence, &project.media, &ids, &mut warnings)
        .unwrap()
        .to_json_value()
        .unwrap();
    assert_eq!(
        wire["composition"]["layers"][0]["effects"][0]["effect"],
        serde_json::json!({
            "type": "grain", "amount": 20.0, "size": 1.0, "softness": 0.0, "aspectRatio": 1.0, "seed": 0.0
        })
    );
    assert_eq!(
        wire["composition"]["layers"][0]["effects"][1]["effect"]["amount"],
        serde_json::json!(2.0)
    );
    assert!(warnings
        .iter()
        .any(|warning| warning.reason.contains("different random kernels")));
}

#[test]
fn noise_native_strength_targets_are_authorable_for_both_abis() {
    // Supplementary keys on unchanged native ABI records, not an own-writer round trip.
    let mut xml = fixture_noise();
    for (id, first, second) in [(737, 10, 30), (730, 5, 25)] {
        xml = noise_control(
            &xml,
            id,
            "<LowerBound>",
            &format!(
                "<Keyframes>0,{first}.,0,0,0,0,0,0;{TICKS},{second}.,4,0,0,0,0,0;</Keyframes><LowerBound>"
            ),
        );
    }
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = &project.sequences[0];
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    let wire =
        crate::convert::premiere_to_tesseract(sequence, &project.media, &ids, &mut Vec::new())
            .unwrap()
            .to_json_value()
            .unwrap();
    let effects = wire["composition"]["layers"][0]["effects"]
        .as_array()
        .unwrap();
    assert_eq!(effects.len(), 2);
    let entries = wire["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    for (effect, expected) in effects.iter().zip([[4., 12.], [2., 10.]]) {
        assert_eq!(effect["effect"]["type"], "grain");
        assert_eq!(effect["effect"]["amount"], serde_json::json!(expected[0]));
        assert!(effect["effect"].get("intensity").is_none());
        let tracks: Vec<_> = entries
            .iter()
            .filter(|entry| entry["target"]["effectId"] == effect["id"])
            .collect();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0]["target"]["paramName"], "intensity");
        let keys = tracks[0]["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(keys.len(), 2);
        for (index, value) in expected.into_iter().enumerate() {
            assert_eq!(keys[index]["layerTime"], serde_json::json!(index * 1000));
            assert_eq!(keys[index]["value"]["value"], serde_json::json!(value));
        }
    }
}

#[test]
fn noise_written_amount_keys_are_active_without_activating_static_controls() {
    // Derive the effect from unchanged human-authored Legacy records. Keys are
    // supplementary edits: Premiere 26.5.2's controlled native activation kept
    // these scalar tuples unchanged and added IsTimeVarying=true to Amount.
    let (clip, _) = read(&fixture_noise());
    for enabled in [true, false] {
        let mut noise = clip.effects[1].clone();
        noise.enabled = enabled;
        noise.params = PrEffectParams::Noise { amount: 0.0 };
        noise.animations = vec![PrEffectParamAnimation {
            param: &crate::schema::NOISE_AMOUNT,
            keys: PrEffectParamKeys::Scalar(vec![
                key(0, 0.0),
                PrScalarKeyframe {
                    easing: PrKeyframeEasing::Hold,
                    ..key(TICKS / 2, 10.0)
                },
                PrScalarKeyframe {
                    easing: PrKeyframeEasing::Hold,
                    ..key(TICKS, 30.0)
                },
            ]),
        }];
        let xml = project_xml(&project(vec![noise])).unwrap();
        let document = roxmltree::Document::parse(&xml).unwrap();
        let controls: Vec<_> = document
            .descendants()
            .filter(|node| {
                node.has_tag_name("VideoComponentParam")
                    && node.children().any(|child| {
                        child.has_tag_name("Name")
                            && matches!(
                                child.text(),
                                Some("Amount of Noise" | "Noise Type" | "Clipping")
                            )
                    })
            })
            .collect();
        assert_eq!(controls.len(), 3);
        for control in controls {
            let field = |name| {
                control
                    .children()
                    .find(|child| child.has_tag_name(name))
                    .and_then(|child| child.text())
            };
            let amount = field("Name") == Some("Amount of Noise");
            assert_eq!(
                field("IsTimeVarying"),
                Some(if amount { "true" } else { "false" })
            );
            if amount {
                assert_eq!(
                    field("Keyframes"),
                    Some(
                        "0,0,4,0,0,0,0,0;127008000000,10,4,0,0,0,0,0;254016000000,30,0,0,0,0,0,0;"
                    )
                );
            } else {
                assert!(field("Keyframes").is_none());
            }
        }
    }
}

#[test]
fn noise_monochrome_and_wrapping_retain_both_effects() {
    for name in ["Noise Type", "Clipping"] {
        let xml = fixture_noise();
        let marker = format!("<Name>{name}</Name>");
        let start = xml.find(&marker).unwrap();
        let (prefix, suffix) = xml.split_at(start);
        let changed = format!("{prefix}{}", suffix.replacen(",true,", ",false,", 1));
        let (clip, omissions) = read(&changed);
        assert_eq!(clip.effects.len(), 2);
        assert!(omissions.is_empty(), "{omissions:?}");
    }
}

/// Mutate exactly one parameter record, never the pinned source or a same-value sibling.
fn noise_control(xml: &str, id: u32, from: &str, to: &str) -> String {
    let start = xml
        .find(&format!("<VideoComponentParam ObjectID=\"{id}\""))
        .unwrap();
    let end = start + xml[start..].find("</VideoComponentParam>").unwrap();
    let record = &xml[start..end];
    assert_eq!(record.matches(from).count(), 1);
    format!(
        "{}{}{}",
        &xml[..start],
        record.replacen(from, to, 1),
        &xml[end..]
    )
}

#[test]
fn noise_modern_exact_record_strength_seed_keys_and_malformed_controls() {
    let xml = fixture_noise();
    let edited = noise_control(&xml, 737, ",50.,", ",30.,");
    let edited = noise_control(&edited, 736, ",0.,", ",17.,");
    let (clip, omissions) = read(&edited);
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        clip.effects[0].params,
        PrEffectParams::ModernNoise {
            amount: 30.0,
            seed: 17.0
        }
    );
    assert_eq!(
        clip.effects[1].params,
        PrEffectParams::Noise { amount: 5.0 }
    );
    let keyed = noise_control(
        &xml,
        737,
        "<LowerBound>",
        &format!("<Keyframes>0,10.,0,0,0,0,0,0;{TICKS},30.,4,0,0,0,0,0;</Keyframes><LowerBound>"),
    );
    let (clip, omissions) = read(&keyed);
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(clip.effects[0].animations.len(), 1);
    let keys = clip.effects[0].animations[0].keys.scalar().unwrap();
    assert_eq!(
        keys.iter().map(|key| key.value).collect::<Vec<_>>(),
        [10.0, 30.0]
    );
    for (id, from, to) in [
        (737, ",50.,", ",NaN,"),
        (736, ",0.,", ",-1.,"),
        (743, ",4,", ",2.5,"),
        (741, ",true,", ",invalid,"),
    ] {
        let (clip, omissions) = read(&noise_control(&xml, id, from, to));
        assert_eq!(clip.effects.len(), 1, "{id}");
        assert_eq!(
            clip.effects[0].params,
            PrEffectParams::Noise { amount: 5.0 }
        );
        assert_eq!(omissions.len(), 1, "{id}: {omissions:?}");
    }
}

/// The `AE.ADBE Posterize` component and Level record `ids` of the
/// `feature_posterize_strict` fixture, verbatim as Premiere 26.5.1 saved
/// them: no `Bypass` or `Intrinsic`, the Level class `a4ff2d6e` version 10
/// without a control type. A is 118/147 (Level 2), B 121/150 (7), C 124/153
/// (4), D 127/156 (Hold keys 3, 8 and 5 at source 1, 1.5 and 2.5 s over the
/// `StartKeyframe` 7) and E 130/159 (16).
fn fixture_posterize(ids: [&str; 2]) -> String {
    fixture_records("feature_posterize_strict.prproj", &ids)
}

/// A Posterize with a whole `level` and the keys of its keyed Level.
fn posterize_effect(enabled: bool, level: u8, animations: Vec<PrEffectParamAnimation>) -> PrEffect {
    PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Posterize(PrPosterize { level }),
        animations,
    }
}

/// Clip D's Level keys as read: Hold into the second and third keys.
fn posterize_hold_keys() -> Vec<PrEffectParamAnimation> {
    let hold = |ticks: i64, value: f64| PrScalarKeyframe {
        easing: PrKeyframeEasing::Hold,
        ..key(ticks, value)
    };
    vec![PrEffectParamAnimation {
        param: &POSTERIZE_LEVEL,
        keys: PrEffectParamKeys::Scalar(vec![
            key(TICKS, 3.0),
            hold(3 * TICKS / 2, 8.0),
            hold(5 * TICKS / 2, 5.0),
        ]),
    }]
}

#[test]
fn posterizes_read_static_keyed_and_bypassed_values_in_stack_order() {
    // Clips A to E of the fixture, verbatim, with B bypassed and a Gaussian
    // Blur between B and C.
    let bypassed = fixture_posterize(["121", "150"]).replace(
        "<DisplayName>Posterize</DisplayName>",
        "<DisplayName>Posterize</DisplayName><Bypass>true</Bypass>",
    );
    let (occurrence, omissions) = read(&with_effects(&[
        (118, fixture_posterize(["118", "147"])),
        (121, bypassed),
        (20, blur(20)),
        (124, fixture_posterize(["124", "153"])),
        (127, fixture_posterize(["127", "156"])),
        (130, fixture_posterize(["130", "159"])),
    ]));
    assert!(omissions.is_empty(), "{omissions:?}");
    // Premiere applies the chain in descending `Index`, so the stack starts
    // with the component written last. D's static Level is its first key's,
    // which Premiere renders before that key, not its `StartKeyframe`.
    assert_eq!(
        occurrence.effects,
        [
            posterize_effect(true, 16, vec![]),
            posterize_effect(true, 3, posterize_hold_keys()),
            posterize_effect(true, 4, vec![]),
            gaussian_blur(true, 25.0, false),
            posterize_effect(false, 7, vec![]),
            posterize_effect(true, 2, vec![]),
        ]
    );
}

#[test]
fn unconvertible_posterizes_are_omitted_with_a_reason() {
    let prefix = "active effect \"Posterize\" (match name \"AE.ADBE Posterize\", VideoFilterComponent version 9, Component version 7) at stack position 1";
    let whole = " is not a whole number; Premiere's rendering of a fractional Level is unmeasured and no rounding is applied";
    let hold_rule = "; only Hold keys convert, because the FX posterize floors the levels between keys and Premiere's stepping there is unmeasured";
    // Clip A's (static Level 2) or D's (keyed) records with one edit, the
    // component renumbered to the chain's ObjectID 20.
    let edited = |ids: [&str; 2], from: &str, to: &str| {
        let records = fixture_posterize(ids);
        let edited = records.replace(from, to);
        assert_ne!(edited, records, "{from}");
        edited.replace(&format!("ObjectID=\"{}\"", ids[0]), "ObjectID=\"20\"")
    };
    let (a, d) = (["118", "147"], ["127", "156"]);
    // D's first key's mode is the easing into its second key, and the
    // second key's the easing into the third.
    #[rustfmt::skip]
    let records = [
        (edited(a, ",2.,", ",2.5,"), format!("Level 2.5{whole}")),
        (edited(a, ",2.,", ",1.,"), "Level \"1.\" is not a number from 2 to 255".to_owned()),
        (edited(a, "<ParameterID>1</ParameterID>", "<ParameterID>2</ParameterID>"), "unknown ParameterID 2".to_owned()),
        (edited(d, "254016000000,3.,4,", "254016000000,3.,0,"), format!("Level keys are Linear between source times 1.000 s and 1.500 s{hold_rule}")),
        (edited(d, "381024000000,8.,4,", "381024000000,8.,5,"), format!("Level keys are Bézier between source times 1.500 s and 2.500 s{hold_rule}")),
        (edited(d, "381024000000,8.,", "381024000000,8.5,"), format!("Level key value 8.5{whole}")),
        (edited(d, "381024000000,8.,", "381024000000,300.,"), "Level key value 300 is outside Premiere's 2 to 255 range".to_owned()),
    ];
    for (records, expected) in records {
        let reason = omitted_reason(records);
        assert!(
            reason.starts_with(prefix) && reason.ends_with(&expected),
            "{expected}: {reason}"
        );
    }
    // Posterize Time, a different effect, is identified by its own match
    // name and rejects the mismatched Posterize Level layout.
    let posterize_time = edited(a, "AE.ADBE Posterize", "AE.ADBE Posterize Time")
        .replace("<DisplayName>Posterize", "<DisplayName>Posterize Time");
    let reason = omitted_reason(posterize_time);
    assert!(
        reason
            .starts_with("active effect \"Posterize Time\" (match name \"AE.ADBE Posterize Time\"")
            && reason.ends_with("unsupported Posterize Time Frame Rate layout"),
        "{reason}"
    );
}

#[test]
fn an_unconvertible_posterize_keeps_its_clip_and_the_other_effects() {
    // Clip D with Linear keys between a Gaussian Blur and clip E's Posterize:
    // only D's Posterize is omitted.
    let linear =
        fixture_posterize(["127", "156"]).replace("254016000000,3.,4,", "254016000000,3.,0,");
    let (occurrence, omissions) = read(&with_effects(&[
        (20, blur(20)),
        (127, linear),
        (130, fixture_posterize(["130", "159"])),
    ]));
    assert_eq!(
        occurrence.effects,
        [
            posterize_effect(true, 16, vec![]),
            gaussian_blur(true, 25.0, false)
        ]
    );
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert_eq!(omissions[0].scope, OmissionScope::Feature);
    assert_eq!(omissions[0].record, "VideoFilterComponent:127");
    assert!(
        omissions[0]
            .reason
            .contains("at stack position 2 on clip \"Source\"")
            && omissions[0]
                .reason
                .contains("Level keys are Linear between source times 1.000 s and 1.500 s"),
        "{}",
        omissions[0].reason
    );
}

/// The `AE.ADBE Geometry` records of the run E11 fixture
/// (`feature_transform_strict`), verbatim: clip B (`VideoFilterComponent:137`:
/// Anchor Point 0.75:0.5, Uniform Scale on with Scale Height 50 and Scale
/// Width 100, Rotation 30), C (140: Skew 30, Skew Axis 45), F (143: Position
/// keyed with the shutter checkbox off and Shutter Angle 180), D (146: Scale
/// Height and Rotation keyed from source In 0.5 s), E (150: Position 0.75:0.5,
/// Uniform Scale 150) and A (134: Position 0.75:0.5, Opacity 50).
fn fixture_transform(component: u32, first_param: u32) -> String {
    let ids: Vec<String> = std::iter::once(component)
        .chain(first_param..first_param + 12)
        .map(|id| id.to_string())
        .collect();
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let records = fixture_records("feature_transform_strict.prproj", &ids);
    assert_eq!(records.matches("ObjectID=").count(), 13, "{component}");
    records
}

/// The static values of a Transform in native `Params` order, as `x:y` points,
/// `true`/`false` checkboxes and numbers, each with its keys (`""` for none).
pub(super) type TransformXml<'a> = [(&'a str, &'a str); 12];

/// A Transform at Premiere's defaults: the composition's shutter angle and
/// bilinear sampling.
pub(super) const DEFAULT_TRANSFORM: TransformXml<'static> = [
    ("0.5:0.5", ""),
    ("0.5:0.5", ""),
    ("false", ""),
    ("100.", ""),
    ("100.", ""),
    ("0.", ""),
    ("0.", ""),
    ("0.", ""),
    ("100.", ""),
    ("true", ""),
    ("0.", ""),
    ("0", ""),
];

/// An `AE.ADBE Geometry` component and its 12 parameter records as Premiere
/// 26.5.1 saves them (`A-static.xml`, verbatim apart from the
/// ids and values): no `Bypass` or `Intrinsic`; version-10 scalars with the
/// 26.5.1 bounds and no control type but the angles'; version-4 points; the
/// two checkboxes **without a `Name`**; Sampling with
/// `DiscontinuousInterpolate`. A keyed parameter carries `IsTimeVarying`.
/// Records use ObjectIDs `id..id + 13`.
pub(super) fn transform_26_5_xml(id: u32, values: TransformXml<'_>) -> String {
    let keyed = |keys: &str| {
        if keys.is_empty() {
            (String::new(), String::new())
        } else {
            (
                "<IsTimeVarying>true</IsTimeVarying>".to_owned(),
                format!("<Keyframes>{keys}</Keyframes>"),
            )
        }
    };
    let point = |object: u32, name: &str, parameter_id: u32, (value, keys): (&str, &str)| {
        let (time_varying, keyframes) = keyed(keys);
        format!(
            "<PointComponentParam ObjectID=\"{object}\" ClassID=\"ca81d347-309b-44d2-acc7-1c572efb973c\" Version=\"4\"><Name>{name}</Name>{time_varying}<ParameterID>{parameter_id}</ParameterID><StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe>{keyframes}</PointComponentParam>"
        )
    };
    let checkbox = |object: u32, parameter_id: u32, (value, _): (&str, &str)| {
        format!(
            "<VideoComponentParam ObjectID=\"{object}\" ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\" Version=\"10\"><ParameterID>{parameter_id}</ParameterID><StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0</StartKeyframe></VideoComponentParam>"
        )
    };
    let scalar = |object: u32,
                  name: &str,
                  parameter_id: u32,
                  control: &str,
                  bounds: [&str; 2],
                  ui: &str,
                  (value, keys): (&str, &str)| {
        let (time_varying, keyframes) = keyed(keys);
        let (lower_ui, upper_ui) = if ui.is_empty() {
            (String::new(), String::new())
        } else {
            (
                format!("<LowerUIBound>-{ui}</LowerUIBound>"),
                format!("<UpperUIBound>{ui}</UpperUIBound>"),
            )
        };
        let control = if control.is_empty() {
            String::new()
        } else {
            format!("<ParameterControlType>{control}</ParameterControlType>")
        };
        format!(
            "<VideoComponentParam ObjectID=\"{object}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"10\"><Name>{name}</Name>{lower_ui}{control}{time_varying}<ParameterID>{parameter_id}</ParameterID>{upper_ui}<StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0</StartKeyframe>{keyframes}<LowerBound>{}</LowerBound><UpperBound>{}</UpperBound></VideoComponentParam>",
            bounds[0], bounds[1]
        )
    };
    let params: String = (0..12)
        .map(|index| {
            format!(
                "<Param Index=\"{index}\" ObjectRef=\"{}\"/>",
                id + 1 + index
            )
        })
        .collect();
    let scale = ["-30000", "30000"];
    let angle = ["-32768", "32767"];
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"><Component Version=\"7\"><Params Version=\"1\">{params}</Params><ID>3</ID><DisplayName>Transform</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Geometry</MatchName></VideoFilterComponent>{}{}{}{}{}{}{}{}{}{}{}<VideoComponentParam ObjectID=\"{}\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"10\"><Name>Sampling</Name><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterID>12</ParameterID><StartKeyframe>-91445760000000000,{},0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>1</UpperBound></VideoComponentParam>",
        point(id + 1, "Anchor Point", 1, values[0]),
        point(id + 2, "Position", 2, values[1]),
        checkbox(id + 3, 11, values[2]),
        scalar(id + 4, "Scale Height", 3, "", scale, "200", values[3]),
        scalar(id + 5, "Scale Width", 4, "", scale, "200", values[4]),
        scalar(id + 6, "Skew", 5, "", ["-70", "70"], "", values[5]),
        scalar(id + 7, "Skew Axis", 6, "3", angle, "", values[6]),
        scalar(id + 8, "Rotation", 7, "3", angle, "", values[7]),
        scalar(id + 9, "Opacity", 8, "", ["0", "100"], "", values[8]),
        checkbox(id + 10, 9, values[9]),
        scalar(id + 11, "Shutter Angle", 10, "", ["0", "360"], "", values[10]),
        id + 12,
        values[11].0,
    )
}

/// A corpus-generation Transform, as `vhs_slideshow` (Premiere 12.1, 7/5)
/// and `vhsvertical` (14.4, 8/6) save one: `Bypass` and `Intrinsic`,
/// version-9 scalars with control types 2 (3 for the angles) and the older
/// ±300 scale bounds, version-3 points with control type 6, the checkboxes
/// with control type 4 and bounds but **no `Name`**, and `IsTimeVarying`
/// on every static parameter. `name` is a checkbox `Name` element to try.
/// Records use ObjectIDs `id..id + 13`.
fn corpus_transform(id: u32, name: &str) -> String {
    let point = |object: u32, name: &str, parameter_id: u32, value: &str| {
        format!(
            "<PointComponentParam ObjectID=\"{object}\" ClassID=\"ca81d347-309b-44d2-acc7-1c572efb973c\" Version=\"3\"><Name>{name}</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>6</ParameterControlType><StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe><ParameterID>{parameter_id}</ParameterID></PointComponentParam>"
        )
    };
    let checkbox = |object: u32, parameter_id: u32, value: &str| {
        format!(
            "<VideoComponentParam ObjectID=\"{object}\" ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\" Version=\"9\">{name}<IsTimeVarying>false</IsTimeVarying><ParameterControlType>4</ParameterControlType><StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound><ParameterID>{parameter_id}</ParameterID></VideoComponentParam>"
        )
    };
    let scalar = |object: u32,
                  name: &str,
                  parameter_id: u32,
                  control: &str,
                  bounds: [&str; 2],
                  ui: &str,
                  value: &str| {
        let ui = if ui.is_empty() {
            String::new()
        } else {
            format!("<LowerUIBound>-{ui}</LowerUIBound><UpperUIBound>{ui}</UpperUIBound>")
        };
        format!(
            "<VideoComponentParam ObjectID=\"{object}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"9\"><Name>{name}</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>{control}</ParameterControlType><StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0</StartKeyframe><LowerBound>{}</LowerBound><UpperBound>{}</UpperBound><ParameterID>{parameter_id}</ParameterID>{ui}</VideoComponentParam>",
            bounds[0], bounds[1]
        )
    };
    let params: String = (0..12)
        .map(|index| {
            format!(
                "<Param Index=\"{index}\" ObjectRef=\"{}\"/>",
                id + 1 + index
            )
        })
        .collect();
    let angle = ["-32768", "32767"];
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"7\"><Component Version=\"5\"><Node Version=\"1\"><Properties Version=\"1\"><ECP.Filter.Expanded>false</ECP.Filter.Expanded></Properties></Node><Params Version=\"1\">{params}</Params><ID>3</ID><DisplayName>Transform</DisplayName>{ACTIVE}<Intrinsic>false</Intrinsic></Component><MatchName>AE.ADBE Geometry</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>{}{}{}{}{}{}{}{}{}{}{}<VideoComponentParam ObjectID=\"{}\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"9\"><Name>Sampling</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>1</UpperBound><ParameterID>12</ParameterID></VideoComponentParam>",
        point(id + 1, "Anchor Point", 1, "0.5:0.5"),
        point(id + 2, "Position", 2, "0.5:0.5"),
        checkbox(id + 3, 11, "true"),
        scalar(id + 4, "Scale Height", 3, "2", ["-300", "300"], "200", "80."),
        scalar(id + 5, "Scale Width", 4, "2", ["-300", "300"], "200", "100."),
        scalar(id + 6, "Skew", 5, "2", ["-70", "70"], "", "0."),
        scalar(id + 7, "Skew Axis", 6, "3", angle, "", "0."),
        scalar(id + 8, "Rotation", 7, "3", angle, "", "0."),
        scalar(id + 9, "Opacity", 8, "2", ["0", "100"], "", "100."),
        checkbox(id + 10, 9, "true"),
        scalar(id + 11, "Shutter Angle", 10, "2", ["0", "360"], "", "0."),
        id + 12,
    )
}

/// Clip D's keys as read: Scale Height Linear into 200, Hold into 50;
/// Rotation Linear 0 to 90.
fn transform_d_animations() -> Vec<PrEffectParamAnimation> {
    vec![
        PrEffectParamAnimation {
            param: &TRANSFORM_SCALE_HEIGHT,
            keys: PrEffectParamKeys::Scalar(vec![
                key(TICKS, 100.0),
                key(3 * TICKS / 2, 200.0),
                PrScalarKeyframe {
                    easing: PrKeyframeEasing::Hold,
                    ..key(5 * TICKS / 2, 50.0)
                },
            ]),
        },
        PrEffectParamAnimation {
            param: &TRANSFORM_ROTATION,
            keys: PrEffectParamKeys::Scalar(vec![key(TICKS, 0.0), key(5 * TICKS / 2, 90.0)]),
        },
    ]
}

#[test]
fn transforms_read_static_and_keyed_values_in_stack_order() {
    // The run E11 fixture's six records verbatim, a corpus 7/5 Transform, a
    // 26.5.1 Transform whose Uniform Scale is off with unequal axes, and one
    // with keyed Opacity, the shutter checkbox off with a keyed Shutter
    // Angle and bicubic Sampling, around a Gaussian Blur.
    let (occurrence, omissions) = read(&with_effects(&[
        (137, fixture_transform(137, 187)),
        (140, fixture_transform(140, 201)),
        (50, blur(50)),
        (146, fixture_transform(146, 229)),
        (150, fixture_transform(150, 254)),
        (70, corpus_transform(70, "")),
        (
            90,
            transform_26_5_xml(90, {
                let mut values = DEFAULT_TRANSFORM;
                (values[3], values[4]) = (("100.", ""), ("70.", ""));
                values
            }),
        ),
        (134, fixture_transform(134, 173)),
        (143, fixture_transform(143, 215)),
        (
            110,
            transform_26_5_xml(110, {
                let mut values = DEFAULT_TRANSFORM;
                values[8] = ("100.", "254016000000,100.,0,0,0,0.16666666666666666,0,0.16666666666666666;381024000000,50.,0,0,0,0.16666666666666666,0,0.16666666666666666;");
                values[9] = ("false", "");
                values[10] = ("0.", "254016000000,90.,0,0,0,0.16666666666666666,0,0.16666666666666666;381024000000,180.,0,0,0,0.16666666666666666,0,0.16666666666666666;");
                values[11] = ("1", "");
                values
            }),
        ),
    ]));
    assert!(omissions.is_empty(), "{omissions:?}");
    // Premiere applies the chain in descending `Index`, so the
    // stack starts with the component written last.
    let uniform = PrTransform {
        uniform_scale: true,
        ..DEFAULT_PR_TRANSFORM
    };
    let expected = [
        // Keyed Opacity and Shutter Angle start at their first keys.
        transform_effect(
            PrTransform {
                composition_shutter_angle: false,
                shutter_angle: 90.0,
                bicubic_sampling: true,
                ..DEFAULT_PR_TRANSFORM
            },
            vec![
                PrEffectParamAnimation {
                    param: &TRANSFORM_OPACITY,
                    keys: PrEffectParamKeys::Scalar(vec![
                        key(TICKS, 100.0),
                        key(3 * TICKS / 2, 50.0),
                    ]),
                },
                PrEffectParamAnimation {
                    param: &TRANSFORM_SHUTTER_ANGLE,
                    keys: PrEffectParamKeys::Scalar(vec![
                        key(TICKS, 90.0),
                        key(3 * TICKS / 2, 180.0),
                    ]),
                },
            ],
        ),
        // F: Position keyed, the shutter checkbox off at 180 (T12).
        transform_effect(
            PrTransform {
                position: [0.25, 0.5],
                composition_shutter_angle: false,
                shutter_angle: 180.0,
                ..DEFAULT_PR_TRANSFORM
            },
            vec![PrEffectParamAnimation {
                param: &TRANSFORM_POSITION,
                keys: PrEffectParamKeys::Point(vec![
                    point_key(0, [0.25, 0.5], PrKeyframeEasing::Linear),
                    point_key(TICKS, [0.75, 0.5], PrKeyframeEasing::Linear),
                ]),
            }],
        ),
        // A: Position 0.75:0.5 and Opacity 50 (T6).
        transform_effect(
            PrTransform {
                position: [0.75, 0.5],
                opacity: 50.0,
                ..DEFAULT_PR_TRANSFORM
            },
            vec![],
        ),
        transform_effect(
            PrTransform {
                scale_width: 70.0,
                ..DEFAULT_PR_TRANSFORM
            },
            vec![],
        ),
        transform_effect(
            PrTransform {
                scale_height: 80.0,
                ..uniform
            },
            vec![],
        ),
        transform_effect(
            PrTransform {
                position: [0.75, 0.5],
                scale_height: 150.0,
                ..uniform
            },
            vec![],
        ),
        // D: a keyed parameter's static value is its first key's.
        transform_effect(uniform, transform_d_animations()),
        gaussian_blur(true, 25.0, false),
        transform_effect(
            PrTransform {
                skew: 30.0,
                skew_axis: 45.0,
                ..DEFAULT_PR_TRANSFORM
            },
            vec![],
        ),
        transform_effect(
            PrTransform {
                anchor_point: [0.75, 0.5],
                scale_height: 50.0,
                rotation: 30.0,
                ..uniform
            },
            vec![],
        ),
    ];
    assert_eq!(occurrence.effects, expected);
}

#[test]
fn numeric_parameter_id_keeps_transform_keys_when_label_is_renamed() {
    let mut values = DEFAULT_TRANSFORM;
    values[2] = ("true", "");
    let keys = format!("{TICKS},80.,0,0,0,0,0,0;{},120.,0,0,0,0,0,0;", 3 * TICKS);
    values[3] = ("100.", &keys);
    let records =
        transform_26_5_xml(20, values).replace("<Name>Scale Height</Name>", "<Name>Scale</Name>");

    let (occurrence, omissions) = read(&with_effects(&[(20, records)]));

    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        occurrence.effects,
        [transform_effect(
            PrTransform {
                uniform_scale: true,
                scale_height: 80.0,
                ..DEFAULT_PR_TRANSFORM
            },
            vec![PrEffectParamAnimation {
                param: &TRANSFORM_SCALE_HEIGHT,
                keys: PrEffectParamKeys::Scalar(vec![key(TICKS, 80.0), key(3 * TICKS, 120.0),]),
            }],
        )]
    );
}

#[test]
fn ordinary_physical_video_transform_keeps_curved_position_and_committed_warning() {
    let keys = format!(
        "0,0.25:0.5,0,0,0,0.16666666666666666,0.1,0.16666666666666666,5,4,0,0,0.05,0.04;{TICKS},0.5:0.25,0,0,0,0.16666666666666666,0.1,0.16666666666666666,5,4,-0.04,-0.03,0.03,0.05;{},0.75:0.5,0,0,0,0.16666666666666666,0.1,0.16666666666666666,5,4,-0.05,0.02,0,0;",
        2 * TICKS
    );
    let mut values = DEFAULT_TRANSFORM;
    values[1] = ("0.25:0.5", &keys);
    let xml = with_effects(&[(20, transform_26_5_xml(20, values))]);
    let (project, mut notes) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    let sequence = project.single_sequence().unwrap();
    let clip = sequence.video_tracks[0].clip(0);
    assert_eq!(clip.effects.len(), 1, "{notes:?}");

    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    let document =
        crate::convert::premiere_to_tesseract(sequence, &project.media, &ids, &mut notes)
            .unwrap()
            .to_json_value()
            .unwrap();
    let reports = notes
        .iter()
        .filter(|note| {
            note.kind == OmissionKind::Approximated
                && note.record == "VideoClipTrackItem:3"
                && note.reason
                    == "Transform Position curved spatial path retains editable tangents but FX traverses parametrically rather than native constant-speed distance"
        })
        .count();
    assert_eq!(reports, 1, "{notes:?}");

    let layers = document["composition"]["layers"].as_array().unwrap();
    let stage = layers
        .iter()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    let children = stage["layers"].as_array().unwrap();
    let videos = children
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .collect::<Vec<_>>();
    assert_eq!(videos.len(), 1, "physical source picture retained");
    let video = videos[0];

    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    for (property, values, incoming, outgoing) in [
        (
            "positionX",
            [480.0, 960.0, 1440.0],
            [None, Some(-76.8), Some(-96.0)],
            [Some(96.0), Some(57.6), None],
        ),
        (
            "positionY",
            [540.0, 270.0, 540.0],
            [None, Some(-32.4), Some(21.6)],
            [Some(43.2), Some(54.0), None],
        ),
    ] {
        let entry = entries
            .iter()
            .find(|entry| {
                entry["target"]["layerId"] == video["id"]
                    && entry["target"]["propertyType"] == property
            })
            .unwrap();
        let keys = entry["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(keys.len(), 3);
        for (index, key) in keys.iter().enumerate() {
            assert_eq!(key["layerTime"], i64::try_from(index).unwrap() * 1000);
            assert_eq!(key["value"]["value"], values[index]);
            for (actual, expected) in [
                (
                    key.get("spatialInTangent").and_then(|value| value.as_f64()),
                    incoming[index],
                ),
                (
                    key.get("spatialOutTangent")
                        .and_then(|value| value.as_f64()),
                    outgoing[index],
                ),
            ] {
                match (actual, expected) {
                    (Some(actual), Some(expected)) => {
                        assert!((actual - expected).abs() < 1e-9)
                    }
                    (None, None) => {}
                    pair => panic!("unpaired spatial tangent {pair:?}"),
                }
            }
        }
    }
}

#[test]
fn unconvertible_transforms_are_omitted_with_a_reason() {
    let prefix = "active effect \"Transform\" (match name \"AE.ADBE Geometry\", VideoFilterComponent version 9, Component version 7) at stack position 1";
    let edited = |edit: &dyn Fn(&mut TransformXml<'_>)| {
        let mut values = DEFAULT_TRANSFORM;
        edit(&mut values);
        transform_26_5_xml(20, values)
    };
    // Clip A's Opacity 50, clip F's shutter, bicubic Sampling, Width keys
    // under Uniform Scale and a skew with a Rotation convert approximately
    // (`PrTransform::approximations`).
    #[rustfmt::skip]
    let records = [
        (edited(&|values| values[0] = ("0.5:0.5", "0,0.25:0.5,0,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;254016000000,0.75:0.5,0,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;")), "keyframed Anchor Point is not supported; only static values convert".to_owned()),
        (edited(&|values| values[5] = ("30.", "254016000000,30.,0,0,0,0.16666666666666666,0,0.16666666666666666;381024000000,40.,0,0,0,0.16666666666666666,0,0.16666666666666666;")), PrTransform::KEYED_SKEW.to_owned()),
        (edited(&|values| values[11] = ("2", "")), "invalid Sampling value \"2\"".to_owned()),
        (edited(&|values| values[3] = ("30001.", "")), "Scale Height \"30001.\" is not a number from -30000 to 30000".to_owned()),
        (edited(&|values| values[2] = ("yes", "")), "invalid Uniform Scale value \"yes\"".to_owned()),
        (edited(&|_| {}).replace("<ParameterID>12</ParameterID>", "<ParameterID>13</ParameterID>"), "unknown ParameterID 13".to_owned()),
        (corpus_transform(20, "").replace(ACTIVE, BYPASSED), "a bypassed Transform is not converted: Premiere renders the clip without it, and only an active Transform becomes the staged video's transform".to_owned()),
    ];
    for (records, expected) in records {
        let id = 20;
        let (occurrence, omissions) = read(&with_effects(&[(id, records)]));
        assert!(occurrence.effects.is_empty(), "{:?}", occurrence.effects);
        assert_eq!(omissions.len(), 1, "{omissions:?}");
        assert_eq!(omissions[0].scope, OmissionScope::Feature);
        assert_eq!(omissions[0].record, format!("VideoFilterComponent:{id}"));
        let reason = &omissions[0].reason;
        assert!(
            (reason.starts_with(prefix) || expected.starts_with("a bypassed"))
                && reason.ends_with(&expected),
            "{expected}: {reason}"
        );
    }
    // Distinct rendered axis tracks cannot share one Corner Pin point track
    // without losing one axis's timing or easing.
    let mut values = DEFAULT_TRANSFORM;
    values[3] = ("100.", "0,100.,0,0,0,0,0,0;254016000000,80.,0,0,0,0,0,0;");
    values[4] = ("100.", "0,100.,0,0,0,0,0,0;254016000000,120.,0,0,0,0,0,0;");
    let reason = omitted_reason(
        transform_26_5_xml(20, values).replace("AE.ADBE Geometry", "AE.ADBE Geometry2"),
    );
    assert!(
        reason.contains("independently keyed Scale Height and Scale Width"),
        "{reason}"
    );
}

#[test]
fn transforms_beside_a_rejected_one_stage_nothing() {
    use crate::{convert, tesseract_output::asset_ids_in_order};
    let edited = |id, edit: &dyn Fn(&mut TransformXml<'_>)| {
        let mut values = DEFAULT_TRANSFORM;
        edit(&mut values);
        transform_26_5_xml(id, values)
    };
    // A convertible Rotation 30 beside a keyed Skew, in both chain orders,
    // and beside a bypassed corpus Transform.
    let rotated = |id| edited(id, &|values| values[7] = ("30.", ""));
    let keyed_skew = |id| {
        edited(id, &|values| {
            values[5] = ("30.", "254016000000,30.,0,0,0,0.16666666666666666,0,0.16666666666666666;381024000000,40.,0,0,0,0.16666666666666666,0,0.16666666666666666;")
        })
    };
    let bypassed = |id| corpus_transform(id, "").replace(ACTIVE, BYPASSED);
    let rule = "another active Transform on the same clip is not converted with this one: native measurements cover one Transform per clip, and Premiere's composition of two is unmeasured";
    // (chain, active Transforms, whether the clip stages, the omissions' record and reason end)
    let cases = [
        (
            [(20, rotated(20)), (40, keyed_skew(40))],
            2,
            false,
            vec![
                ("VideoFilterComponent:40", PrTransform::KEYED_SKEW),
                ("VideoClipTrackItem:3", rule),
            ],
        ),
        (
            [(20, keyed_skew(20)), (40, rotated(40))],
            2,
            false,
            vec![
                ("VideoFilterComponent:20", PrTransform::KEYED_SKEW),
                ("VideoClipTrackItem:3", rule),
            ],
        ),
        (
            [(20, rotated(20)), (40, bypassed(40))],
            1,
            true,
            vec![("VideoFilterComponent:40", "a bypassed Transform is not converted: Premiere renders the clip without it, and only an active Transform becomes the staged video's transform")],
        ),
    ];
    for (chain, active_transforms, stages, expected) in cases {
        let case = format!("{active_transforms} active, stages {stages}");
        let (project, mut omissions) =
            inspect_project_with_omissions(&with_effects(&chain), Some("sequence-1")).unwrap();
        let sequence = project.single_sequence().unwrap();
        let clip = sequence.video_tracks[0].clip(0);
        // The reader keeps the convertible Transform and the native count.
        assert_eq!(clip.active_transforms, active_transforms, "{case}");
        assert_eq!(clip.effects.len(), 1, "{case}: {:?}", clip.effects);
        let document = convert::premiere_to_tesseract(
            sequence,
            &project.media,
            &asset_ids_in_order(sequence, &project.media),
            &mut omissions,
        )
        .unwrap()
        .to_json_value()
        .unwrap();
        // Staged: a group holds the video; otherwise the clip stays a flat
        // video layer over the canvas without a Transform.
        let layers = document["composition"]["layers"].as_array().unwrap();
        let groups = layers
            .iter()
            .filter(|layer| layer["type"] == "Group")
            .count();
        let videos: Vec<_> = layers
            .iter()
            .filter(|layer| layer["type"] == "Video")
            .map(|layer| layer["transform"]["rotation"].clone())
            .collect();
        if stages {
            assert_eq!((groups, videos.len()), (1, 0), "{case}: {layers:?}");
        } else {
            assert_eq!(groups, 0, "{case}: {layers:?}");
            assert_eq!(videos, [serde_json::json!(0.0)], "{case}");
        }
        let reported: Vec<_> = omissions
            .iter()
            .filter(|omission| omission.reason.contains("Transform"))
            .map(|omission| (omission.record.as_str(), omission.reason.as_str()))
            .collect();
        assert_eq!(reported.len(), expected.len(), "{case}: {reported:?}");
        for ((record, reason), (expected_record, end)) in reported.iter().zip(&expected) {
            assert!(
                record == expected_record && reason.ends_with(end),
                "{case}: {record} {reason}"
            );
        }
    }
}

#[test]
fn invert_reads_static_keyed_and_bypassed_values_in_stack_order() {
    // Clip C of the run E5 fixture: Blend 100 at source 1.5 s, Linear to 20
    // at 2 s, which holds from 3 s until 100 at 3.5 s; its StartKeyframe
    // keeps 0.
    let fixture_keys = "381024000000,100.,0,0,0,0.16666666666666666,-16,0.16666666666666666;508032000000,20.,0,0,-16,0.16666666666666666,0,0.16666666666666666;762048000000,20.,4,0,0,0.16666666666666666,0,0.33333333333333331;889056000000,100.,0,0,16,0.16666666666666666,0,0.16666666666666666;";
    // Clip B: an empty copy of the private data names A's blob, here one
    // that no record stores.
    let copy = invert_26_5_xml(30, "0", ("30.", "")).replace(
        INVERT_PRIVATE_DATA,
        "<PremiereFilterPrivateData Encoding=\"base64\" BinaryHash=\"a0f1dc8e-2dc1-f50b-a89c-43600000241c\"/>",
    );
    let (occurrence, omissions) = read(&with_effects(&[
        (20, invert_26_5_xml(20, "0", ("0.", ""))),
        (30, copy),
        (40, blur(40)),
        (50, invert_26_5_xml(50, "0", ("0.", fixture_keys))),
        // The corpus form (`horror_title`, Premiere 14.4) and it bypassed.
        (60, invert(60)),
        (70, invert(70).replace(ACTIVE, BYPASSED)),
    ]));
    assert!(omissions.is_empty(), "{omissions:?}");
    // Premiere applies the chain in descending `Index`, so the
    // stack starts with the component written last. Private data of any form
    // is ignored, a keyed Blend starts at its first key, and the Hold out of
    // the 3 s key is the easing into the 3.5 s key, as for Motion keys.
    let hold = PrScalarKeyframe {
        easing: PrKeyframeEasing::Hold,
        ..key(7 * TICKS / 2, 100.0)
    };
    #[rustfmt::skip]
    let expected = [
        invert_effect(false, 0.0, vec![]),
        invert_effect(true, 0.0, vec![]),
        invert_effect(true, 100.0, vec![key(3 * TICKS / 2, 100.0), key(2 * TICKS, 20.0), key(3 * TICKS, 20.0), hold]),
        gaussian_blur(true, 25.0, false),
        invert_effect(true, 30.0, vec![]),
        invert_effect(true, 0.0, vec![]),
    ];
    assert_eq!(occurrence.effects, expected);
}

#[test]
fn unconvertible_inverts_are_omitted_with_a_reason() {
    let base = invert_26_5_xml(20, "0", ("30.", ""));
    // A third parameter, which no Invert record has.
    let third = base.replace("<Param Index=\"1\" ObjectRef=\"22\"/>", "<Param Index=\"1\" ObjectRef=\"22\"/><Param Index=\"2\" ObjectRef=\"23\"/>")
        + "<VideoComponentParam ObjectID=\"23\" ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\" Version=\"10\"><Name>Clip Result</Name><ParameterID>3</ParameterID><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound></VideoComponentParam>";
    #[rustfmt::skip]
    let records = [
        // FX levels has no channel selection: only RGB converts.
        (invert_26_5_xml(20, "1", ("30.", "")), "Channel \"1\" is not RGB (0); FX levels inverts every channel"),
        (base.replace("<ParameterID>1</ParameterID>", "<IsTimeVarying>true</IsTimeVarying><ParameterID>1</ParameterID>").replace("</StartKeyframe><LowerBound>0</LowerBound><UpperBound>15", "</StartKeyframe><Keyframes>381024000000,1,4,0,0,0,0,0;</Keyframes><LowerBound>0</LowerBound><UpperBound>15"), "keyframed Channel is not supported; only static values convert"),
        (invert_26_5_xml(20, "0", ("150.", "")), "Blend With Original \"150.\" is not a number from 0 to 100"),
        (invert_26_5_xml(20, "0", ("0.", "381024000000,150.,0,0,0,0.16666666666666666,0,0.16666666666666666;")), "Blend With Original key value 150 is outside Premiere's 0 to 100 range"),
        (third, "expected 2 parameters, found 3"),
        (base.replace("<ParameterID>2</ParameterID>", "<ParameterID>3</ParameterID>"), "unknown ParameterID 3"),
    ];
    for (changed, expected) in records {
        assert_ne!(changed, base, "{expected}");
        let reason = omitted_reason(changed);
        assert!(
            reason.starts_with("active effect \"Invert\" (match name \"AE.ADBE Invert\", VideoFilterComponent version 9, Component version 7) at stack position 1")
                && reason.ends_with(expected),
            "{expected}: {reason}"
        );
    }
}

#[test]
fn ambiguous_component_order_is_not_guessed() {
    let xml = with_effects(&[(20, blur(20)), (30, blur(30))])
        .replace(
            "<Component Index=\"0\" ObjectRef=\"20\"/>",
            "<Component Index=\"1\" ObjectRef=\"20\"/>",
        )
        .replace(
            "<Component Index=\"1\" ObjectRef=\"30\"/>",
            "<Component Index=\"0\" ObjectRef=\"30\"/>",
        );
    let (occurrence, omissions) = read(&xml);
    assert!(occurrence.effects.is_empty());
    assert_eq!(omissions.len(), 2, "{omissions:?}");
    assert!(omissions
        .iter()
        .all(|omission| omission.reason.contains("stack order is ambiguous")));
}

#[test]
fn sequence_level_standard_effects_still_reject_the_sequence() {
    let xml = SOURCE
        .replace(
            "<VideoComponentChain ObjectID=\"2\"><ComponentChain/></VideoComponentChain>",
            "<VideoComponentChain ObjectID=\"2\"><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"20\"/></Components></ComponentChain></VideoComponentChain>",
        )
        .replace("</PremiereData>", &format!("{}</PremiereData>", blur(20)));
    let error = inspect_project_with_omissions(&xml, Some("sequence-1"))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("sequence-level video components are unsupported"),
        "{error}"
    );
}

#[test]
fn default_motion_chain_with_a_standard_effect_converts() {
    // 134 corpus chains have this shape; the base reader omitted them all.
    let (occurrence, omissions) = read(&with_chain(SOURCE, DEFAULT_FLAGS, &[(20, blur(20))]));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(occurrence.effects, [gaussian_blur(true, 25.0, false)]);
}

#[test]
fn malformed_intrinsic_components_next_to_effects_still_omit_the_occurrence() {
    let opacity = |id| intrinsic(id, 2, "Opacity", "AE.ADBE Opacity");
    let motion = |id| intrinsic(id, 1, "Motion", "AE.ADBE Motion");
    let vector_motion = |id| intrinsic(id, 3, "Vector Motion", "AE.ADBE Graphic Group");
    // Corpus chain shapes: 41, 35 and 12 chains respectively.
    for (flags, components, expected) in [
        (
            "<DefaultMotion>true</DefaultMotion>",
            vec![(20, opacity(20)), (30, blur(30))],
            "VideoFilterComponent:20: missing Opacity Params",
        ),
        (
            "",
            vec![(20, opacity(20)), (21, motion(21)), (30, blur(30))],
            "VideoFilterComponent:21: missing Motion Params",
        ),
        (
            DEFAULT_FLAGS,
            vec![(20, vector_motion(20)), (30, black_white_pr(30))],
            "VideoFilterComponent:20: unsupported video component Some(\"AE.ADBE Graphic Group\")",
        ),
    ] {
        let reason = omitted_occurrence_reason(flags, &components);
        assert!(reason.ends_with(expected), "{expected}: {reason}");
    }
}

#[test]
fn legacy_luma_key_native_controls_keep_editable_clip_and_siblings() {
    let records = fixture_records("legacy-luma-key.xml", &["543", "719", "720"]);
    for bypass in [false, true] {
        let records = if bypass {
            records.replace("<ID>3</ID>", "<ID>3</ID><Bypass>true</Bypass>")
        } else {
            records.clone()
        };
        let source = SOURCE
            .replace(
                "<InPoint>0</InPoint>",
                &format!("<InPoint>{TICKS}</InPoint>"),
            )
            .replace(
                "<OutPoint>1270080000000</OutPoint>",
                &format!("<OutPoint>{}</OutPoint>", 6 * TICKS),
            );
        let (occurrence, _) = read(&with_chain(
            &source,
            DEFAULT_FLAGS,
            &[(20, blur(20)), (543, records), (30, tint(30))],
        ));
        assert_eq!(occurrence.in_ticks, TICKS);
        assert_eq!(occurrence.effects.len(), 3);
        assert!(matches!(
            occurrence.effects[0].params,
            PrEffectParams::Tint(_)
        ));
        assert_eq!(occurrence.effects[1].enabled, !bypass);
        assert_eq!(occurrence.effects[1].animations.len(), 1);
        let keys = occurrence.effects[1].animations[0].keys.scalar().unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!((keys[0].source_ticks, keys[0].value), (0, 40.0));
        assert_eq!((keys[1].source_ticks, keys[1].value), (612809129304, 70.0));
        assert_eq!(keys[0].easing, PrKeyframeEasing::Linear);
        assert_eq!(occurrence.effects[2], gaussian_blur(true, 25.0, false));
    }
}

#[test]
fn legacy_luma_key_invalid_active_controls_are_contextual_and_bypass_keeps_siblings() {
    let native = fixture_records("legacy-luma-key.xml", &["543", "719", "720"]);
    for varied in [
        native.replace("<ID>3</ID>", "<ID>3</ID><Bypass>maybe</Bypass>"),
        native.replace(",70.,", ",101.,"),
    ] {
        let reason = omitted_occurrence_reason(DEFAULT_FLAGS, &[(543, varied)]);
        assert!(
            reason.contains("AE.ADBE Legacy Key Luma")
                && reason.contains("stack position 1")
                && reason.contains("the clip is not converted without it"),
            "{reason}"
        );
    }
    let ordered = with_chain(
        &with_second_clip(SOURCE),
        DEFAULT_FLAGS,
        &[(543, native.clone())],
    );
    let disordered = ordered.replace(
        "<Component Index=\"0\" ObjectRef=\"543\"/>",
        "<Component Index=\"2\" ObjectRef=\"543\"/>",
    );
    assert_ne!(ordered, disordered);
    let (project, notes) = inspect_project_with_omissions(&disordered, Some("sequence-1")).unwrap();
    assert_eq!(
        project.sequences[0]
            .video_occurrences()
            .map(|c| c.id.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("VideoClipTrackItem:9")]
    );
    assert!(notes.iter().any(|n| n.scope == OmissionScope::Occurrence
        && n.reason.contains("AE.ADBE Legacy Key Luma")
        && n.reason.contains("stack position 1")
        && n.reason.contains("unambiguous")
        && n.reason.contains("the clip is not converted without it")));
    let invalid_bypassed = native
        .replace("<ID>3</ID>", "<ID>3</ID><Bypass>true</Bypass>")
        .replace(",70.,", ",101.,");
    let (clip, notes) = read(&with_effects(&[(543, invalid_bypassed), (20, blur(20))]));
    assert_eq!(clip.effects, vec![gaussian_blur(true, 25.0, false)]);
    assert!(notes.iter().any(|n| n.reason.contains("bypassed")
        && n.reason.contains("stack position")
        && n.reason.contains("101")));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn legacy_luma_key_public_file_roundtrip_keeps_native_controls_editable() {
    let records = fixture_records("legacy-luma-key.xml", &["543", "719", "720"]);
    let xml = with_effects(&[(543, records)]);
    for wire in public_round_trip_checked(&xml, |notes| {
        assert!(!notes.is_empty());
        assert!(
            notes
                .iter()
                .all(|n| n.kind == crate::OmissionKind::Approximated
                    && n.reason.contains("Legacy Luma Key approximation")),
            "{notes:?}"
        );
    }) {
        let video = &wire["composition"]["layers"][0];
        assert_eq!(
            video["effects"][0]["effect"],
            serde_json::json!({"type":"lumaKey","threshold":0.4,"softness":0.2,"invert":0.0})
        );
        let tracks = &wire["composition"]["dynamics"]["entries"];
        assert_eq!(tracks.as_array().unwrap().len(), 1);
        assert_eq!(tracks[0]["target"]["paramName"], "threshold");
        assert_eq!(
            tracks[0]["animator"]["keyframes"][1]["value"],
            serde_json::json!({"type":"float","value":0.7})
        );
    }
}

#[test]
fn legacy_luma_key_native_same_value_bezier_endpoints_stay_editable() {
    let native = fixture_records("legacy-luma-key.xml", &["543", "719", "720"]);
    // Supplementary same-value Bezier: no velocity-to-FX cubic exists, but the
    // raw values and times remain valid and editable under the approximation.
    let native = native
        .replace(
            "0,40.,0,0,0,0.16666666666666666",
            "0,40.,5,0,0,0.16666666666666666",
        )
        .replace("612809129304,70.,0,0", "612809129304,40.,4,0");
    let (clip, notes) = read(&with_effects(&[(543, native)]));
    let keys = clip.effects[0].animations[0].keys.scalar().unwrap();
    assert_eq!(
        keys.iter()
            .map(|k| (k.source_ticks, k.value, k.easing))
            .collect::<Vec<_>>(),
        vec![
            (0, 40.0, PrKeyframeEasing::Linear),
            (612809129304, 40.0, PrKeyframeEasing::Linear)
        ]
    );
    assert!(notes
        .iter()
        .any(|n| n.reason.contains("Bezier keys approximated as Linear")
            && n.reason.contains("stack position 1")));
}

#[test]
fn keying_and_masked_effects_omit_their_occurrence() {
    for (components, expected) in [
        // A `Bypass` that is neither true nor false counts as active, so the
        // Track Matte Key reads as the clip's mask and rejects the flag.
        (
            vec![(20, track_matte_key(20).replace(ACTIVE, "<Bypass>maybe</Bypass>"))],
            "VideoFilterComponent:20: invalid Bypass \"maybe\"",
        ),
        (
            vec![(20, masked_tint(20))],
            "VideoFilterComponent:24: missing mask Params",
        ),
        // Hypothetical records: these match names are unobserved.
        (
            vec![(
                20,
                tint(20)
                    .replace("<DisplayName>Tint</DisplayName>", "<DisplayName>Ultra Key</DisplayName>")
                    .replace("AE.ADBE Tint", "AE.ADBE Unobserved Keyer"),
            )],
            "active effect \"Ultra Key\" (match name \"AE.ADBE Unobserved Keyer\", VideoFilterComponent version 7, Component version 5) at stack position 1: changes what the clip covers or its transparency; the clip is not converted without it",
        ),
        (
            vec![(
                20,
                track_matte_key(20)
                    .replace("<DisplayName>Track Matte Key</DisplayName>", "<DisplayName>Localized keyer</DisplayName>")
                    .replace("AE.ADBE Legacy Key Track Matte", "AE.ADBE Legacy Key Unobserved"),
            )],
            "active effect \"Localized keyer\" (match name \"AE.ADBE Legacy Key Unobserved\", VideoFilterComponent version 8, Component version 6) at stack position 1: changes what the clip covers or its transparency; the clip is not converted without it",
        ),
    ] {
        let reason = omitted_occurrence_reason(DEFAULT_FLAGS, &components);
        assert!(reason.contains(expected), "{expected}: {reason}");
    }
    // Premiere renders a clip without its bypassed effects, so a bypassed
    // keyer or masked effect keeps the clip and only the effect is reported:
    // an unmapped one as unknown, and a mapped one (the Tint) by its reader,
    // which rejects the incomplete mask record.
    for (records, expected) in [
        (
            track_matte_key(20).replace(ACTIVE, BYPASSED),
            "unknown bypassed effect \"Track Matte Key\"",
        ),
        (
            masked_tint(20).replacen(ACTIVE, BYPASSED, 1),
            "bypassed effect \"Tint\" (match name \"AE.ADBE Tint\", VideoFilterComponent version 7, Component version 5) at stack position 1 on clip \"Source\" (VideoClipTrackItem:3, V1, 0.000 s to 5.000 s): VideoFilterComponent:24: missing mask Params",
        ),
        (
            crop(20).replace(ACTIVE, BYPASSED),
            "unknown bypassed effect \"Crop\"",
        ),
    ] {
        let reason = omitted_reason(records);
        assert!(reason.starts_with(expected), "{reason}");
    }
    // A bypassed masked effect that has a mapping keeps its clip and is
    // omitted for its incomplete mask, not imported as a disabled effect
    // without that mask.
    let masked_blur = blur(20).replacen(ACTIVE, BYPASSED, 1).replace(
        "</Component><MatchName>AE.ADBE Gaussian Blur 2</MatchName>",
        "</Component><SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"24\"/></SubComponents><MatchName>AE.ADBE Gaussian Blur 2</MatchName>",
    ) + &mask(24);
    let reason = omitted_reason(masked_blur);
    assert!(
        reason.starts_with("bypassed effect \"Gaussian Blur\"")
            && reason.ends_with("VideoFilterComponent:24: missing mask Params"),
        "{reason}"
    );
    // A pixel effect keeps its clip and omits only itself; see
    // `unknown_active_effect_is_omitted_with_identity_clip_track_and_time`.
    let (occurrence, omissions) = read(&with_effects(&[(20, black_white_pr(20))]));
    assert_eq!(occurrence.id.as_deref(), Some("VideoClipTrackItem:3"));
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert_eq!(omissions[0].scope, OmissionScope::Feature);
}

/// The imported document of `xml`'s one sequence, which maps without omission.
fn import(xml: &str) -> serde_json::Value {
    let (project, _) = inspect_project_with_omissions(xml, Some("sequence-1")).unwrap();
    let sequence = &project.sequences[0];
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    let mut omissions = Vec::new();
    let document =
        crate::convert::premiere_to_tesseract(sequence, &project.media, &ids, &mut omissions)
            .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    document.to_json_value().unwrap()
}

/// The layer that carries the mask of an imported one-clip document, flat
/// (the video) or staged (its group), and the video under it.
fn masked_and_video(document: &serde_json::Value) -> (&serde_json::Value, &serde_json::Value) {
    let top = &document["composition"]["layers"][0];
    match top["type"].as_str() {
        Some("Video") => (top, top),
        Some("Group") => (top, &top["layers"][0]),
        other => panic!("unexpected top layer {other:?}"),
    }
}

#[test]
fn converted_effects_keep_their_side_of_a_crop_or_wipe() {
    let bypassed = |id| {
        blur(id)
            .replace(ACTIVE, BYPASSED)
            .replace(",false,0,0,0,0,0,0", ",true,0,0,0,0,0,0")
    };
    let keyed = |id| keyed_blur(id, CORPUS_KEYED_BLURRINESS);
    let pin = |id| {
        corner_pin(
            id,
            [
                ("0.1:0.05", ""),
                ("0.95:0", ""),
                ("0:1", ""),
                ("0.85:0.9", ""),
            ],
        )
    };
    let keyed_effect = with_blurriness_keys(
        gaussian_blur(true, 0.0, false),
        vec![key(914456685542400, 10.0), key(914495145479490, 0.0)],
    );
    let pin_effect = corner_pin_effect(
        true,
        [[0.1, 0.05], [0.95, 0.0], [0.0, 1.0], [0.85, 0.9]],
        Vec::new(),
    );
    let pin_fields = serde_json::json!({"type": "cornerPin",
        "upperLeftX": 0.1, "upperLeftY": 0.05, "upperRightX": 0.95, "upperRightY": 0.0,
        "lowerLeftX": 0.0, "lowerLeftY": 1.0, "lowerRightX": 0.85, "lowerRightY": 0.9});
    let blurriness = |value: f64| serde_json::json!({"blurriness": value});
    // (wipe, the effect's records, its model, fields of its FX effect): an
    // active blur, a bypassed one with Repeat Edge Pixels, a keyed Blurriness
    // and a Corner Pin beside the Crop; a static and a
    // keyed blur beside the keyed wipe.
    for (wipe, record, effect, fields) in [
        (
            false,
            blur as fn(u32) -> String,
            gaussian_blur(true, 25.0, false),
            blurriness(25.0),
        ),
        (
            false,
            bypassed,
            gaussian_blur(false, 25.0, true),
            blurriness(25.0),
        ),
        (false, keyed, keyed_effect.clone(), blurriness(10.0)),
        (false, pin, pin_effect, pin_fields),
        (
            true,
            blur,
            gaussian_blur(true, 25.0, false),
            blurriness(25.0),
        ),
        (true, keyed, keyed_effect, blurriness(10.0)),
    ] {
        let ((mask_id, mask), effect_id) = if wipe {
            ((154, adobe_linear_wipe()), 20)
        } else {
            ((20, top_crop(20)), 30)
        };
        // Premiere applies the chain in descending `Index`
        // (AME rendered clips A and C sharp, B and D soft). A mask at
        // Index 0 applies after the effect, which stages the clip; a mask at
        // the higher Index applies first, as FX applies a video's masks before
        // its effects.
        for (components, above_mask, masked_type) in [
            (
                vec![(mask_id, mask.clone()), (effect_id, record(effect_id))],
                1,
                "Group",
            ),
            (
                vec![(effect_id, record(effect_id)), (mask_id, mask.clone())],
                0,
                "Video",
            ),
        ] {
            let xml = with_effects(&components);
            let (clip, omissions) = read(&xml);
            assert!(omissions.is_empty(), "{omissions:?}");
            if wipe {
                let wipe = clip.linear_wipe.as_ref().unwrap();
                assert_eq!(
                    (wipe.angle_degrees, wipe.feather, wipe.completion.len()),
                    (270, 5.0, 2)
                );
            } else {
                assert_eq!(clip.crop.top, 15.0);
            }
            assert_eq!(clip.effects, std::slice::from_ref(&effect));
            assert_eq!(clip.effects_above_mask, above_mask);
            let document = import(&xml);
            let (masked, video) = masked_and_video(&document);
            assert_eq!(masked["type"], masked_type);
            assert_eq!(masked["masks"].as_array().unwrap().len(), 1);
            assert_eq!(video["effects"][0]["enabled"], effect.enabled);
            for (field, value) in fields.as_object().unwrap() {
                assert_eq!(&video["effects"][0]["effect"][field], value, "{field}");
            }
            // The effect's keys are its only tracks, beside the wipe guide's.
            let entries = document["composition"]["dynamics"]["entries"]
                .as_array()
                .map_or(&[][..], Vec::as_slice);
            let effect_tracks = entries
                .iter()
                .filter(|entry| entry["target"]["kind"] == "effectProperty")
                .count();
            assert_eq!(effect_tracks, effect.animations.len());
            assert_eq!(entries.len() - effect_tracks, usize::from(wipe));
            // The guide sits beside the layer that its mask is on.
            let guide_id = &masked["masks"][0]["layer"];
            let layers = match masked_type {
                "Video" => &document["composition"]["layers"],
                _ => &masked["layers"],
            };
            assert!(layers
                .as_array()
                .unwrap()
                .iter()
                .any(|layer| &layer["id"] == guide_id && layer["type"] == "Rect"));
        }
    }
}

#[test]
fn premiere_26_5_crop_and_linear_wipe_read_as_saved() {
    let keys = |keys: &[PrScalarKeyframe]| -> Vec<(i64, f64)> {
        keys.iter()
            .map(|key| (key.source_ticks, key.value))
            .collect()
    };
    // P0's first clip: its Crop alone, which stays on the video.
    let xml = with_effects(&[(20, crop_26_5(20))]);
    let (clip, omissions) = read(&xml);
    assert!(omissions.is_empty(), "{omissions:?}");
    let crop = clip.crop;
    assert_eq!(
        [
            crop.left,
            crop.top,
            crop.right,
            crop.bottom,
            crop.edge_feather
        ],
        [20.0, 15.0, 0.0, 10.0, 0.0]
    );
    assert_eq!(masked_and_video(&import(&xml)).0["type"], "Video");
    // Its second clip: the keyed wipe alone, at the default Motion.
    let xml = with_effects(&[(20, linear_wipe_26_5(20))]);
    let (clip, omissions) = read(&xml);
    assert!(omissions.is_empty(), "{omissions:?}");
    let wipe = clip.linear_wipe.as_ref().unwrap();
    assert_eq!(
        (wipe.initial_completion, wipe.angle_degrees, wipe.feather),
        (0.0, 90, 0.0)
    );
    assert_eq!(
        keys(&wipe.completion),
        [(TICKS / 2, 0.0), (3 * TICKS / 2, 60.0)]
    );
    assert_eq!(masked_and_video(&import(&xml)).0["type"], "Video");
    // Its third clip: the Crop at Index 0 and its blur at Index 1, which
    // Premiere applies first, stage the clip.
    let xml = with_effects(&[(20, crop_26_5(20)), (30, blur_26_5(30))]);
    let (clip, omissions) = read(&xml);
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(clip.crop.top, 15.0);
    assert_eq!(clip.effects, [gaussian_blur(true, 40.0, true)]);
    assert_eq!(clip.effects_above_mask, 1);
    assert_eq!(masked_and_video(&import(&xml)).0["type"], "Group");
}

#[test]
fn single_animated_crop_edges_use_the_matching_cardinal_reveal_anchor() {
    for (edge, angle, anchor, property) in [
        ("Left", 90, [1920.0, 0.0], "scaleX"),
        ("Top", 0, [0.0, 1080.0], "scaleY"),
        ("Right", 270, [0.0, 0.0], "scaleX"),
        ("Bottom", 180, [0.0, 0.0], "scaleY"),
    ] {
        let records = top_crop(20).replace(",15.,", ",0.,").replace(
            "<CurrentValue>15</CurrentValue>",
            "<CurrentValue>0</CurrentValue>",
        );
        let static_fields = format!(
            "<Name>{edge}</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>2</ParameterControlType><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe>"
        );
        let animated_fields = format!(
            "<Name>{edge}</Name><ParameterControlType>2</ParameterControlType><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><Keyframes>0,100.,0,0,0,0,0,0;254016000000,0.,0,0,0,0,0,0;</Keyframes>"
        );
        let records = records.replace(&static_fields, &animated_fields);
        assert_ne!(records, top_crop(20), "{edge}");
        let xml = with_second_clip(&with_effects(&[(20, records)]));

        let (clip, omissions) = read(&xml);
        assert!(omissions.is_empty(), "{edge}: {omissions:?}");
        assert!(clip.crop.is_default(), "{edge}");
        let wipe = clip.linear_wipe.as_ref().unwrap();
        assert_eq!(wipe.initial_completion, 100.0, "{edge}");
        assert_eq!(wipe.angle_degrees, angle, "{edge}");
        assert_eq!(
            wipe.completion
                .iter()
                .map(|key| (key.source_ticks, key.value))
                .collect::<Vec<_>>(),
            [(0, 100.0), (TICKS, 0.0)],
            "{edge}"
        );

        let document = import(&xml);
        let (masked, _) = masked_and_video(&document);
        let guide_id = &masked["masks"][0]["layer"];
        let layers = document["composition"]["layers"].as_array().unwrap();
        let guide = layers
            .iter()
            .find(|layer| &layer["id"] == guide_id)
            .unwrap();
        let videos: Vec<_> = layers
            .iter()
            .filter(|layer| layer["type"] == "Video")
            .collect();
        assert_eq!(videos.len(), 2, "{edge}");
        let sibling = videos
            .iter()
            .find(|layer| layer["sourceRange"]["start"] == 5000)
            .unwrap();
        assert!(
            sibling["masks"].as_array().is_none_or(Vec::is_empty),
            "{edge}"
        );
        assert_eq!(
            guide["transform"]["anchorPoint"],
            serde_json::json!(anchor),
            "{edge}"
        );
        assert_eq!(
            guide["transform"]["position"],
            serde_json::json!(anchor),
            "{edge}"
        );
        let entry = document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| &entry["target"]["layerId"] == guide_id)
            .unwrap();
        assert_eq!(entry["target"]["propertyType"], property, "{edge}");
        let keys = entry["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(keys[0]["value"]["value"], 0.0, "{edge}");
        assert_eq!(keys[1]["value"]["value"], 100.0, "{edge}");
    }
}

#[test]
fn empty_linear_wipe_completion_requires_a_constant_parameter() {
    let mut wipe = linear_wipe_26_5(20);
    let start = wipe.find("<Keyframes>").unwrap();
    let end = start + wipe[start..].find("</Keyframes>").unwrap() + "</Keyframes>".len();
    wipe.replace_range(start..end, "<Keyframes></Keyframes>");
    let reason = omitted_occurrence_reason(DEFAULT_FLAGS, &[(20, wipe.clone())]);
    assert!(
        reason.contains("empty time-varying Transition Completion"),
        "{reason}"
    );
    let constant = wipe.replace(
        "<IsTimeVarying>true</IsTimeVarying>",
        "<IsTimeVarying>false</IsTimeVarying>",
    );
    let (clip, omissions) = read(&with_effects(&[(20, constant)]));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert!(clip.linear_wipe.unwrap().completion.is_empty());
    let malformed = wipe.replace(
        "<IsTimeVarying>true</IsTimeVarying>",
        "<IsTimeVarying>maybe</IsTimeVarying>",
    );
    let reason = omitted_occurrence_reason(DEFAULT_FLAGS, &[(20, malformed)]);
    assert!(
        reason.contains("invalid Transition Completion IsTimeVarying"),
        "{reason}"
    );
}

#[test]
fn premiere_26_5_crop_and_linear_wipe_with_a_changed_value_fail_as_today() {
    // Each record is the P0 form with one change, and each fails with the
    // message that the same change gives a Premiere 26.3 record.
    let crop = crop_26_5(20);
    let feather = crop.find("<VideoComponentParam ObjectID=\"26\"").unwrap();
    let wipe = linear_wipe_26_5(20);
    let wipe_feather = wipe.find("<VideoComponentParam ObjectID=\"23\"").unwrap();
    let flags = "<Bypass>false</Bypass><Intrinsic>false</Intrinsic>";
    for (records, expected) in [
        // A missing parameter.
        (
            crop[..feather].replace("<Param Index=\"5\" ObjectRef=\"26\"/>", ""),
            "unsupported Crop parameter layout",
        ),
        // Another ClassID.
        (
            crop.replacen(
                "ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\"",
                "ClassID=\"a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542\"",
                1,
            ),
            "unexpected Crop parameter Left layout",
        ),
        // A keyed Crop edge beside other nonzero edges is not cardinal.
        (
            crop.replace(
                "<Name>Left</Name>",
                "<Name>Left</Name><IsTimeVarying>true</IsTimeVarying>",
            )
            .replace(
                "<StartKeyframe>-91445760000000000,20.,0,0,0,0,0,0</StartKeyframe>",
                "<StartKeyframe>-91445760000000000,20.,0,0,0,0,0,0</StartKeyframe><Keyframes>0,20.,0,0,0,0,0,0;254016000000,30.,0,0,0,0,0,0;</Keyframes>",
            ),
            "animated Crop requires one edge with every other edge and Edge Feather at zero",
        ),
        // Each layout is exact: the 26.5.1 parameters under the 26.3
        // component flags, and the 26.3 parameters without them.
        (
            crop.replace(
                "<DisplayName>Crop</DisplayName>",
                &format!("<DisplayName>Crop</DisplayName>{flags}"),
            ),
            "unexpected Crop parameter Left layout",
        ),
        (
            top_crop(20).replace(&format!("{ACTIVE}<Intrinsic>false</Intrinsic>"), ""),
            "unexpected Crop parameter Left layout",
        ),
        // The wipe: a missing parameter, the 26.3 name without the 26.3
        // flags, and the 26.5.1 name with them.
        (
            wipe[..wipe_feather].replace("<Param Index=\"2\" ObjectRef=\"23\"/>", ""),
            "unsupported Linear Wipe parameter layout",
        ),
        (
            wipe.replace("Linear Wipe (Legacy)", "Linear Wipe"),
            "unsupported Linear Wipe component",
        ),
        (
            wipe.replace(
                "<DisplayName>Linear Wipe (Legacy)</DisplayName>",
                &format!("<DisplayName>Linear Wipe (Legacy)</DisplayName>{flags}"),
            ),
            "unsupported Linear Wipe component",
        ),
    ] {
        let reason = omitted_occurrence_reason(DEFAULT_FLAGS, &[(20, records)]);
        assert!(reason.contains(expected), "{expected}: {reason}");
    }
}

#[test]
fn motion_crop_applies_after_every_standard_effect() {
    // The chain [Motion, blur 40, blur 25] applies as the stack [blur 25,
    // blur 40] and then Motion (descending `Index`; the native
    // reference has Motion at Index 0 applying after the effects), so a Motion
    // Crop follows both blurs and stages the clip with them on its video. At
    // Motion Crop 0 the clip keeps no mask and stays flat.
    for (left, crop_left, above_mask, top_type) in
        [("20.", 20.0, 2, "Group"), ("0.", 0.0, 0, "Video")]
    {
        let xml = with_chain(
            SOURCE,
            EXPLICIT_MOTION_FLAGS,
            &[
                (199, motion_26_5(left)),
                (30, blur_26_5(30)),
                (50, blur(50)),
            ],
        );
        let (clip, omissions) = read(&xml);
        assert!(omissions.is_empty(), "{omissions:?}");
        assert_eq!(clip.crop.left, crop_left);
        assert_eq!(
            clip.effects,
            [
                gaussian_blur(true, 25.0, false),
                gaussian_blur(true, 40.0, true)
            ]
        );
        assert_eq!(clip.effects_above_mask, above_mask);
        let document = import(&xml);
        let (masked, video) = masked_and_video(&document);
        assert_eq!(masked["type"], top_type);
        assert_eq!(
            masked["masks"].as_array().map_or(0, Vec::len),
            usize::from(above_mask > 0)
        );
        assert_eq!(video["effects"].as_array().unwrap().len(), 2);
    }
}

#[test]
fn motion_crop_beside_a_crop_effect_or_linear_wipe_omits_the_occurrence() {
    // Premiere applies the Crop effect at its stack position and the Motion
    // Crop with Motion; the clip keeps one mask, so neither is dropped.
    let reason = omitted_occurrence_reason(
        EXPLICIT_MOTION_FLAGS,
        &[(199, motion_26_5("20.")), (20, crop_26_5(20))],
    );
    assert!(
        reason.ends_with(
            "VideoFilterComponent:199: a Motion Crop beside an active Crop effect on one clip is not converted"
        ),
        "{reason}"
    );
    let crop_and_wipe = with_chain(
        &with_second_clip(SOURCE),
        EXPLICIT_MOTION_FLAGS,
        &[(199, motion_26_5("20.")), (154, adobe_linear_wipe())],
    );
    assert_eq!(
        read_first_clip_omitted(&crop_and_wipe),
        [mask_boundary_omission(
            "Crop and Linear Wipe on one clip are not converted"
        )]
    );
    // Beside Motion Crop 0 the Crop effect converts as before.
    let (clip, omissions) = read(&with_chain(
        SOURCE,
        EXPLICIT_MOTION_FLAGS,
        &[(199, motion_26_5("0.")), (20, crop_26_5(20))],
    ));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        [
            clip.crop.left,
            clip.crop.top,
            clip.crop.right,
            clip.crop.bottom
        ],
        [20.0, 15.0, 0.0, 10.0]
    );
}

#[test]
fn converted_effects_on_both_sides_of_a_crop_are_omitted() {
    let later = blur(40).replace(",25.,", ",40.,");
    let skew = [
        ("0.1:0.05", ""),
        ("0.95:0", ""),
        ("0:1", ""),
        ("0.85:0.9", ""),
    ];
    // The stacks [blur 40, Crop, blur 30] and [blur, Crop, Corner Pin], each
    // chain in descending `Index`. The clip and its Crop are kept, flat,
    // without effects.
    for (components, omitted) in [
        (
            vec![(30, blur(30)), (20, top_crop(20)), (40, later)],
            [("40", "Gaussian Blur"), ("30", "Gaussian Blur")],
        ),
        (
            vec![
                (20, corner_pin(20, skew)),
                (30, top_crop(30)),
                (40, blur(40)),
            ],
            [("40", "Gaussian Blur"), ("20", "Corner Pin")],
        ),
    ] {
        let (clip, omissions) = read(&with_effects(&components));
        assert_eq!(clip.crop.top, 15.0);
        assert!(clip.effects.is_empty());
        assert_eq!(clip.effects_above_mask, 0);
        assert_eq!(omissions.len(), 2, "{omissions:?}");
        for ((omission, (id, name)), position) in omissions.iter().zip(omitted).zip([1, 3]) {
            assert_eq!(
                (omission.scope, omission.record.as_str()),
                (
                    OmissionScope::Feature,
                    format!("VideoFilterComponent:{id}").as_str()
                )
            );
            assert!(
                omission
                    .reason
                    .starts_with(&format!("active effect \"{name}\" (match name"))
                    && omission
                        .reason
                        .contains(&format!("at stack position {position} on clip"))
                    && omission
                        .reason
                        .ends_with(crate::schema::MASK_EFFECT_ORDER_REASON),
                "{omission}"
            );
        }
    }
}

#[test]
fn omitted_effects_do_not_count_on_either_side_of_a_crop() {
    // The chain [unknown, Crop, blur] applies as the stack [blur, Crop,
    // unknown], staged; the chain [blur, Crop, unknown] applies as [unknown,
    // Crop, blur], flat. The unmapped effect is omitted where it stands.
    for (components, above_mask) in [
        (
            vec![(30, black_white_pr(30)), (20, top_crop(20)), (40, blur(40))],
            1,
        ),
        (
            vec![(40, blur(40)), (20, top_crop(20)), (30, black_white_pr(30))],
            0,
        ),
    ] {
        let (clip, omissions) = read(&with_effects(&components));
        assert_eq!(clip.effects, [gaussian_blur(true, 25.0, false)]);
        assert_eq!(clip.effects_above_mask, above_mask);
        assert_eq!(omissions.len(), 1, "{omissions:?}");
        assert_eq!(omissions[0].record, "VideoFilterComponent:30");
        assert!(
            omissions[0].reason.ends_with("no Tesseract effect mapping"),
            "{omissions:?}"
        );
    }
}

/// Reads a [`with_second_clip`] source, checks that only its second occurrence
/// converts, and returns the omissions.
fn read_first_clip_omitted(xml: &str) -> Vec<Omission> {
    let (project, omissions) = inspect_project_with_omissions(xml, Some("sequence-1")).unwrap();
    let kept: Vec<_> = project.sequences[0]
        .video_occurrences()
        .map(|clip| clip.id.as_deref())
        .collect();
    assert_eq!(kept, [Some("VideoClipTrackItem:9")]);
    omissions
}

/// The occurrence omission that `mask_boundary` gives the first clip.
fn mask_boundary_omission(reason: &str) -> Omission {
    Omission {
        scope: OmissionScope::Occurrence,
        kind: OmissionKind::Omitted,
        record: "VideoClipTrackItem:3".to_owned(),
        reason: format!("track 0, range 0..1270080000000 ticks: {reason}; occurrence omitted"),
    }
}

#[test]
fn masks_that_import_cannot_place_omit_the_occurrence() {
    let crop_and_wipe = with_chain(
        &with_second_clip(SOURCE),
        DEFAULT_FLAGS,
        &[(20, top_crop(20)), (154, adobe_linear_wipe())],
    );
    let small_media = with_chain(
        &with_second_clip(SOURCE),
        DEFAULT_FLAGS,
        &[(154, adobe_linear_wipe())],
    )
    .replace(
        "<FrameRect>0,0,1920,1080</FrameRect></VideoStream>",
        "<FrameRect>0,0,1280,720</FrameRect></VideoStream>",
    );
    for (xml, reason) in [
        (
            crop_and_wipe,
            "Crop and Linear Wipe on one clip are not converted",
        ),
        (
            small_media,
            "Linear Wipe on media that is not sequence-sized is not converted",
        ),
    ] {
        assert_eq!(
            read_first_clip_omitted(&xml),
            [mask_boundary_omission(reason)]
        );
    }
}

#[test]
fn crop_with_unmeasured_anchor_point_or_scale_height_keys_still_omits_the_occurrence() {
    // The static Motion of `feature_motion_static_transform_strict.prproj`
    // (Premiere 26.3, Scale Height 135 and Width 70), keyed in one parameter
    // outside the measured key forms: an Anchor Point key with Premiere's
    // automatic spatial mode, and Scale Height keys without Uniform Scale.
    let motion = fixture_records(
        "feature_motion_static_transform_strict.prproj",
        &["146", "147", "148", "149", "150", "151", "152", "153"],
    );
    let flags = "<DefaultOpacity>true</DefaultOpacity><DefaultOpacityComponentID>2</DefaultOpacityComponentID>";
    for (name, keys, expected) in [
        (
            "<Name>Anchor Point</Name>",
            "<Keyframes>0,0.25:0.75,0,0,0,0,0,0,5,4,0,0,0,0;</Keyframes>",
            "only Linear Anchor Point keys without spatial tangents convert; Premiere's other Anchor Point keys are unmeasured",
        ),
        (
            "<Name>Scale</Name>",
            "<Keyframes>0,135.,0,0,0,0,0,0;254016000000,100.,0,0,0,0,0,0;</Keyframes>",
            "animated Scale Height without Uniform Scale is unsupported",
        ),
    ] {
        // Key the parameter and set its `IsTimeVarying`, as Premiere does.
        let (head, param) = motion.split_at(motion.find(name).unwrap());
        let keyed = head.to_owned()
            + &param
                .replacen(
                    "<IsTimeVarying>false</IsTimeVarying>",
                    "<IsTimeVarying>true</IsTimeVarying>",
                    1,
                )
                .replacen(name, &format!("{name}{keys}"), 1);
        let reason = omitted_occurrence_reason(flags, &[(146, keyed), (20, top_crop(20))]);
        assert!(reason.ends_with(expected), "{expected}: {reason}");
    }
}

#[test]
fn unsupported_linear_and_radial_wipes_omit_their_occurrence() {
    for display_name in ["Linear Wipe", "Radial Wipe"] {
        let reason = omitted_occurrence_reason(DEFAULT_FLAGS, &[(20, wipe(20, display_name))]);
        if display_name == "Linear Wipe" {
            assert!(reason.contains("missing Linear Wipe Params"), "{reason}");
        } else {
            assert!(
            reason.ends_with(&format!("active effect \"{display_name}\" (match name \"AE.ADBE {display_name}\", VideoFilterComponent version 8, Component version 6) at stack position 1: changes what the clip covers or its transparency; the clip is not converted without it")),
            "{reason}"
        );
        }
        // Bypassed, the wipe keeps its clip and is reported like any unmapped effect.
        let reason = omitted_reason(wipe(20, display_name).replace(ACTIVE, BYPASSED));
        assert!(
            reason.starts_with(&format!("unknown bypassed effect \"{display_name}\"")),
            "{reason}"
        );
    }
}

fn project(effects: Vec<PrEffect>) -> PrProjectFile {
    let media = MediaId("/tmp/media/source.mp4".into());
    PrProjectFile::from_sequences(
        vec![PrSequence {
            native_frame_ticks: None,
            id: None,
            name: "Effect stack".into(),
            top_level: Some(true),
            video_tracks: vec![PrVideoTrack::media([PrVideoOccurrence {
                id: None,
                media: media.clone(),
                start_ticks: 0,
                end_ticks: 2 * TICKS,
                in_ticks: 0,
                out_ticks: 2 * TICKS,
                playback_rate: 1.0,
                frame_blending: None,
                time_remap: None,
                linear_wipe: None,
                opacity_mask: None,
                track_matte: None,
                opacity: 100.0,
                blend_mode: Default::default(),
                transform: Default::default(),
                crop: Default::default(),
                animations: Vec::new(),
                effects_above_mask: 0,
                stroke: None,
                active_transforms: u8::try_from(
                    effects
                        .iter()
                        .filter(|effect| matches!(effect.params, PrEffectParams::Transform(_)))
                        .count(),
                )
                .unwrap(),
                source_effects: None,
                effects,
                enabled: true,
            }])],
            audio: Vec::new(),
            frame_rate: FrameRate::Fps30,
            width: 1920,
            height: 1080,
            timeline_end_ticks: 2 * TICKS,
        }],
        [(
            media,
            PrMedia {
                name: "source.mp4".into(),
                relative_path: Some("./media/source.mp4".into()),
                relative_paths: vec!["./media/source.mp4".into()],
                absolute_paths: vec![(MediaPathField::FilePath, "/tmp/media/source.mp4".into())],
                video: Some(PrVideoStream {
                    pixel_aspect: Default::default(),
                    interpretation: Default::default(),
                    orientation: crate::schema::VideoOrientation::Identity,
                    intrinsic_ticks: 10 * TICKS,
                    frame_rate: (FrameRate::Fps30).into(),
                    width: 1920,
                    height: 1080,
                    kind: crate::schema::PrMediaKind::Video {
                        codec: Some(crate::schema::VideoCodec::H264),
                        hdr_profile: None,
                    },
                }),
                audio: None,
            },
        )]
        .into(),
    )
}

fn reread(xml: &str) -> PrVideoOccurrence {
    let (project, omissions) = inspect_project_with_omissions(xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    project.sequences[0].video_tracks[0].clip(0).clone()
}

#[test]
fn written_stack_rereads_with_its_order_bypass_and_values() {
    let effects = vec![
        gaussian_blur(true, 25.0, false),
        gaussian_blur(false, 80.5, true),
    ];
    let xml = project_xml(&project(effects.clone())).unwrap();
    assert_eq!(reread(&xml).effects, effects);
}

#[test]
fn written_stack_follows_intrinsic_opacity_and_motion_in_one_chain() {
    use crate::schema::{PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe};
    let mut native = project(vec![gaussian_blur(false, 12.25, false)]);
    let clip = native.sequences[0].video_tracks[0].clip_mut(0);
    clip.opacity = 50.0;
    clip.animations.push(PrPropertyAnimation::Rotation(vec![
        PrScalarKeyframe {
            source_ticks: 0,
            value: 0.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: TICKS,
            value: 45.0,
            easing: PrKeyframeEasing::Linear,
        },
    ]));
    let xml = project_xml(&native).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let match_names: Vec<_> = document
        .root_element()
        .children()
        .filter(|node| node.has_tag_name("VideoFilterComponent"))
        .filter_map(|node| {
            node.children()
                .find(|child| child.has_tag_name("MatchName"))
                .and_then(|child| child.text())
        })
        .collect();
    assert_eq!(
        match_names,
        [
            "AE.ADBE Opacity",
            "AE.ADBE Motion",
            "AE.ADBE Gaussian Blur 2"
        ]
    );
    let occurrence = reread(&xml);
    assert_eq!(occurrence.opacity, 50.0);
    assert_eq!(occurrence.animations.len(), 1);
    assert_eq!(occurrence.effects, [gaussian_blur(false, 12.25, false)]);
}

#[test]
fn written_keyed_blurriness_rereads_with_its_keys() {
    let mut keys = vec![
        key(TICKS / 2, 120.0),
        key(TICKS, 0.0),
        key(2 * TICKS, 606.0),
    ];
    keys[2].easing = PrKeyframeEasing::Hold;
    // A bypassed keyed blur that applies before an active static one.
    let keyed = with_blurriness_keys(gaussian_blur(false, 0.0, true), keys);
    let effects = vec![keyed, gaussian_blur(true, 10.0, false)];
    let xml = project_xml(&project(effects.clone())).unwrap();
    fn text<'a>(param: roxmltree::Node<'a, '_>, tag: &str) -> Option<&'a str> {
        param
            .children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
    }
    let document = roxmltree::Document::parse(&xml).unwrap();
    let keyed_params: Vec<_> = document
        .root_element()
        .children()
        .filter(|node| text(*node, "Keyframes").is_some())
        .map(|param| {
            ["Name", "IsTimeVarying", "StartKeyframe", "Keyframes"].map(|tag| text(param, tag))
        })
        .collect();
    // Written like a keyed Motion parameter: no `IsTimeVarying`, and the first
    // key's value as the static value.
    assert_eq!(
        keyed_params,
        [[
            Some("Blurriness"),
            None,
            Some("-91445760000000000,120.,0,0,0,0,0,0"),
            Some("127008000000,120,0,0,0,0,0,0;254016000000,0,4,0,0,0,0,0;508032000000,606,0,0,0,0,0,0;"),
        ]]
    );
    assert_eq!(reread(&xml).effects, effects);
}

#[test]
fn written_corner_pin_rereads_with_its_keys() {
    use PrKeyframeEasing::{Hold, Linear};
    // A bypassed Corner Pin whose Upper Left is keyed, above a static blur.
    let keyed = corner_pin_effect(
        false,
        [[0.2, 0.2], [1.0, 0.0], [0.0, 1.0], [0.85, 0.9]],
        vec![corner_keys(
            0,
            vec![
                point_key(TICKS / 2, [0.2, 0.2], Linear),
                point_key(TICKS, [0.0, 0.0], Linear),
                point_key(2 * TICKS, [0.3, 0.2], Hold),
            ],
        )],
    );
    let effects = vec![keyed, gaussian_blur(true, 10.0, false)];
    let xml = project_xml(&project(effects.clone())).unwrap();
    fn text<'a>(param: roxmltree::Node<'a, '_>, tag: &str) -> Option<&'a str> {
        param
            .children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
    }
    let document = roxmltree::Document::parse(&xml).unwrap();
    let corners: Vec<_> = document
        .root_element()
        .children()
        .filter(|node| node.has_tag_name("PointComponentParam"))
        .map(|param| {
            [
                "Name",
                "IsTimeVarying",
                "ParameterControlType",
                "StartKeyframe",
                "Keyframes",
            ]
            .map(|tag| text(param, tag))
        })
        .collect();
    // Premiere's static point form, and the keyed corner written like a keyed
    // Motion Position: no `IsTimeVarying`, its first key as `StartKeyframe`.
    let static_corner = |name, start| [Some(name), Some("false"), Some("6"), Some(start), None];
    assert_eq!(
        corners,
        [
            [
                Some("Upper Left"),
                None,
                Some("6"),
                Some("-91445760000000000,0.2:0.2,0,0,0,0,0,0,5,4,0,0,0,0"),
                Some("127008000000,0.2:0.2,0,0,0,0,0,0,0,0,0,0,0,0;254016000000,0:0,4,0,0,0,0,0,0,0,0,0,0,0;508032000000,0.3:0.2,0,0,0,0,0,0,0,0,0,0,0,0;"),
            ],
            static_corner(
                "Upper Right",
                "-91445760000000000,1:0,0,0,0,0,0,0,5,4,0,0,0,0"
            ),
            static_corner(
                "Lower Left",
                "-91445760000000000,0:1,0,0,0,0,0,0,5,4,0,0,0,0"
            ),
            static_corner(
                "Lower Right",
                "-91445760000000000,0.85:0.9,0,0,0,0,0,0,5,4,0,0,0,0"
            ),
        ]
    );
    assert_eq!(reread(&xml).effects, effects);
}

#[test]
fn written_directional_blurs_reread_with_their_keys() {
    // A bypassed Directional Blur with keyed Direction and Blur Length above
    // a static one and a Gaussian Blur.
    let hold = PrScalarKeyframe {
        easing: PrKeyframeEasing::Hold,
        ..key(2 * TICKS, 32767.0)
    };
    let effects = vec![
        keyed_directional(
            directional_blur(false, 0.0, 0.0),
            vec![key(0, -45.0), key(TICKS, 30.0), hold],
            vec![key(TICKS / 2, 12.5), key(TICKS, 1000.0)],
        ),
        directional_blur(true, 90.0, 0.0),
        gaussian_blur(true, 10.0, false),
    ];
    let xml = project_xml(&project(effects.clone())).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let fields = |node: roxmltree::Node<'_, '_>, tags: &[&str]| {
        let text = |tag: &&str| {
            node.children()
                .find(|child| child.has_tag_name(*tag))
                .and_then(|child| child.text())
        };
        tags.iter()
            .map(|tag| text(tag).unwrap_or("-"))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let records: Vec<_> = document
        .root_element()
        .children()
        .filter_map(|node| match fields(node, &["MatchName", "Name"]).as_str() {
            "AE.ADBE Motion Blur | -" => Some(fields(
                node.first_element_child().unwrap(),
                &["DisplayName", "Bypass", "Intrinsic"],
            )),
            "- | Direction" | "- | Blur Length" => Some(fields(
                node,
                &[
                    "Name",
                    "IsTimeVarying",
                    "ParameterControlType",
                    "StartKeyframe",
                    "Keyframes",
                    "LowerBound",
                    "UpperBound",
                    "UpperUIBound",
                ],
            )),
            _ => None,
        })
        .collect();
    // The corpus records' display name, which Premiere 26.5.1 shows as
    // "Directional Blur (Legacy)", and their static form. A keyed parameter
    // is written like a keyed Motion parameter: no `IsTimeVarying`, and its
    // first key as `StartKeyframe`.
    assert_eq!(
        records,
        [
            "Directional Blur | true | false",
            "Direction | - | 3 | -91445760000000000,-45.,0,0,0,0,0,0 | 0,-45,0,0,0,0,0,0;254016000000,30,4,0,0,0,0,0;508032000000,32767,0,0,0,0,0,0; | -32768 | 32767 | -",
            "Blur Length | - | 2 | -91445760000000000,12.5,0,0,0,0,0,0 | 127008000000,12.5,0,0,0,0,0,0;254016000000,1000,0,0,0,0,0,0; | 0 | 1000 | 20",
            "Directional Blur | false | false",
            "Direction | false | 3 | -91445760000000000,90.,0,0,0,0,0,0 | - | -32768 | 32767 | -",
            "Blur Length | false | 2 | -91445760000000000,0.,0,0,0,0,0,0 | - | 0 | 1000 | 20",
        ]
    );
    assert_eq!(reread(&xml).effects, effects);
}

#[test]
fn written_brightness_contrast_rereads_with_its_keys() {
    // A bypassed Brightness & Contrast with keyed Brightness and Contrast
    // above a Gaussian Blur and a static one.
    let hold = PrScalarKeyframe {
        easing: PrKeyframeEasing::Hold,
        ..key(2 * TICKS, -100.0)
    };
    let effects = vec![
        brightness_contrast(
            false,
            [12.5, 20.0],
            [
                vec![key(0, 12.5), key(TICKS, 100.0), hold],
                vec![key(TICKS / 2, 20.0), key(TICKS, -40.0)],
            ],
        ),
        gaussian_blur(true, 10.0, false),
        brightness_contrast(true, [37.0, -25.0], [vec![], vec![]]),
    ];
    let xml = project_xml(&project(effects.clone())).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let fields = |node: roxmltree::Node<'_, '_>, tags: &[&str]| {
        let text = |tag: &&str| {
            node.children()
                .find(|child| child.has_tag_name(*tag))
                .and_then(|child| child.text())
        };
        tags.iter()
            .map(|tag| text(tag).unwrap_or("-"))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let records: Vec<_> = document
        .root_element()
        .children()
        .filter_map(|node| match fields(node, &["MatchName", "Name"]).as_str() {
            "AE.ADBE Brightness & Contrast 2 | -" => Some(fields(
                node.first_element_child().unwrap(),
                &["DisplayName", "Bypass", "Intrinsic"],
            )),
            "- | Brightness" | "- | Contrast" => Some(fields(
                node,
                &[
                    "Name",
                    "IsTimeVarying",
                    "ParameterControlType",
                    "StartKeyframe",
                    "Keyframes",
                    "LowerBound",
                    "UpperBound",
                ],
            )),
            _ => None,
        })
        .collect();
    // The corpus records' static form (`ParameterControlType` 2). A keyed
    // parameter is written like a keyed Motion parameter: no `IsTimeVarying`,
    // and its first key as `StartKeyframe`.
    assert_eq!(
        records,
        [
            "Brightness & Contrast | true | false",
            "Brightness | - | 2 | -91445760000000000,12.5,0,0,0,0,0,0 | 0,12.5,0,0,0,0,0,0;254016000000,100,4,0,0,0,0,0;508032000000,-100,0,0,0,0,0,0; | -100 | 100",
            "Contrast | - | 2 | -91445760000000000,20.,0,0,0,0,0,0 | 127008000000,20,0,0,0,0,0,0;254016000000,-40,0,0,0,0,0,0; | -100 | 100",
            "Brightness & Contrast | false | false",
            "Brightness | false | 2 | -91445760000000000,37.,0,0,0,0,0,0 | - | -100 | 100",
            "Contrast | false | 2 | -91445760000000000,-25.,0,0,0,0,0,0 | - | -100 | 100",
        ]
    );
    assert_eq!(reread(&xml).effects, effects);
}

#[test]
fn written_invert_rereads_with_its_keys() {
    // A bypassed Invert with keyed Blend above a Gaussian Blur and a static one.
    let hold = PrScalarKeyframe {
        easing: PrKeyframeEasing::Hold,
        ..key(2 * TICKS, 100.0)
    };
    let effects = vec![
        invert_effect(
            false,
            100.0,
            vec![key(TICKS / 2, 100.0), key(TICKS, 20.0), hold],
        ),
        gaussian_blur(true, 10.0, false),
        invert_effect(true, 30.0, vec![]),
    ];
    let xml = project_xml(&project(effects.clone())).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let fields = |node: roxmltree::Node<'_, '_>, tags: &[&str]| {
        let text = |tag: &&str| {
            node.children()
                .find(|child| child.has_tag_name(*tag))
                .and_then(|child| child.text())
        };
        tags.iter()
            .map(|tag| text(tag).unwrap_or("-"))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let records: Vec<_> = document
        .root_element()
        .children()
        .filter_map(|node| match fields(node, &["MatchName", "Name"]).as_str() {
            "AE.ADBE Invert | -" => Some(format!(
                "{} | {}",
                fields(
                    node.first_element_child().unwrap(),
                    &["DisplayName", "Bypass", "Intrinsic"],
                ),
                fields(node, &["PremiereFilterPrivateData"])
            )),
            "- | Channel" | "- | Blend With Original" => Some(fields(
                node,
                &[
                    "Name",
                    "IsTimeVarying",
                    "DiscontinuousInterpolate",
                    "ParameterControlType",
                    "StartKeyframe",
                    "Keyframes",
                    "LowerBound",
                    "UpperBound",
                ],
            )),
            _ => None,
        })
        .collect();
    // The corpus records' form (control types 7 and 2) without private data;
    // the Channel is RGB. A keyed Blend is written like a keyed Motion
    // parameter: no `IsTimeVarying`, and its first key as `StartKeyframe`.
    assert_eq!(
        records,
        [
            "Invert | true | false | -",
            "Channel | false | true | 7 | -91445760000000000,0,0,0,0,0,0,0 | - | 0 | 15",
            "Blend With Original | - | - | 2 | -91445760000000000,100.,0,0,0,0,0,0 | 127008000000,100,0,0,0,0,0,0;254016000000,20,4,0,0,0,0,0;508032000000,100,0,0,0,0,0,0; | 0 | 100",
            "Invert | false | false | -",
            "Channel | false | true | 7 | -91445760000000000,0,0,0,0,0,0,0 | - | 0 | 15",
            "Blend With Original | false | - | 2 | -91445760000000000,30.,0,0,0,0,0,0 | - | 0 | 100",
        ]
    );
    assert_eq!(reread(&xml).effects, effects);
}

#[test]
fn written_tint_and_black_white_reread_with_their_keys() {
    // A bypassed Tint with keyed Map White To and Amount above a Gaussian
    // Blur, a static Tint and a Black & White.
    let linear = PrKeyframeEasing::Linear;
    let hold = PrScalarKeyframe {
        easing: PrKeyframeEasing::Hold,
        ..key(2 * TICKS, 50.0)
    };
    let effects = vec![
        tint_effect(
            false,
            ([0, 0, 0], [255, 255, 255], 0.0),
            (
                vec![],
                vec![
                    colour_key(TICKS / 2, [255, 255, 255], linear),
                    colour_key(TICKS, [0, 128, 255], linear),
                    colour_key(2 * TICKS, [255, 128, 0], PrKeyframeEasing::Hold),
                ],
                vec![key(TICKS / 2, 0.0), key(TICKS, 100.0), hold],
            ),
        ),
        gaussian_blur(true, 10.0, false),
        tint_effect(
            true,
            ([163, 247, 143], [240, 242, 22], 100.0),
            (vec![], vec![], vec![]),
        ),
        black_white_effect(true),
    ];
    let xml = project_xml(&project(effects.clone())).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let fields = |node: roxmltree::Node<'_, '_>, tags: &[&str]| {
        let text = |tag: &&str| {
            node.children()
                .find(|child| child.has_tag_name(*tag))
                .and_then(|child| child.text())
        };
        tags.iter()
            .map(|tag| text(tag).unwrap_or("-"))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let records: Vec<_> = document
        .root_element()
        .children()
        .filter_map(|node| match fields(node, &["MatchName", "Name"]).as_str() {
            "AE.ADBE Tint | -" | "AE.ADBE Black & White | -" => {
                let body = node.first_element_child().unwrap();
                Some(format!(
                    "{} | Params {}",
                    fields(body, &["DisplayName", "Bypass", "Intrinsic"]),
                    body.children().any(|child| child.has_tag_name("Params"))
                ))
            }
            "- | Map Black To" | "- | Map White To" | "- | Amount to Tint" => Some(fields(
                node,
                &[
                    "Name",
                    "IsTimeVarying",
                    "ParameterControlType",
                    "StartKeyframe",
                    "Keyframes",
                    "LowerBound",
                    "UpperBound",
                ],
            )),
            _ => None,
        })
        .collect();
    // The corpus Tint records' form (Premiere 12.1: control types 5 and 2,
    // colour bounds 0 to 2^64 - 1) with opaque colours; a keyed parameter is
    // written like a keyed Motion parameter: no `IsTimeVarying`, its first
    // key as `StartKeyframe`, and colour keys with zero handles. A Black &
    // White has no `Params`, as Premiere 26.5.1 saves it.
    assert_eq!(
        records,
        [
            "Tint | true | false | Params true",
            "Map Black To | false | 5 | -91445760000000000,18374686479671623680,0,0,0,0,0,0 | - | 0 | 18446744073709551615",
            "Map White To | - | 5 | -91445760000000000,18374966859414961920,0,0,0,0,0,0 | 127008000000,18374966859414961920,0,0,0,0,0,0;254016000000,18374686481819172608,4,0,0,0,0,0;508032000000,18374966857284190208,0,0,0,0,0,0; | 0 | 18446744073709551615",
            "Amount to Tint | - | 2 | -91445760000000000,0.,0,0,0,0,0,0 | 127008000000,0,0,0,0,0,0,0;254016000000,100,4,0,0,0,0,0;508032000000,50,0,0,0,0,0,0; | 0 | 100",
            "Tint | false | false | Params true",
            "Map Black To | false | 5 | -91445760000000000,18374865704210960128,0,0,0,0,0,0 | - | 0 | 18446744073709551615",
            "Map White To | false | 5 | -91445760000000000,18374950366522381824,0,0,0,0,0,0 | - | 0 | 18446744073709551615",
            "Amount to Tint | false | 2 | -91445760000000000,100.,0,0,0,0,0,0 | - | 0 | 100",
            "Black & White | false | false | Params false",
        ]
    );
    assert_eq!(reread(&xml).effects, effects);
}

#[test]
fn written_ramps_reread_with_their_keys() {
    // A bypassed vertical ramp with a keyed End of Ramp along its axis and
    // keyed colours above a Gaussian Blur, and a static horizontal ramp with
    // a Blend.
    let linear = PrKeyframeEasing::Linear;
    let effects = vec![
        ramp_effect(
            false,
            ([0.5, 0.0], [0.5, 1.0]),
            ([0, 0, 0], [255, 255, 255]),
            1.0,
            vec![
                PrEffectParamAnimation {
                    param: &RAMP_START_COLOR,
                    keys: PrEffectParamKeys::Colour(vec![
                        colour_key(TICKS / 2, [0, 0, 0], linear),
                        colour_key(TICKS, [200, 40, 40], PrKeyframeEasing::Hold),
                    ]),
                },
                PrEffectParamAnimation {
                    param: &RAMP_END,
                    keys: PrEffectParamKeys::Point(vec![
                        point_key(TICKS / 2, [0.5, 1.0], linear),
                        point_key(3 * TICKS / 2, [0.5, 0.6], linear),
                    ]),
                },
                PrEffectParamAnimation {
                    param: &RAMP_BLEND,
                    keys: PrEffectParamKeys::Scalar(vec![
                        key(TICKS, 1.0),
                        key(3 * TICKS / 2, 0.0),
                        PrScalarKeyframe {
                            easing: PrKeyframeEasing::Hold,
                            ..key(5 * TICKS / 2, 0.5)
                        },
                    ]),
                },
            ],
        ),
        gaussian_blur(true, 10.0, false),
        ramp_effect(
            true,
            ([0.2, 0.5], [0.8, 0.5]),
            ([200, 40, 40], [40, 40, 200]),
            0.300000011921,
            vec![],
        ),
    ];
    let xml = project_xml(&project(effects.clone())).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let fields = |node: roxmltree::Node<'_, '_>, tags: &[&str]| {
        let text = |tag: &&str| {
            node.children()
                .find(|child| child.has_tag_name(*tag))
                .and_then(|child| child.text())
        };
        tags.iter()
            .map(|tag| text(tag).unwrap_or("-"))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let param_fields = [
        "Name",
        "IsTimeVarying",
        "ParameterControlType",
        "StartKeyframe",
        "Keyframes",
        "LowerBound",
        "UpperBound",
        "UpperUIBound",
    ];
    let records: Vec<_> = document
        .root_element()
        .children()
        .filter_map(|node| match fields(node, &["MatchName", "Name"]).as_str() {
            "AE.ADBE Ramp | -" => Some(fields(
                node.first_element_child().unwrap(),
                &["DisplayName", "Bypass", "Intrinsic"],
            )),
            "- | Start of Ramp"
            | "- | Start Color"
            | "- | End of Ramp"
            | "- | End Color"
            | "- | Ramp Shape"
            | "- | Ramp Scatter"
            | "- | Blend With Original" => Some(fields(node, &param_fields)),
            _ => None,
        })
        .collect();
    // The corpus Ramp records' form (Premiere 12.1: control types 6, 5, 7 and
    // 2, colour bounds 0 to 2^64 - 1, Scatter's UI bound) with opaque colours,
    // the linear Shape and Scatter 0; a keyed parameter is written like a
    // keyed Motion parameter: no `IsTimeVarying`, its first key as
    // `StartKeyframe`, point keys with straight spatial fields and colour keys
    // with zero handles.
    assert_eq!(
        records,
        [
            "Ramp | true | false",
            "Start of Ramp | false | 6 | -91445760000000000,0.5:0,0,0,0,0,0,0,5,4,0,0,0,0 | - | - | - | -",
            "Start Color | - | 5 | -91445760000000000,18374686479671623680,0,0,0,0,0,0 | 127008000000,18374686479671623680,4,0,0,0,0,0;254016000000,18374906382668277760,0,0,0,0,0,0; | 0 | 18446744073709551615 | -",
            "End of Ramp | - | 6 | -91445760000000000,0.5:1,0,0,0,0,0,0,5,4,0,0,0,0 | 127008000000,0.5:1,0,0,0,0,0,0,0,0,0,0,0,0;381024000000,0.5:0.6,0,0,0,0,0,0,0,0,0,0,0,0; | - | - | -",
            "End Color | false | 5 | -91445760000000000,18374966859414961920,0,0,0,0,0,0 | - | 0 | 18446744073709551615 | -",
            "Ramp Shape | false | 7 | -91445760000000000,0,0,0,0,0,0,0 | - | 0 | 1 | -",
            "Ramp Scatter | false | 2 | -91445760000000000,0.,0,0,0,0,0,0 | - | 0 | 512 | 50",
            "Blend With Original | - | 2 | -91445760000000000,1.,0,0,0,0,0,0 | 254016000000,1,0,0,0,0,0,0;381024000000,0,4,0,0,0,0,0;635040000000,0.5,0,0,0,0,0,0; | 0 | 1 | -",
            "Ramp | false | false",
            "Start of Ramp | false | 6 | -91445760000000000,0.2:0.5,0,0,0,0,0,0,5,4,0,0,0,0 | - | - | - | -",
            "Start Color | false | 5 | -91445760000000000,18374906382668277760,0,0,0,0,0,0 | - | 0 | 18446744073709551615 | -",
            "End of Ramp | false | 6 | -91445760000000000,0.8:0.5,0,0,0,0,0,0,5,4,0,0,0,0 | - | - | - | -",
            "End Color | false | 5 | -91445760000000000,18374730460807874560,0,0,0,0,0,0 | - | 0 | 18446744073709551615 | -",
            "Ramp Shape | false | 7 | -91445760000000000,0,0,0,0,0,0,0 | - | 0 | 1 | -",
            "Ramp Scatter | false | 2 | -91445760000000000,0.,0,0,0,0,0,0 | - | 0 | 512 | 50",
            "Blend With Original | false | 2 | -91445760000000000,0.300000011921,0,0,0,0,0,0 | - | 0 | 1 | -",
        ]
    );
    assert_eq!(reread(&xml).effects, effects);
}

#[test]
fn written_mosaics_reread_without_a_checkbox_name_and_with_their_keys() {
    // A bypassed Mosaic with clip D's Hold keys on both counts (its static
    // counts the first keys') above a Gaussian Blur, and a static 48 x 27 one.
    let effects = vec![
        mosaic_effect(
            false,
            (10, 10),
            vec![
                PrEffectParamAnimation {
                    param: &MOSAIC_HORIZONTAL_BLOCKS,
                    keys: PrEffectParamKeys::Scalar(mosaic_hold_keys(40.0)),
                },
                PrEffectParamAnimation {
                    param: &MOSAIC_VERTICAL_BLOCKS,
                    keys: PrEffectParamKeys::Scalar(mosaic_hold_keys(30.0)),
                },
            ],
        ),
        gaussian_blur(true, 10.0, false),
        mosaic_effect(true, (48, 27), vec![]),
    ];
    let xml = project_xml(&project(effects.clone())).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let fields = |node: roxmltree::Node<'_, '_>, tags: &[&str]| {
        let text = |tag: &&str| {
            node.children()
                .find(|child| child.has_tag_name(*tag))
                .and_then(|child| child.text())
        };
        tags.iter()
            .map(|tag| text(tag).unwrap_or("-"))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let param_fields = [
        "Name",
        "IsTimeVarying",
        "ParameterControlType",
        "StartKeyframe",
        "Keyframes",
        "LowerBound",
        "UpperBound",
        "UpperUIBound",
        "ParameterID",
    ];
    let records: Vec<_> = document
        .root_element()
        .children()
        .filter_map(|node| {
            match (
                fields(node, &["MatchName", "Name"]).as_str(),
                node.attribute("ClassID"),
            ) {
                ("AE.ADBE Mosaic | -", _) => Some(fields(
                    node.first_element_child().unwrap(),
                    &["DisplayName", "Bypass", "Intrinsic"],
                )),
                ("- | Horizontal Blocks" | "- | Vertical Blocks", _)
                | ("- | -", Some("cc12343e-f113-4d3b-ae05-b287db77d461")) => {
                    Some(fields(node, &param_fields))
                }
                _ => None,
            }
        })
        .collect();
    // The corpus records' form (7/5, control types 1 and 4, bounds and the UI
    // bound) with whole counts; the checkbox has no `Name` element, as
    // Premiere 26.5.1 saves it; a keyed count is written like a keyed Motion
    // parameter: no `IsTimeVarying`, its first key as `StartKeyframe`, mode 4
    // (Hold) out of every key but the last, zero handles.
    assert_eq!(
        records,
        [
            "Mosaic (Legacy) | true | false",
            "Horizontal Blocks | - | 1 | -91445760000000000,10,0,0,0,0,0,0 | 254016000000,10,4,0,0,0,0,0;381024000000,40,4,0,0,0,0,0;635040000000,20,0,0,0,0,0,0; | 1 | 4000 | 200 | 1",
            "Vertical Blocks | - | 1 | -91445760000000000,10,0,0,0,0,0,0 | 254016000000,10,4,0,0,0,0,0;381024000000,30,4,0,0,0,0,0;635040000000,20,0,0,0,0,0,0; | 1 | 4000 | 200 | 2",
            "- | false | 4 | -91445760000000000,true,0,0,0,0,0,0 | - | false | true | - | 3",
            "Mosaic (Legacy) | false | false",
            "Horizontal Blocks | false | 1 | -91445760000000000,48,0,0,0,0,0,0 | - | 1 | 4000 | 200 | 1",
            "Vertical Blocks | false | 1 | -91445760000000000,27,0,0,0,0,0,0 | - | 1 | 4000 | 200 | 2",
            "- | false | 4 | -91445760000000000,true,0,0,0,0,0,0 | - | false | true | - | 3",
        ]
    );
    // The Gaussian Blur checkbox keeps its `<Name> </Name>`.
    assert!(xml.contains("<Name> </Name>"), "{xml}");
    assert_eq!(reread(&xml).effects, effects);
}

#[test]
fn written_replicates_reread_with_their_count_and_hold_keys() {
    // A bypassed Replicate with Hold Count keys (its static Count the first
    // key's) above a Gaussian Blur, and a static Count 16.
    let effects = vec![
        replicate_effect(false, 2, replicate_hold_keys()),
        gaussian_blur(true, 10.0, false),
        replicate_effect(true, 16, vec![]),
    ];
    let xml = project_xml(&project(effects.clone())).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let fields = |node: roxmltree::Node<'_, '_>, tags: &[&str]| {
        tags.iter()
            .map(|tag| {
                node.children()
                    .find(|child| child.has_tag_name(*tag))
                    .and_then(|child| child.text())
                    .unwrap_or("-")
            })
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let records: Vec<_> = document
        .root_element()
        .children()
        .filter_map(|node| match fields(node, &["MatchName", "Name"]).as_str() {
            "AE.ADBE Replicate | -" => Some(fields(
                node.first_element_child().unwrap(),
                &["DisplayName", "Bypass", "Intrinsic"],
            )),
            "- | Count" => Some(fields(
                node,
                &[
                    "Name",
                    "IsTimeVarying",
                    "ParameterControlType",
                    "StartKeyframe",
                    "Keyframes",
                    "LowerBound",
                    "UpperBound",
                    "UpperUIBound",
                    "ParameterID",
                ],
            )),
            _ => None,
        })
        .collect();
    // The corpus effect form (7/5) with Premiere 26.5.1's Count: whole
    // values, bounds 2 to 16 and no UI bounds; keys written like a keyed
    // Mosaic count's, mode 4 (Hold) out of every key but the last.
    assert_eq!(
        records,
        [
            "Replicate | true | false",
            "Count | - | 1 | -91445760000000000,2,0,0,0,0,0,0 | 254016000000,2,4,0,0,0,0,0;381024000000,4,4,0,0,0,0,0;635040000000,3,0,0,0,0,0,0; | 2 | 16 | - | 1",
            "Replicate | false | false",
            "Count | false | 1 | -91445760000000000,16,0,0,0,0,0,0 | - | 2 | 16 | - | 1",
        ]
    );
    assert_eq!(reread(&xml).effects, effects);
}

#[test]
fn written_posterizes_reread_with_whole_levels_and_hold_keys() {
    // A bypassed Posterize with clip D's Hold keys (its static Level the
    // first key's) above a Gaussian Blur, and a static Level 16.
    let effects = vec![
        posterize_effect(false, 3, posterize_hold_keys()),
        gaussian_blur(true, 10.0, false),
        posterize_effect(true, 16, vec![]),
    ];
    let xml = project_xml(&project(effects.clone())).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let fields = |node: roxmltree::Node<'_, '_>, tags: &[&str]| {
        let text = |tag: &&str| {
            node.children()
                .find(|child| child.has_tag_name(*tag))
                .and_then(|child| child.text())
        };
        tags.iter()
            .map(|tag| text(tag).unwrap_or("-"))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let param_fields = [
        "Name",
        "IsTimeVarying",
        "ParameterControlType",
        "StartKeyframe",
        "Keyframes",
        "LowerBound",
        "UpperBound",
        "UpperUIBound",
        "ParameterID",
    ];
    let records: Vec<_> = document
        .root_element()
        .children()
        .filter_map(|node| match fields(node, &["MatchName", "Name"]).as_str() {
            "AE.ADBE Posterize | -" => Some(fields(
                node.first_element_child().unwrap(),
                &["DisplayName", "Bypass", "Intrinsic"],
            )),
            "- | Level" => Some(fields(node, &param_fields)),
            _ => None,
        })
        .collect();
    // The writer's corpus generation (7/5 with `Bypass` and `Intrinsic`, the
    // Level class's control type 8) with Premiere 26.5.1's bounds and UI
    // bound; a keyed Level is written like a keyed Motion parameter: no
    // `IsTimeVarying`, its first key as `StartKeyframe`, mode 4 (Hold) out of
    // every key but the last, zero handles.
    assert_eq!(
        records,
        [
            "Posterize | true | false",
            "Level | - | 8 | -91445760000000000,3.,0,0,0,0,0,0 | 254016000000,3,4,0,0,0,0,0;381024000000,8,4,0,0,0,0,0;635040000000,5,0,0,0,0,0,0; | 2 | 255 | 32 | 1",
            "Posterize | false | false",
            "Level | false | 8 | -91445760000000000,16.,0,0,0,0,0,0 | - | 2 | 255 | 32 | 1",
        ]
    );
    assert_eq!(reread(&xml).effects, effects);
}

#[test]
fn written_transforms_reread_without_checkbox_names_and_with_their_keys() {
    // Clip D's Transform with Position and Opacity keys added, the shutter
    // checkbox off at 180 and bicubic Sampling, its static values the first
    // keys', below clip B's static one.
    let d_animations = {
        let mut animations = transform_d_animations();
        animations.insert(
            0,
            PrEffectParamAnimation {
                param: &TRANSFORM_POSITION,
                keys: PrEffectParamKeys::Point(vec![
                    point_key(0, [0.25, 0.5], PrKeyframeEasing::Linear),
                    point_key(TICKS, [0.75, 0.5], PrKeyframeEasing::Linear),
                ]),
            },
        );
        animations.push(PrEffectParamAnimation {
            param: &TRANSFORM_OPACITY,
            keys: PrEffectParamKeys::Scalar(vec![key(TICKS, 100.0), key(3 * TICKS / 2, 50.0)]),
        });
        animations
    };
    let effects = vec![
        transform_effect(
            PrTransform {
                position: [0.25, 0.5],
                uniform_scale: true,
                composition_shutter_angle: false,
                shutter_angle: 180.0,
                bicubic_sampling: true,
                ..DEFAULT_PR_TRANSFORM
            },
            d_animations,
        ),
        transform_effect(
            PrTransform {
                anchor_point: [0.75, 0.5],
                uniform_scale: true,
                scale_height: 50.0,
                rotation: 30.0,
                ..DEFAULT_PR_TRANSFORM
            },
            vec![],
        ),
    ];
    let xml = project_xml(&project(effects.clone())).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let fields = |node: roxmltree::Node<'_, '_>, tags: &[&str]| {
        let text = |tag: &&str| {
            node.children()
                .find(|child| child.has_tag_name(*tag))
                .and_then(|child| child.text())
        };
        tags.iter()
            .map(|tag| text(tag).unwrap_or("-"))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let param_fields = [
        "Name",
        "IsTimeVarying",
        "ParameterControlType",
        "DiscontinuousInterpolate",
        "StartKeyframe",
        "Keyframes",
        "LowerBound",
        "UpperBound",
        "LowerUIBound",
        "UpperUIBound",
        "ParameterID",
    ];
    let mut transforms = 0;
    let records: Vec<_> = document
        .root_element()
        .children()
        .filter_map(|node| {
            if fields(node, &["MatchName"]) == "AE.ADBE Geometry" {
                transforms += 1;
                return Some(fields(
                    node.first_element_child().unwrap(),
                    &["DisplayName", "Bypass", "Intrinsic"],
                ));
            }
            // The Transform parameters are the records after the first
            // Transform component and before the second's parameters end.
            (transforms == 1
                && (node.has_tag_name("VideoComponentParam")
                    || node.has_tag_name("PointComponentParam")))
            .then(|| fields(node, &param_fields))
        })
        .collect();
    // The AE-family form (7/5 with `Bypass`, control types 6/4/2/3/7, the
    // 26.5.1 bounds and slider range, `Sampling` with
    // `DiscontinuousInterpolate`), the checkboxes without a `Name`; a keyed
    // parameter written like a keyed Motion parameter: no `IsTimeVarying`,
    // its first key as `StartKeyframe`, mode 4 (Hold) out of a key that
    // starts a Hold, point keys in the 14-field straight-path form.
    assert_eq!(
        records,
        [
            "Transform | false | false",
            "Anchor Point | false | 6 | - | -91445760000000000,0.5:0.5,0,0,0,0,0,0,5,4,0,0,0,0 | - | - | - | - | - | 1",
            "Position | - | 6 | - | -91445760000000000,0.25:0.5,0,0,0,0,0,0,5,4,0,0,0,0 | 0,0.25:0.5,0,0,0,0,0,0,0,0,0,0,0,0;254016000000,0.75:0.5,0,0,0,0,0,0,0,0,0,0,0,0; | - | - | - | - | 2",
            "- | false | 4 | - | -91445760000000000,true,0,0,0,0,0,0 | - | false | true | - | - | 11",
            "Scale Height | - | 2 | - | -91445760000000000,100.,0,0,0,0,0,0 | 254016000000,100,0,0,0,0,0,0;381024000000,200,4,0,0,0,0,0;635040000000,50,0,0,0,0,0,0; | -30000 | 30000 | -200 | 200 | 3",
            "Scale Width | false | 2 | - | -91445760000000000,100.,0,0,0,0,0,0 | - | -30000 | 30000 | -200 | 200 | 4",
            "Skew | false | 2 | - | -91445760000000000,0.,0,0,0,0,0,0 | - | -70 | 70 | - | - | 5",
            "Skew Axis | false | 3 | - | -91445760000000000,0.,0,0,0,0,0,0 | - | -32768 | 32767 | - | - | 6",
            "Rotation | - | 3 | - | -91445760000000000,0.,0,0,0,0,0,0 | 254016000000,0,0,0,0,0,0,0;635040000000,90,0,0,0,0,0,0; | -32768 | 32767 | - | - | 7",
            "Opacity | - | 2 | - | -91445760000000000,100.,0,0,0,0,0,0 | 254016000000,100,0,0,0,0,0,0;381024000000,50,0,0,0,0,0,0; | 0 | 100 | - | - | 8",
            "- | false | 4 | - | -91445760000000000,false,0,0,0,0,0,0 | - | false | true | - | - | 9",
            "Shutter Angle | false | 2 | - | -91445760000000000,180.,0,0,0,0,0,0 | - | 0 | 360 | - | - | 10",
            "Sampling | false | 7 | true | -91445760000000000,1,0,0,0,0,0,0 | - | 0 | 1 | - | - | 12",
            "Transform | false | false",
        ]
    );
    assert_eq!(reread(&xml).effects, effects);
}

#[test]
fn blurriness_outside_premiere_bounds_rejects_before_encoding() {
    let error = project_xml(&project(vec![gaussian_blur(true, 30001.0, false)]))
        .unwrap_err()
        .to_string();
    assert!(error.contains("Gaussian Blur Blurriness 30001"), "{error}");
}

/// Every Levels row at its neutral value, Premiere's default.
const NEUTRAL_LEVELS: [u16; 20] = [
    0, 255, 0, 255, 100, 0, 255, 0, 255, 100, 0, 255, 0, 255, 100, 0, 255, 0, 255, 100,
];

/// `values` as Levels private data: little-endian u16s in base64.
fn levels_private_data(values: &[u16]) -> String {
    STANDARD.encode(
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>(),
    )
}

/// A `PR.ADBE Levels` component and its 20 parameter records in the shape
/// Premiere 26.5.1 saves (`feature_levels_strict`): no `Bypass`
/// or `Intrinsic`, every `ParameterID` -1, the whole-number `StartKeyframe`s
/// `start`, which its private data repeats, and `keys` on the parameter at
/// their index. Records use ObjectIDs `id..id + 20`.
fn levels(id: u32, start: [u16; 20], keys: Option<(u32, &str)>) -> String {
    let params: String = (0..20)
        .map(|index| {
            format!(
                "<Param Index=\"{index}\" ObjectRef=\"{}\"/>",
                id + 1 + index
            )
        })
        .collect();
    let mut records = format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"><Component Version=\"7\"><Params Version=\"1\">{params}</Params><ID>3</ID><DisplayName>Levels</DisplayName></Component><PremiereFilterPrivateData Encoding=\"base64\" BinaryHash=\"levels-{id}\">{}\n\t\t</PremiereFilterPrivateData><VideoFilterType>1</VideoFilterType><MatchName>PR.ADBE Levels</MatchName></VideoFilterComponent>",
        levels_private_data(&start)
    );
    for ((index, param), value) in (0..).zip(LEVELS.params).zip(start) {
        let (time_varying, keyframes) = match keys {
            Some((keyed, wire)) if keyed == index => (
                "<IsTimeVarying>true</IsTimeVarying>",
                format!("<Keyframes>{wire}</Keyframes>"),
            ),
            _ => ("", String::new()),
        };
        records.push_str(&format!("<VideoComponentParam ObjectID=\"{}\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"10\"><Name>{}</Name><ParameterControlType>1</ParameterControlType>{time_varying}<ParameterID>-1</ParameterID><StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0</StartKeyframe>{keyframes}<LowerBound>0</LowerBound><UpperBound>{}</UpperBound></VideoComponentParam>", id + 1 + index, param.name, param.upper_bound));
    }
    records
}

/// An imported Levels with its master (RGB) values in native order.
fn levels_effect(rgb: [f64; 5], animations: Vec<PrEffectParamAnimation>) -> PrEffect {
    PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::Levels(PrLevels::Master { rgb }),
        animations,
    }
}

/// The (RGB) White Output Level keys of clip D of `feature_levels_strict`
/// (Premiere 26.5.1): 255 at source 1 s, Linear to 128 at 1.5 s, then a Hold
/// to 200 at 2.5 s.
const FIXTURE_WHITE_OUTPUT_KEYS: &str = "254016000000,255,0,0,0,0.16666666666666666,-254,0.16666666666666666;381024000000,128,4,0,-254,0.16666666666666666,0,0.33333333333333331;635040000000,200,0,0,72,0.16666666666666666,0,0.16666666666666666;";

#[test]
fn premiere_26_levels_read_static_and_keyed_master_values() {
    let with_master = |master: [u16; 5]| {
        let mut start = NEUTRAL_LEVELS;
        start[..5].copy_from_slice(&master);
        start
    };
    // Clips A, C and D of the run E4 fixture: a black input, a Gamma of 1.5,
    // and White Output keys whose StartKeyframe keeps 255.
    let white_output = PrEffectParamAnimation {
        param: &LEVELS.params[3],
        keys: PrEffectParamKeys::Scalar(vec![
            key(TICKS, 255.0),
            key(3 * TICKS / 2, 128.0),
            PrScalarKeyframe {
                easing: PrKeyframeEasing::Hold,
                ..key(5 * TICKS / 2, 200.0)
            },
        ]),
    };
    for (records, expected) in [
        (
            levels(20, with_master([3, 255, 0, 255, 100]), None),
            levels_effect([3.0, 255.0, 0.0, 255.0, 100.0], Vec::new()),
        ),
        (
            levels(20, with_master([0, 255, 0, 255, 150]), None),
            levels_effect([0.0, 255.0, 0.0, 255.0, 150.0], Vec::new()),
        ),
        (
            levels(20, NEUTRAL_LEVELS, Some((3, FIXTURE_WHITE_OUTPUT_KEYS))),
            levels_effect([0.0, 255.0, 0.0, 255.0, 100.0], vec![white_output]),
        ),
    ] {
        let (occurrence, omissions) = read(&with_effects(&[(20, records)]));
        assert!(omissions.is_empty(), "{omissions:?}");
        assert_eq!(occurrence.effects, [expected]);
    }
}

#[test]
fn deduplicated_levels_private_data_reads_its_stored_copy() {
    // Premiere stores each distinct blob once and names it from empty copies,
    // as `abstract_slideshow` does for 49 Motion blobs.
    let stored = format!(
        "BinaryHash=\"levels-50\">{}\n\t\t</PremiereFilterPrivateData>",
        levels_private_data(&NEUTRAL_LEVELS)
    );
    let copy = levels(50, NEUTRAL_LEVELS, None).replace(&stored, "BinaryHash=\"levels-20\"/>");
    assert!(!copy.contains(&stored));
    let (occurrence, omissions) = read(&with_effects(&[
        (20, levels(20, NEUTRAL_LEVELS, None)),
        (50, copy),
    ]));
    assert!(omissions.is_empty(), "{omissions:?}");
    let neutral = levels_effect([0.0, 255.0, 0.0, 255.0, 100.0], Vec::new());
    assert_eq!(occurrence.effects, [neutral.clone(), neutral]);
}

#[test]
fn levels_keeps_its_order_around_a_blur() {
    let neutral = || levels_effect([0.0, 255.0, 0.0, 255.0, 100.0], Vec::new());
    // Premiere applies the chain in descending `Index`, so the
    // component written second applies first.
    for (components, expected) in [
        (
            vec![(20, levels(20, NEUTRAL_LEVELS, None)), (50, blur(50))],
            [gaussian_blur(true, 25.0, false), neutral()],
        ),
        (
            vec![(50, blur(50)), (20, levels(20, NEUTRAL_LEVELS, None))],
            [neutral(), gaussian_blur(true, 25.0, false)],
        ),
    ] {
        let (occurrence, omissions) = read(&with_effects(&components));
        assert!(omissions.is_empty(), "{omissions:?}");
        assert_eq!(occurrence.effects, expected);
    }
}

#[test]
fn levels_forms_fx_cannot_hold_omit_the_levels() {
    let with = |index: usize, values: &[u16]| {
        let mut start = NEUTRAL_LEVELS;
        start[index..index + values.len()].copy_from_slice(values);
        start
    };
    let neutral = levels(20, NEUTRAL_LEVELS, None);
    let private = levels_private_data(&NEUTRAL_LEVELS);
    let mut selectors = NEUTRAL_LEVELS;
    selectors[8] = 0;
    selectors[12] = 255;
    for (records, reason) in [
        (levels(20, selectors, None).replace(&levels_private_data(&selectors), &private), "PremiereFilterPrivateData stores 255 for (R) White Output Level, not its StartKeyframe value \"0\""),
        (
            levels(20, selectors, None).replace("</DisplayName>", "</DisplayName><Bypass>true</Bypass>"),
            "Component/Bypass is not supported",
        ),
        // Invalid channel values still reject rather than being silently neutralized.
        (
            levels(20, with(5, &[256]), None),
            "(R) Black Input Level 256 is outside Premiere's 0 to 255 range",
        ),
        (
            levels(20, NEUTRAL_LEVELS, Some((14, "0,100,0,0,0,0,0,0;254016000000,150,0,0,0,0,0,0;"))),
            "keyframed (G) Gamma is not supported; only static values convert",
        ),
        // The private data must repeat every StartKeyframe.
        (
            neutral.replace(&private, &levels_private_data(&NEUTRAL_LEVELS[..19])),
            "PremiereFilterPrivateData has 38 bytes, not 2 for each of the 20 parameters",
        ),
        (
            neutral.replace(&private, &levels_private_data(&with(0, &[4]))),
            "PremiereFilterPrivateData stores 4 for (RGB) Black Input Level, not its StartKeyframe value \"0\"",
        ),
        // Parameters are identified by Name, each with ParameterID -1.
        (
            neutral.replace("<Name>(RGB) Gamma</Name>", ""),
            "unknown parameter \"<no Name>\"",
        ),
        (
            neutral.replace("<Name>(R) Gamma</Name>", "<Name>(G) Gamma</Name>"),
            "duplicate parameter \"(G) Gamma\"",
        ),
        (
            neutral.replace("<Name>(RGB) Gamma</Name>", "<Name>(RGB) Contrast</Name>"),
            "unknown parameter \"(RGB) Contrast\"",
        ),
        (
            neutral.replacen("<ParameterID>-1</ParameterID>", "<ParameterID>1</ParameterID>", 1),
            "ParameterID 1 is not -1",
        ),
        // The bypassed form of a Premiere-native filter is unverified.
        (
            neutral.replace("</DisplayName>", "</DisplayName><Bypass>false</Bypass>"),
            "Component/Bypass is not supported",
        ),
        (
            neutral.replace("</DisplayName>", "</DisplayName><Intrinsic>false</Intrinsic>"),
            "Component/Intrinsic is not supported",
        ),
        // Forms that no Adobe render measured, static or keyed.
        (
            levels(20, with(0, &[100, 100]), None),
            "input black 100 reaches input white 100, a Levels form that no Adobe render measured",
        ),
        (
            levels(20, with(2, &[200, 100]), None),
            "output black 200 exceeds output white 100, a Levels form that no Adobe render measured",
        ),
        (
            levels(20, NEUTRAL_LEVELS, Some((0, "0,0,0,0,0,0,0,0;254016000000,255,0,0,0,0,0,0;"))),
            "input black 255 reaches input white 255, a Levels form that no Adobe render measured",
        ),
    ] {
        let omitted = omitted_reason(records);
        assert!(omitted.ends_with(reason), "{omitted}");
    }
}

#[test]
fn written_levels_rereads_with_its_keys_and_private_data() {
    // A Levels with keyed White Output above a blur.
    let keyed = levels_effect(
        [20.0, 235.0, 16.0, 240.0, 70.0],
        vec![PrEffectParamAnimation {
            param: &LEVELS.params[3],
            keys: PrEffectParamKeys::Scalar(vec![
                key(TICKS / 2, 240.0),
                PrScalarKeyframe {
                    easing: PrKeyframeEasing::Hold,
                    ..key(TICKS, 128.0)
                },
            ]),
        }],
    );
    let effects = vec![keyed, gaussian_blur(true, 10.0, false)];
    let xml = project_xml(&project(effects.clone())).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let text = |node: roxmltree::Node<'_, '_>, tag: &str| {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .map(str::to_owned)
    };
    let root = document.root_element();
    let filter = root
        .children()
        .find(|node| text(*node, "MatchName").as_deref() == Some("PR.ADBE Levels"))
        .unwrap();
    let body = filter
        .children()
        .find(|child| child.has_tag_name("Component"))
        .unwrap();
    // Premiere 26.5.1's form: records 9 and 7 without flags, and private data
    // that repeats the StartKeyframes, the keyed level's first key included.
    assert_eq!(
        [filter.attribute("Version"), body.attribute("Version")],
        [Some("9"), Some("7")]
    );
    assert_eq!(
        [text(body, "Bypass"), text(body, "Intrinsic")],
        [None, None]
    );
    let mut start = NEUTRAL_LEVELS;
    start[..5].copy_from_slice(&[20, 235, 16, 240, 70]);
    assert_eq!(
        text(filter, "PremiereFilterPrivateData"),
        Some(levels_private_data(&start))
    );
    let params: Vec<_> = root
        .children()
        .filter(|node| {
            node.has_tag_name("VideoComponentParam") && node.attribute("Version") == Some("10")
        })
        .map(|param| {
            ["ParameterID", "IsTimeVarying", "StartKeyframe", "Keyframes"]
                .map(|tag| text(param, tag))
        })
        .collect();
    assert_eq!(params.len(), 20);
    assert!(params.iter().all(|[id, ..]| id.as_deref() == Some("-1")));
    // Premiere 26.5.1 marks only the keyed level time-varying.
    assert!(params
        .iter()
        .enumerate()
        .all(|(index, [_, varying, ..])| index == 3 || varying.as_deref() == Some("false")));
    assert_eq!(
        params[3],
        [
            Some("-1".to_owned()),
            Some("true".to_owned()),
            Some("-91445760000000000,240,0,0,0,0,0,0".to_owned()),
            Some("127008000000,240,4,0,0,0,0,0;254016000000,128,0,0,0,0,0,0;".to_owned()),
        ]
    );
    assert_eq!(reread(&xml).effects, effects);
}

fn film_impact_blur(amount: f64, repeat_edge_pixels: bool) -> PrEffect {
    PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::FilmImpactBlur(PrFilmImpactBlur {
            amount,
            repeat_edge_pixels,
        }),
        animations: Vec::new(),
    }
}

const FRAGMENT: &str = include_str!("../../../tests/fixtures/film-impact-gaussian-blur-26.5.1.xml");
const FIXTURE: &str = "feature_film_impact_blur_26_5_derived.prproj";

// Supplementary mutations of one parameter; the original native fragment stays
// unchanged. Native behavior is asserted separately against the saved project.
fn change_parameter(id: &str, from: &str, to: &str) -> String {
    change_parameter_in(FRAGMENT, id, from, to)
}

fn change_parameter_in(fragment: &str, id: &str, from: &str, to: &str) -> String {
    let xml = format!("<PremiereData>{fragment}</PremiereData>");
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let record = doc
        .root_element()
        .children()
        .find(|node| {
            node.children()
                .any(|child| child.has_tag_name("ParameterID") && child.text() == Some(id))
        })
        .unwrap();
    let text = &xml[record.range()];
    assert!(text.contains(from));
    fragment.replace(text, &text.replace(from, to))
}

#[test]
fn film_impact_native_default_reads_its_native_amount() {
    // The version stamp of another release does not change the checked layout.
    for fragment in [
        FRAGMENT.to_owned(),
        change_parameter("8140", ",260501.,", ",260400.,"),
    ] {
        let (clip, omissions) = read(&with_effects(&[(128, fragment)]));
        assert_eq!(clip.effects, [film_impact_blur(20.0, true)]);
        assert!(omissions.is_empty(), "{omissions:?}");
    }
}

#[test]
fn film_impact_saved_fixture_reads_strength_edges_and_literal_key_easing() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(FIXTURE);
    let xml = crate::format::read_xml(&path).unwrap();
    let (project, omissions) =
        inspect_project_with_omissions(&xml, Some("093ea8ea-ef35-4657-a134-f0f1bbd260fb")).unwrap();
    let chart = &project.sequences[0].video_tracks[1];
    assert_eq!(chart.items.len(), 19);
    for (index, amount, repeat) in [
        (1, 0.0, true),
        (2, 5.0, true),
        (3, 20.0, true),
        (4, 50.0, true),
        (5, 100.0, true),
        (6, 20.0, true),
        (7, 20.0, true),
        (8, 20.0, true),
        (12, 50.0, true),
        (13, 50.0, false),
        (15, 20.0, true),
    ] {
        let clip = chart.clip(index);
        assert_eq!(clip.start_ticks, index as i64 * TICKS);
        assert_eq!(clip.effects, [film_impact_blur(amount, repeat)], "C{index}");
    }
    assert!(chart.clip(0).effects.is_empty());
    for index in [9, 10, 11, 14] {
        assert!(chart.clip(index).effects.is_empty(), "C{index}");
    }
    // C9 and C10 (Angle 45) are directional, C11 mirrors and C14 is chromatic.
    let effects: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.record.starts_with("VideoFilterComponent:"))
        .collect();
    assert_eq!(effects.len(), 4, "{effects:?}");
    for (detail, count) in [
        ("cannot represent a directional blur", 2),
        ("Edge Behavior 0 is not supported", 1),
        ("Chromatic Aberration", 1),
    ] {
        assert_eq!(
            effects
                .iter()
                .filter(|omission| omission.reason.contains(detail))
                .count(),
            count,
            "{detail}: {effects:?}"
        );
    }
    for (index, easing) in [(16, PrKeyframeEasing::Linear), (17, PrKeyframeEasing::Hold)] {
        let clip = chart.clip(index);
        assert_eq!(clip.in_ticks, TICKS);
        assert_eq!(clip.out_ticks, 4 * TICKS);
        let effect = &clip.effects[0];
        assert_eq!(effect.params, film_impact_blur(5.0, true).params);
        assert_eq!(effect.animations[0].param, &FILM_IMPACT_BLUR_AMOUNT);
        let keys = effect.animations[0].keys.scalar().unwrap();
        assert_eq!(
            keys.iter()
                .map(|k| (k.source_ticks, k.value))
                .collect::<Vec<_>>(),
            [
                (3 * TICKS / 2, 5.0),
                (5 * TICKS / 2, 50.0),
                (7 * TICKS / 2, 20.0)
            ]
        );
        assert_eq!(keys[0].easing, PrKeyframeEasing::Linear);
        assert_eq!(keys[1].easing, easing);
        assert_eq!(keys[2].easing, easing);
    }
    let effect = &chart.clip(18).effects[0];
    let keys = effect.animations[0].keys.scalar().unwrap();
    for (key, expected_y2) in keys.iter().skip(1).zip([5.0 / 6.0, 59.0 / 60.0]) {
        let PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = key.easing else {
            panic!("Bezier Amount must remain keyed");
        };
        assert_eq!(x1, 1.0 / 6.0);
        assert_eq!(y1, 0.0);
        assert_eq!(x2, 5.0 / 6.0);
        assert!((y2 - expected_y2).abs() < 1e-12);
    }
    // Write edited native Amount keys with the current Film Impact identity.
    let mut edited = effect.clone();
    let PrEffectParamKeys::Scalar(keys) = &mut edited.animations[0].keys else {
        unreachable!();
    };
    keys[1].value = 40.0;
    keys[1].source_ticks = 11 * TICKS / 4;
    let exported = project_xml(&self::project(vec![edited.clone()])).unwrap();
    assert!(exported.contains("<MatchName>AE.Impact_Blur_FX</MatchName>"));
    assert!(!exported.contains("AE.ADBE Gaussian Blur 2"));
    let (reopened, omissions) = inspect_project_with_omissions(&exported, None).unwrap();
    assert_eq!(
        reopened.sequences[0].video_tracks[0].clip(0).effects,
        [edited]
    );
    assert!(omissions.is_empty(), "{omissions:?}");
}

/// The fields and attributes of the effect `match_name` in `xml`: its filter,
/// component and `params` parameter records. Generated object references and
/// the per-chain component ID are left out.
fn native_layout(xml: &str, match_name: &str, params: usize) -> NativeLayout {
    let doc = roxmltree::Document::parse(xml).unwrap();
    let filter = doc
        .descendants()
        .find(|node| node.has_tag_name("MatchName") && node.text() == Some(match_name))
        .unwrap()
        .parent()
        .unwrap();
    let component = filter
        .children()
        .find(|node| node.has_tag_name("Component"))
        .unwrap();
    let attributes = |node: roxmltree::Node<'_, '_>| {
        node.attributes()
            .filter(|attribute| attribute.name() != "ObjectID")
            .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let fields = |node: roxmltree::Node<'_, '_>| {
        node.children()
            .filter(|child| {
                child.is_element()
                    && !matches!(child.tag_name().name(), "Component" | "Params" | "ID")
            })
            .map(|child| {
                (
                    child.tag_name().name().to_owned(),
                    child.text().unwrap_or("").to_owned(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let parameters = component
        .children()
        .find(|node| node.has_tag_name("Params"))
        .unwrap()
        .children()
        .filter(|node| node.has_tag_name("Param"))
        .map(|reference| {
            let id = reference.attribute("ObjectRef").unwrap();
            let parameter = doc
                .descendants()
                .find(|node| node.attribute("ObjectID") == Some(id))
                .unwrap();
            (attributes(parameter), fields(parameter))
        })
        .collect::<Vec<_>>();
    assert_eq!(parameters.len(), params);
    (
        attributes(filter),
        fields(filter),
        attributes(component),
        fields(component),
        parameters,
    )
}

type Fields = std::collections::BTreeMap<String, String>;
type NativeLayout = (Fields, Fields, Fields, Fields, Vec<(Fields, Fields)>);

#[test]
fn film_impact_default_export_matches_the_independent_native_layout() {
    // Compare fields from the Premiere-saved fragment, not the writer's spec.
    // Generated object references and the per-chain component ID may differ.
    let native = format!("<PremiereData>{FRAGMENT}</PremiereData>");
    let (clip, _) = read(&with_effects(&[(128, FRAGMENT.to_owned())]));
    let exported = project_xml(&project(clip.effects)).unwrap();
    let name = "AE.Impact_Blur_FX";
    assert_eq!(
        native_layout(&exported, name, 22),
        native_layout(&native, name, 22)
    );
    assert!(!exported.contains("AE.ADBE Gaussian Blur 2"));
}

#[test]
fn film_impact_unmodeled_controls_and_invalid_amount_omit_only_the_effect() {
    for (id, from, to, reason) in [
        ("6", ",0.,", ",25.,", "Chromatic Aberration"),
        ("8", ",1,", ",0,", "Edge Behavior 0 is not supported"),
        ("3", ",20.,", ",1001.,", "Amount"),
        ("3", ",20.,", ",NaN,", "Amount"),
        ("8100", ",false,", ",true,", "hidden ParameterID 8100"),
        ("9020", ",false,", ",true,", "hidden ParameterID 9020"),
        ("8300", ",0.,", ",42.,", "hidden ParameterID 8300"),
        ("9040", ",-1.,", ",1920.,", "hidden ParameterID 9040"),
        (
            "4",
            "<ParameterID>4</ParameterID>",
            "<IsTimeVarying>true</IsTimeVarying><ParameterID>4</ParameterID>",
            "keyframed Thickness",
        ),
        (
            "2",
            "<ParameterID>2</ParameterID>",
            "<IsTimeVarying>true</IsTimeVarying><ParameterID>2</ParameterID>",
            "keyframed Angle",
        ),
        (
            "3",
            "<ParameterID>3</ParameterID>",
            "<IsTimeVarying>true</IsTimeVarying><ParameterID>3</ParameterID>",
            "empty time-varying Amount",
        ),
    ] {
        let mutant = change_parameter(id, from, to);
        let (clip, omissions) = read(&with_effects(&[(128, mutant), (20, blur(20))]));
        assert_eq!(clip.effects, [gaussian_blur(true, 25.0, false)], "{reason}");
        assert_eq!(omissions.len(), 1, "{reason}: {omissions:?}");
        assert_eq!(omissions[0].scope, OmissionScope::Feature);
        assert_eq!(omissions[0].record, "VideoFilterComponent:128");
        assert!(
            omissions[0].reason.contains(reason),
            "{reason}: {omissions:?}"
        );
    }
}

/// A synthetic 20-parameter Gaussian Blur (`_ Applied Version` 260200): the
/// 26.5.1 record of [`FRAGMENT`] without its hidden ParameterIDs 8300 and
/// 8301, with Amount keyed Linear from 0 at `start` to 2 one second later.
pub(super) fn film_impact_20_parameter_fragment(start: i64) -> String {
    let mut fragment = FRAGMENT.replace(",260501.,", ",260200.,");
    for id in ["181", "182"] {
        let begin = fragment
            .find(&format!("<VideoComponentParam ObjectID=\"{id}\""))
            .unwrap();
        let end = begin + fragment[begin..].find("</VideoComponentParam>").unwrap();
        fragment.replace_range(begin..end + "</VideoComponentParam>".len(), "");
    }
    let (begin, end) = (
        fragment.find("<Params Version=\"1\">").unwrap(),
        fragment.find("</Params>").unwrap(),
    );
    let params: String = (165..=180)
        .chain(183..=186)
        .enumerate()
        .map(|(index, id)| format!("<Param Index=\"{index}\" ObjectRef=\"{id}\"/>"))
        .collect();
    fragment.replace_range(begin..end, &format!("<Params Version=\"1\">{params}"));
    let ramp = format!(
        "<IsTimeVarying>true</IsTimeVarying><StartKeyframe>-91445760000000000,2.,0,0,0,0,0,0</StartKeyframe><Keyframes>{start},0.,0,0,0,0,0,0;{},2.,0,0,0,0,0,0;</Keyframes>",
        start + TICKS
    );
    change_parameter_in(
        &fragment,
        "3",
        "<StartKeyframe>-91445760000000000,20.,0,0,0,0,0,0</StartKeyframe>",
        &ramp,
    )
}

#[test]
fn film_impact_20_parameter_layout_reads_its_keyed_amount() {
    let fragment = film_impact_20_parameter_fragment(TICKS);
    assert_eq!(fragment.matches("<Param Index=").count(), 20);
    // The helper writes the 26.2 stamp as its own literal, not from the
    // layout defaults. The stamp gates no visual control, so the 26.5.1
    // stamp in the same layout reads the same blur.
    for stamp in [",260200.,", ",260501.,"] {
        let fragment = change_parameter_in(&fragment, "8140", ",260200.,", stamp);
        assert_eq!(fragment.matches(stamp).count(), 1, "{stamp}");
        let (clip, omissions) = read(&with_effects(&[(128, fragment)]));
        assert!(omissions.is_empty(), "{omissions:?}");
        let [effect] = clip.effects.as_slice() else {
            panic!("one effect: {:?}", clip.effects);
        };
        assert_eq!(
            effect.params,
            // A keyed Amount starts at its first key.
            film_impact_blur(0.0, true).params
        );
        assert!(effect.enabled);
        let [animation] = effect.animations.as_slice() else {
            panic!("one animation: {:?}", effect.animations);
        };
        assert_eq!(animation.param, &FILM_IMPACT_BLUR_AMOUNT);
        assert_eq!(
            animation.keys.scalar().unwrap(),
            [key(TICKS, 0.0), key(2 * TICKS, 2.0)]
        );
    }
}

#[test]
fn film_impact_20_parameter_layout_keeps_identity_and_hidden_value_checks() {
    for (id, from, to, reason) in [
        // A 26.5.1 hidden parameter is no parameter of the 26.2 layout;
        // numeric parameter identities must remain unique.
        (
            "9020",
            "<ParameterID>9020</ParameterID>",
            "<ParameterID>8300</ParameterID>",
            "unknown ParameterID 8300",
        ),
        (
            "9042",
            "<ParameterID>9042</ParameterID>",
            "<ParameterID>9041</ParameterID>",
            "duplicate ParameterID 9041",
        ),
        ("8100", ",false,", ",true,", "hidden ParameterID 8100"),
        ("9040", ",-1.,", ",1920.,", "hidden ParameterID 9040"),
        ("6", ",0.,", ",25.,", "Chromatic Aberration"),
        ("8", ",1,", ",0,", "Edge Behavior 0 is not supported"),
    ] {
        let mutant = change_parameter_in(&film_impact_20_parameter_fragment(0), id, from, to);
        let (clip, omissions) = read(&with_effects(&[(128, mutant), (20, blur(20))]));
        assert_eq!(clip.effects, [gaussian_blur(true, 25.0, false)], "{reason}");
        assert_eq!(omissions.len(), 1, "{reason}: {omissions:?}");
        assert_eq!(omissions[0].record, "VideoFilterComponent:128");
        assert!(
            omissions[0].reason.contains(reason),
            "{reason}: {omissions:?}"
        );
    }
    // Twenty-one parameters is neither layout.
    let fragment = film_impact_20_parameter_fragment(0);
    let extra = fragment.replace(
        "<Param Index=\"19\" ObjectRef=\"186\"/>",
        "<Param Index=\"19\" ObjectRef=\"186\"/><Param Index=\"20\" ObjectRef=\"186\"/>",
    );
    assert_ne!(extra, fragment);
    let (clip, omissions) = read(&with_effects(&[(128, extra)]));
    assert!(clip.effects.is_empty());
    assert!(
        omissions[0]
            .reason
            .contains("expected 22 parameters, found 21"),
        "{omissions:?}"
    );
}

const DIRECTIONAL_FRAGMENT: &str =
    include_str!("../../../tests/fixtures/film-impact-directional-blur-26.5.1.xml");

fn film_impact_directional_blur(angle: f64, amount: f64) -> PrEffect {
    PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::FilmImpactDirectionalBlur(PrFilmImpactDirectionalBlur {
            angle,
            amount,
        }),
        animations: Vec::new(),
    }
}

/// The saved default record with the Legacy blur's transparent exterior.
fn transparent_directional_fragment() -> String {
    change_parameter_in(DIRECTIONAL_FRAGMENT, "8", ",1,", ",2,")
}

#[test]
fn film_impact_directional_blur_reads_its_angle_and_amount_with_repeated_or_transparent_edges() {
    for fragment in [
        DIRECTIONAL_FRAGMENT.to_owned(),
        transparent_directional_fragment(),
    ] {
        let (clip, omissions) = read(&with_effects(&[(180, fragment)]));
        assert_eq!(clip.effects, [film_impact_directional_blur(0.0, 35.0)]);
        assert!(omissions.is_empty(), "{omissions:?}");
    }
}

#[test]
fn film_impact_directional_export_matches_the_independent_native_layout() {
    // The saved default with Edge Behavior 2, which export writes.
    let fragment = transparent_directional_fragment();
    let native = format!("<PremiereData>{fragment}</PremiereData>");
    let exported = project_xml(&project(vec![film_impact_directional_blur(0.0, 35.0)])).unwrap();
    let name = "AE.Impact_Directional_Blur_FX";
    assert_eq!(
        native_layout(&exported, name, 20),
        native_layout(&native, name, 20)
    );
    assert!(!exported.contains("AE.ADBE Motion Blur"));
}

#[test]
fn film_impact_directional_saved_probe_imports_repeated_and_transparent_edges() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_film_impact_directional_blur_26_5.prproj");
    let xml = crate::format::read_xml(&path).unwrap();
    let (project, omissions) =
        inspect_project_with_omissions(&xml, Some("8b7c0e2e-8497-413f-b0f6-427efd2b3194")).unwrap();
    let chart = &project.sequences[0].video_tracks[1];
    assert_eq!(chart.items.len(), 20);
    // Premiere's default repeated edges, except C8's transparent exterior. C7
    // is at Scale 50 and Rotation 30, and C11's Seed 1234 is not kept.
    for (index, angle, amount) in [
        (2, 0.0, 10.0),
        (3, 0.0, 35.0),
        (4, 0.0, 100.0),
        (5, 90.0, 35.0),
        (6, 30.0, 35.0),
        (7, 0.0, 35.0),
        (8, 0.0, 35.0),
        (11, 0.0, 35.0),
    ] {
        let clip = chart.clip(index);
        assert_eq!(clip.start_ticks, index as i64 * TICKS);
        assert_eq!(
            clip.effects,
            [film_impact_directional_blur(angle, amount)],
            "C{index}"
        );
    }
    // C0 holds Angle 0 from 0.2 s to its key 90 at 0.6 s and keys Amount 10 to
    // 100 (Linear) at 0.2 and 0.8 s. A key has the easing of the segment into it.
    let [effect] = chart.clip(0).effects.as_slice() else {
        panic!("expected one effect on C0: {:?}", chart.clip(0).effects);
    };
    assert_eq!(
        effect.params,
        film_impact_directional_blur(0.0, 10.0).params
    );
    let tracks: Vec<_> = effect
        .animations
        .iter()
        .map(|animation| {
            let keys = animation.keys.scalar().unwrap().iter();
            let keys = keys.map(|key| (key.source_ticks, key.value, key.easing));
            (animation.param.id, keys.collect::<Vec<_>>())
        })
        .collect();
    assert_eq!(
        tracks,
        [
            (
                2,
                vec![
                    (TICKS / 5, 0.0, PrKeyframeEasing::Linear),
                    (3 * TICKS / 5, 90.0, PrKeyframeEasing::Hold)
                ]
            ),
            (
                3,
                vec![
                    (TICKS / 5, 10.0, PrKeyframeEasing::Linear),
                    (4 * TICKS / 5, 100.0, PrKeyframeEasing::Linear)
                ]
            ),
        ]
    );
    // C12 to C19 are Legacy.
    for (index, direction, blur_length) in [
        (12, 0.0, 10.0),
        (13, 0.0, 35.0),
        (14, 0.0, 100.0),
        (15, 30.0, 35.0),
        (16, -30.0, 35.0),
        (17, 0.0, 35.0),
        (18, -30.0, 35.0),
        (19, 0.0, 55.0),
    ] {
        let clip = chart.clip(index);
        assert_eq!(clip.start_ticks, index as i64 * TICKS);
        assert_eq!(
            clip.effects,
            [directional_blur(true, direction, blur_length)],
            "C{index}"
        );
    }
    // C1 has no effect, C9 mirrors edges and C10 is chromatic.
    for index in [1, 9, 10] {
        assert!(chart.clip(index).effects.is_empty(), "C{index}");
    }
    let effects: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.record.starts_with("VideoFilterComponent:"))
        .collect();
    assert_eq!(effects.len(), 2, "{effects:?}");
    for (detail, count) in [
        ("Edge Behavior 0 is not supported", 1),
        ("Chromatic Aberration", 1),
    ] {
        assert_eq!(
            effects
                .iter()
                .filter(|omission| omission.reason.contains(detail))
                .count(),
            count,
            "{detail}: {effects:?}"
        );
    }
}

#[test]
fn film_impact_directional_angle_and_amount_keys_read_as_their_tracks() {
    let keys = "0,10.,0,0,0,0.16666666666666666,0,0.16666666666666666;254016000000,90.,0,0,0,0.16666666666666666,0,0.16666666666666666;";
    let mut fragment = DIRECTIONAL_FRAGMENT.to_owned();
    for id in ["2", "3"] {
        let parameter = format!("<ParameterID>{id}</ParameterID>");
        let varying = format!("<IsTimeVarying>true</IsTimeVarying>{parameter}");
        fragment = change_parameter_in(&fragment, id, &parameter, &varying);
        let keyframes = format!("</StartKeyframe><Keyframes>{keys}</Keyframes>");
        fragment = change_parameter_in(&fragment, id, "</StartKeyframe>", &keyframes);
    }
    let (clip, omissions) = read(&with_effects(&[(180, fragment)]));
    assert!(omissions.is_empty(), "{omissions:?}");
    let [effect] = clip.effects.as_slice() else {
        panic!("expected one effect: {:?}", clip.effects);
    };
    // A keyed parameter starts at its first key.
    assert_eq!(
        effect.params,
        film_impact_directional_blur(10.0, 10.0).params
    );
    let tracks: Vec<_> = effect
        .animations
        .iter()
        .map(|animation| {
            let values = animation.keys.scalar().unwrap().iter().map(|key| key.value);
            (animation.param.id, values.collect::<Vec<_>>())
        })
        .collect();
    assert_eq!(tracks, [(2, vec![10.0, 90.0]), (3, vec![10.0, 90.0])]);
}

#[test]
fn film_impact_directional_unmodeled_controls_and_invalid_amount_omit_only_the_effect() {
    for (id, from, to, reason) in [
        ("6", ",0.,", ",25.,", "Chromatic Aberration"),
        ("8", ",1,", ",0,", "Edge Behavior 0 is not supported"),
        ("3", ",35.,", ",1001.,", "Amount"),
        ("8100", ",false,", ",true,", "hidden ParameterID 8100"),
        ("9040", ",-1.,", ",1920.,", "hidden ParameterID 9040"),
        (
            "8041",
            "<ParameterID>8041</ParameterID>",
            "<IsTimeVarying>true</IsTimeVarying><ParameterID>8041</ParameterID>",
            "keyframed Seed",
        ),
    ] {
        let mutant = change_parameter_in(DIRECTIONAL_FRAGMENT, id, from, to);
        let (clip, omissions) = read(&with_effects(&[(180, mutant), (20, blur(20))]));
        assert_eq!(clip.effects, [gaussian_blur(true, 25.0, false)], "{reason}");
        assert_eq!(omissions.len(), 1, "{reason}: {omissions:?}");
        assert_eq!(omissions[0].scope, OmissionScope::Feature);
        assert_eq!(omissions[0].record, "VideoFilterComponent:180");
        assert!(
            omissions[0].reason.contains(reason),
            "{reason}: {omissions:?}"
        );
    }
}

/// 5,000 keys, above the 4,096 that the schema once allowed, a millisecond
/// apart: `key(index)` at each time.
fn past_the_former_limit<T>(key: impl Fn(i64, i64) -> T) -> Vec<T> {
    (0..5000)
        .map(|index| key(index * TICKS / 1000, index))
        .collect()
}

#[test]
fn written_keys_past_the_former_limit_reread_with_every_key() {
    let linear = PrKeyframeEasing::Linear;
    let scalar = past_the_former_limit(|source_ticks, index| PrScalarKeyframe {
        easing: if index % 7 == 6 {
            PrKeyframeEasing::Hold
        } else {
            linear
        },
        ..key(source_ticks, (index % 100) as f64)
    });
    // End of Ramp along the ramp's axis.
    let point = past_the_former_limit(|source_ticks, index| {
        point_key(
            source_ticks,
            [0.5, if index % 2 == 0 { 1.0 } else { 0.6 }],
            linear,
        )
    });
    let colour = past_the_former_limit(|source_ticks, index| {
        let rgb = if index % 2 == 0 {
            [255, 255, 255]
        } else {
            [0, 128, 255]
        };
        colour_key(source_ticks, rgb, linear)
    });
    for (case, effect) in [
        (
            "scalar",
            with_blurriness_keys(gaussian_blur(true, 0.0, false), scalar.clone()),
        ),
        (
            "point",
            ramp_effect(
                true,
                ([0.5, 0.0], [0.5, 1.0]),
                ([0, 0, 0], [255, 255, 255]),
                0.0,
                vec![PrEffectParamAnimation {
                    param: &RAMP_END,
                    keys: PrEffectParamKeys::Point(point),
                }],
            ),
        ),
        (
            "colour",
            tint_effect(
                true,
                ([0, 0, 0], [255, 255, 255], 100.0),
                (vec![], colour, vec![]),
            ),
        ),
    ] {
        let effects = vec![effect];
        let xml = project_xml(&project(effects.clone())).unwrap();
        assert_eq!(reread(&xml).effects, effects, "{case}");
    }
    // A Linear Wipe's completion keys, 0 to 100.
    let mut written = project(Vec::new());
    let wipe = crate::schema::PrLinearWipe {
        initial_completion: 0.0,
        completion: scalar,
        angle_degrees: 90,
        feather: 0.0,
    };
    written.sequences[0].video_tracks[0].clip_mut(0).linear_wipe = Some(wipe.clone());
    let xml = project_xml(&written).unwrap();
    let read = reread(&xml).linear_wipe.unwrap();
    assert_eq!(read.completion, wipe.completion);
    assert_eq!(
        (read.initial_completion, read.angle_degrees, read.feather),
        (0.0, 90, 0.0)
    );
}

/// The FX documents that the public file API imports from `xml`, with
/// `video-30fps-10s.mp4` as the media of its clip, and reimports from their
/// Premiere export. No step omits anything.
#[cfg(feature = "ffmpeg-library")]
fn public_round_trip(xml: &str) -> [serde_json::Value; 2] {
    public_round_trip_checked(xml, |notes| assert!(notes.is_empty(), "{notes:?}"))
}

#[cfg(feature = "ffmpeg-library")]
fn public_round_trip_checked(
    xml: &str,
    check_notes: impl Fn(&[Omission]),
) -> [serde_json::Value; 2] {
    use std::path::Path;
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("media")).unwrap();
    std::fs::write(
        directory.path().join("media/source.mp4"),
        include_bytes!("../../../tests/fixtures/video-30fps-10s.mp4"),
    )
    .unwrap();
    let project = directory.path().join("project.prproj");
    crate::test_support::write_prproj(&project, xml);
    let archive = |output: &Path| {
        std::fs::read_dir(output)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "tsrct")
            })
            .unwrap()
    };
    let document = |output: &Path| {
        tesseract_file::TesseractFile::open(archive(output))
            .unwrap()
            .project_json()
            .unwrap()
    };
    let imported = directory.path().join("imported");
    let omissions =
        crate::premiere_to_tesseract(&project, &imported, Some("sequence-1"), false).unwrap();
    check_notes(&omissions);
    let exported = directory.path().join("exported");
    let omissions = crate::tesseract_to_premiere(archive(&imported), &exported, false).unwrap();
    check_notes(&omissions);
    let reimported = directory.path().join("reimported");
    let omissions =
        crate::premiere_to_tesseract(exported.join("project.prproj"), &reimported, None, false)
            .unwrap();
    check_notes(&omissions);
    [document(&imported), document(&reimported)]
}

/// The key tracks of an FX document, each named by its effect parameter or
/// else by the layer property that it keys: the layer time and value of each
/// key.
#[cfg(feature = "ffmpeg-library")]
fn key_tracks(document: &serde_json::Value) -> std::collections::BTreeMap<String, Vec<(i64, f64)>> {
    let mut tracks = std::collections::BTreeMap::new();
    for entry in document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
    {
        let target = &entry["target"];
        let name = target["paramName"]
            .as_str()
            .or_else(|| target["propertyType"].as_str())
            .unwrap();
        let keys = entry["animator"]["keyframes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|key| {
                (
                    key["layerTime"].as_i64().unwrap(),
                    key["value"]["value"].as_f64().unwrap(),
                )
            })
            .collect();
        assert!(tracks.insert(name.to_owned(), keys).is_none(), "{name}");
    }
    tracks
}

/// The FX track `name` of [`past_the_former_limit`] keys: `value(index)` at
/// `index` ms.
#[cfg(feature = "ffmpeg-library")]
fn track_past_the_former_limit(
    name: &str,
    value: impl Fn(i64) -> f64,
) -> (String, Vec<(i64, f64)>) {
    (
        name.to_owned(),
        past_the_former_limit(|_, index| (index, value(index))),
    )
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn keyed_blurriness_past_the_former_limit_converts_exports_and_reimports_every_key() {
    // 5,000 Blurriness keys a millisecond apart over the 5 s clip.
    let keys: String = (0..5000)
        .map(|index| {
            format!(
                "{},{}.,0,0,0,0.16666666666666666,0,0.16666666666666666;",
                index * TICKS / 1000,
                index % 100
            )
        })
        .collect();
    let blur = keyed_blur(
        20,
        &format!("<ParameterControlType>8</ParameterControlType><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><Keyframes>{keys}</Keyframes>"),
    );
    let expected =
        std::collections::BTreeMap::from([track_past_the_former_limit("blurriness", |index| {
            (index % 100) as f64
        })]);
    for document in public_round_trip(&with_effects(&[(20, blur)])) {
        assert_eq!(key_tracks(&document), expected);
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn keyed_ramp_end_past_the_former_limit_converts_exports_and_reimports_every_key() {
    // A vertical Ramp whose End of Ramp keeps x 0.5 and alternates y 1 and 0.6.
    let y = |index: i64| if index % 2 == 0 { 1.0 } else { 0.6 };
    let keys = past_the_former_limit(|ticks, index| {
        format!(
            "{ticks},0.5:{},0,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;",
            y(index)
        )
    })
    .concat();
    let ramp = ramp_26_5_xml(
        20,
        RampXml {
            end: ("0.5:1", &keys),
            ..DEFAULT_RAMP
        },
    );
    let expected = std::collections::BTreeMap::from([
        track_past_the_former_limit("endX", |_| 0.5),
        track_past_the_former_limit("endY", y),
    ]);
    for document in public_round_trip(&with_effects(&[(20, ramp)])) {
        assert_eq!(key_tracks(&document), expected);
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn keyed_tint_colour_past_the_former_limit_converts_exports_and_reimports_every_key() {
    // A Tint whose Map White To alternates opaque white and (0, 128, 255), as
    // three FX channel tracks of shares of 255.
    let rgb = |index: i64| {
        if index % 2 == 0 {
            [255, 255, 255]
        } else {
            [0, 128, 255]
        }
    };
    let native = |[r, g, b]: [u8; 3]| {
        (0xff00_u64 << 48 | u64::from(r) << 40 | u64::from(g) << 24 | u64::from(b) << 8).to_string()
    };
    let keys = past_the_former_limit(|ticks, index| {
        format!(
            "{ticks},{},0,0,0,0.16666666666666666,0,0.16666666666666666;",
            native(rgb(index))
        )
    })
    .concat();
    let white = native([255, 255, 255]);
    let tint = tint_26_5_xml(
        20,
        [(TINT_DEFAULT_BLACK, ""), (&white, &keys), ("100.", "")],
    );
    let expected: std::collections::BTreeMap<_, _> = ["whiteR", "whiteG", "whiteB"]
        .into_iter()
        .enumerate()
        .map(|(channel, name)| {
            track_past_the_former_limit(name, |index| f64::from(rgb(index)[channel]) / 255.0)
        })
        .collect();
    for document in public_round_trip(&with_effects(&[(20, tint)])) {
        assert_eq!(key_tracks(&document), expected);
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn keyed_linear_wipe_completion_past_the_former_limit_converts_exports_and_reimports_every_key() {
    // The wipe at 270 degrees keys its guide's Scale X, 100 minus completion.
    let keys = past_the_former_limit(|ticks, index| {
        format!(
            "{ticks},{}.,0,0,0,0.16666666666666666,0,0.16666666666666666;",
            index % 100
        )
    })
    .concat();
    let native = "<Keyframes>0,100,0,0,0,0,0,0;254016000000,0,0,0,0,0,0,0;</Keyframes>";
    let wipe = adobe_linear_wipe().replace(native, &format!("<Keyframes>{keys}</Keyframes>"));
    assert_ne!(wipe, adobe_linear_wipe());
    let expected =
        std::collections::BTreeMap::from([track_past_the_former_limit("scaleX", |index| {
            (100 - index % 100) as f64
        })]);
    for document in public_round_trip(&with_effects(&[(154, wipe)])) {
        assert_eq!(key_tracks(&document), expected);
    }
}

#[test]
fn native_geometry2_adjustment_zoom_keeps_editable_corner_keys() {
    let native = include_str!("../../../tests/fixtures/cap2-native-geometry2.xml");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_adjustment_layer_26_5_strict.prproj");
    let source = crate::format::read_xml(&path).unwrap();
    let dom = roxmltree::Document::parse(&source).unwrap();
    let object = |id| {
        let node = dom
            .root_element()
            .children()
            .find(|node| node.attribute("ObjectID") == Some(id))
            .unwrap();
        &source[node.range()]
    };
    let chain = object("121");
    let clip = object("154");
    let item = object("98");
    let xml = source
        .replace(
            chain,
            &chain.replace("ObjectRef=\"153\"", "ObjectRef=\"407\""),
        )
        .replace(
            clip,
            &clip
                .replace("914457600000000", "914450328792000")
                .replace("915219648000000", "914905895904000"),
        )
        .replace(
            item,
            &item.replace(
                "<End>762048000000</End>",
                "<Start>105945840000</Start><End>561512952000</End>",
            ),
        )
        .replace(
            "</PremiereData>",
            &native.replace("<PremiereData Version=\"3\">", ""),
        )
        .replace(
            "<FrameRate>8467200000</FrameRate>",
            "<FrameRate>10594584000</FrameRate>",
        );
    let (project, mut omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    let zoom = project.sequences[0]
        .video_occurrences()
        .find(|clip| clip.id.as_deref() == Some("VideoClipTrackItem:98"))
        .unwrap();
    assert_eq!(zoom.effects.len(), 1);
    assert!(matches!(
        zoom.effects[0].params,
        PrEffectParams::CornerPin(_)
    ));
    assert_eq!(zoom.effects[0].animations.len(), 4);
    let keys = zoom.effects[0].animations[0].keys.point().unwrap();
    assert_eq!(
        keys.iter().map(|key| key.source_ticks).collect::<Vec<_>>(),
        [914513896296000, 914545680048000]
    );
    assert_eq!(keys[0].value, [0.0; 2]);
    assert!((keys[1].value[0] + 0.09).abs() < 1e-12);
    assert_eq!(keys[0].easing, PrKeyframeEasing::Linear);
    assert_eq!(
        keys[1].easing,
        PrKeyframeEasing::CubicBezier {
            x1: 1.0 / 3.0,
            y1: 0.0,
            x2: 1.0 - 1.0 / 3.0,
            y2: 1.0
        }
    );
    let (sequences, media) = project.into_parts();
    let sequence = &sequences[0];
    let document = crate::convert::premiere_to_tesseract(
        sequence,
        &media,
        &crate::tesseract_output::asset_ids_in_order(sequence, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.record == "VideoFilterComponent:407"),
        "{omissions:?}"
    );
    let layer = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Adjustment" && layer["activeRange"]["start"] == 417)
        .unwrap();
    assert_eq!(layer["effects"][0]["effect"]["type"], "cornerPin");
    assert_eq!(layer["effects"][0]["effect"]["upperLeftX"], 0.0);
    let entries: Vec<_> = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["target"]["effectId"] == layer["effects"][0]["id"])
        .collect();
    assert_eq!(entries.len(), 8);
    for entry in entries {
        let keys = entry["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(
            keys.iter()
                .map(|key| key["layerTime"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            [250, 375]
        );
        assert_eq!(keys[0]["easing"]["type"], "linear");
        assert_eq!(
            keys[1]["easing"],
            serde_json::json!({"type":"cubicBezier", "x1":1.0/3.0, "y1":0.0, "x2":1.0-1.0/3.0, "y2":1.0})
        );
        let name = entry["target"]["paramName"].as_str().unwrap();
        let upper_or_left = matches!(
            name,
            "upperLeftX" | "upperLeftY" | "upperRightY" | "lowerLeftX"
        );
        let (start, end) = if upper_or_left {
            (0.0, -0.09)
        } else {
            (1.0, 1.09)
        };
        assert_eq!(keys[0]["value"]["value"], start);
        assert!((keys[1]["value"]["value"].as_f64().unwrap() - end).abs() < 1e-12);
    }
}

#[test]
fn native_geometry2_keeps_picture_and_motion_with_a_stale_static_current_value() {
    let native = include_str!("../../../tests/fixtures/cap2-native-geometry2.xml");
    let skew_name = "<Name>Skew</Name>";
    assert_eq!(native.matches(skew_name).count(), 1);
    let records = native
        .replace("<PremiereData Version=\"3\">", "")
        .replace("</PremiereData>", "")
        .replace("ObjectID=\"407\"", "ObjectID=\"20\"")
        .replace(
            skew_name,
            &format!("{skew_name}<CurrentValue>7.4576416015625</CurrentValue>"),
        );
    let xml = with_effects(&[(20, records)]);
    let (occurrence, omissions) = read(&xml);
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(occurrence.media.as_str(), "Media:ObjectUID:media-1");
    let [PrEffect {
        params: PrEffectParams::CornerPin(_),
        animations,
        ..
    }] = occurrence.effects.as_slice()
    else {
        panic!("{:?}", occurrence.effects);
    };
    assert_eq!(animations.len(), 4);

    let document = import(&xml);
    let (_, video) = masked_and_video(&document);
    assert_eq!(video["type"], "Video");
    assert_eq!(video["source"]["assetId"], "premiere-video-1");
    assert_eq!(video["effects"][0]["effect"]["type"], "cornerPin");
    assert_eq!(
        document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .len(),
        8
    );
}

#[test]
fn geometry2_width_keys_keep_independent_height_terminal_collapse_and_report_blur() {
    const FRAME: i64 = TICKS / 25;
    let width_keys = format!(
        "0,100.,5,0,0,0,-750,0.33333333333333331;\
         {FRAME},70.,5,0,-750,0.33333333333333331,375,0.33333333333333331;\
         {},85.,5,0,375,0.33333333333333331,-2125,0.33333333333333331;\
         {},0.,0,0,-2125,0.33333333333333331,0,0;",
        2 * FRAME,
        3 * FRAME
    );
    let mut values = DEFAULT_TRANSFORM;
    values[3] = ("100.", "");
    values[4] = ("100.", &width_keys);
    values[9] = ("true", "");
    values[10] = ("360.", "");
    let records = transform_26_5_xml(20, values).replace("AE.ADBE Geometry", "AE.ADBE Geometry2");
    let xml = with_effects(&[(20, records)]);
    let (occurrence, omissions) = read(&xml);
    let [omission] = omissions.as_slice() else {
        panic!("{omissions:?}");
    };
    assert_eq!(omission.kind, OmissionKind::Approximated);
    assert_eq!(omission.record, "VideoFilterComponent:20");
    assert!(
        omission.reason.contains("saved Shutter Angle 360")
            && omission
                .reason
                .contains("Use Composition's Shutter Angle is enabled")
            && omission.reason.contains("forward one-frame smear"),
        "{omission:?}"
    );
    let [PrEffect {
        params: PrEffectParams::CornerPin(pin),
        animations,
        ..
    }] = occurrence.effects.as_slice()
    else {
        panic!("{:?}", occurrence.effects);
    };
    assert_eq!(
        pin.corners,
        [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]
    );
    assert_eq!(animations.len(), 4);
    let widths = [100.0, 70.0, 85.0, 0.0];
    for (corner_index, animation) in animations.iter().enumerate() {
        let keys = animation.keys.point().unwrap();
        assert_eq!(
            keys.iter().map(|key| key.source_ticks).collect::<Vec<_>>(),
            [0, FRAME, 2 * FRAME, 3 * FRAME]
        );
        assert_eq!(keys[0].easing, PrKeyframeEasing::Linear);
        for key in &keys[1..] {
            let PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = key.easing else {
                panic!("{:?}", key.easing);
            };
            for (actual, expected) in [
                (x1, 1.0 / 3.0),
                (y1, 1.0 / 3.0),
                (x2, 2.0 / 3.0),
                (y2, 2.0 / 3.0),
            ] {
                assert!((actual - expected).abs() < 1e-12, "{actual} {expected}");
            }
        }
        for (key, width) in keys.iter().zip(widths) {
            let expected_x = if corner_index % 2 == 0 {
                0.5 - width / 200.0
            } else {
                0.5 + width / 200.0
            };
            let expected_y = if corner_index < 2 { 0.0 } else { 1.0 };
            assert_eq!(key.value, [expected_x, expected_y]);
        }
    }

    let document = import(&xml);
    let (_, video) = masked_and_video(&document);
    assert_eq!(video["source"]["assetId"], "premiere-video-1");
    assert_eq!(video["effects"][0]["effect"]["type"], "cornerPin");
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 8);
    for entry in entries {
        let keys = entry["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(
            keys.iter()
                .map(|key| key["layerTime"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            [0, 40, 80, 120]
        );
    }
}

#[test]
fn geometry2_keyed_shutter_reports_one_approximation_and_keeps_scale() {
    const FRAME: i64 = TICKS / 25;
    let shutter_keys = format!("0,0.,0,0,0,0,0,0;{FRAME},180.,0,0,0,0,0,0;");
    let mut values = DEFAULT_TRANSFORM;
    values[10] = ("0.", &shutter_keys);
    let records = transform_26_5_xml(20, values).replace("AE.ADBE Geometry", "AE.ADBE Geometry2");
    let (occurrence, omissions) = read(&with_effects(&[(20, records)]));
    assert!(matches!(
        occurrence.effects.as_slice(),
        [PrEffect {
            params: PrEffectParams::CornerPin(_),
            ..
        }]
    ));
    let [omission] = omissions.as_slice() else {
        panic!("{omissions:?}");
    };
    assert_eq!(omission.kind, OmissionKind::Approximated);
    assert!(
        omission
            .reason
            .contains("keyed Geometry2 motion blur (saved Shutter Angle range 0 to 180"),
        "{omission:?}"
    );
}

#[test]
fn geometry2_uniform_scale_keeps_height_keys_and_reports_inert_width_keys() {
    const FRAME: i64 = TICKS / 25;
    let height_keys = format!("0,100.,0,0,0,0,0,0;{FRAME},80.,0,0,0,0,0,0;");
    let width_keys = format!("0,100.,0,0,0,0,0,0;{FRAME},0.,0,0,0,0,0,0;");
    let mut values = DEFAULT_TRANSFORM;
    values[2] = ("true", "");
    values[3] = ("100.", &height_keys);
    values[4] = ("100.", &width_keys);
    let records = transform_26_5_xml(20, values).replace("AE.ADBE Geometry", "AE.ADBE Geometry2");
    let (occurrence, omissions) = read(&with_effects(&[(20, records)]));
    let [omission] = omissions.as_slice() else {
        panic!("{omissions:?}");
    };
    assert_eq!(omission.kind, OmissionKind::Omitted);
    assert!(
        omission
            .reason
            .contains("Scale Width keys under Uniform Scale were not imported"),
        "{omission:?}"
    );
    let [PrEffect {
        params: PrEffectParams::CornerPin(_),
        animations,
        ..
    }] = occurrence.effects.as_slice()
    else {
        panic!("{:?}", occurrence.effects);
    };
    assert_eq!(animations.len(), 4);
    for (corner_index, animation) in animations.iter().enumerate() {
        let keys = animation.keys.point().unwrap();
        assert_eq!(keys.len(), 2);
        let expected = if corner_index == 0 {
            [0.1, 0.1]
        } else if corner_index == 1 {
            [0.9, 0.1]
        } else if corner_index == 2 {
            [0.1, 0.9]
        } else {
            [0.9, 0.9]
        };
        assert!(
            keys[1]
                .value
                .iter()
                .zip(expected)
                .all(|(actual, expected)| (actual - expected).abs() < 1e-12),
            "{:?} {expected:?}",
            keys[1].value
        );
    }
}

#[test]
fn masked_geometry2_admission_follows_its_clip_frame() {
    let masked_geometry = |values| {
        transform_26_5_xml(20, values)
            .replace("AE.ADBE Geometry", "AE.ADBE Geometry2")
            .replace(
                "</Component><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Geometry2</MatchName>",
                "</Component><SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"40\"/></SubComponents><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Geometry2</MatchName>",
            ) + &super::mask::mask(40, true)
    };
    let mut off_centre = DEFAULT_TRANSFORM;
    off_centre[0] = ("0.4:0.5", "");
    off_centre[1] = ("0.6:0.5", "");
    off_centre[2] = ("true", "");
    off_centre[3] = ("50.", "");

    // An unrotated sequence-sized owner uses the canvas reader. Keep its
    // off-centre masked Geometry2 and independent Tint through conversion.
    let xml = with_effects(&[(20, masked_geometry(off_centre)), (60, tint(60))]);
    let (occurrence, omissions) = read(&xml);
    assert!(omissions.is_empty(), "{omissions:?}");
    let [PrEffect {
        params: PrEffectParams::Tint(_),
        mask: None,
        ..
    }, PrEffect {
        params: PrEffectParams::CornerPin(pin),
        mask: Some(_),
        ..
    }] = occurrence.effects.as_slice()
    else {
        panic!("{:?}", occurrence.effects);
    };
    for (corner, expected) in
        pin.corners
            .iter()
            .zip([[0.4, 0.25], [0.9, 0.25], [0.4, 0.75], [0.9, 0.75]])
    {
        assert!(
            corner
                .iter()
                .zip(expected)
                .all(|(actual, expected)| (actual - expected).abs() < 1e-12),
            "{corner:?} {expected:?}"
        );
    }
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = &project.sequences[0];
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    let mut omissions = Vec::new();
    let document =
        crate::convert::premiere_to_tesseract(sequence, &project.media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
    assert!(
        omissions
            .iter()
            .all(|omission| omission.kind == OmissionKind::Approximated),
        "{omissions:?}"
    );
    assert!(
        omissions.iter().any(|omission| omission
            .reason
            .contains("effect mask retained on an isolated editable adjustment")),
        "{omissions:?}"
    );
    let scope = &document["composition"]["layers"][0];
    assert_eq!(scope["type"], "Group");
    let children = scope["layers"].as_array().unwrap();
    let video = children
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    assert_eq!(video["effects"][0]["effect"]["type"], "tintTritone");
    let masked = children
        .iter()
        .find(|layer| layer["name"] == "Premiere masked effect")
        .unwrap();
    assert_eq!(masked["type"], "Adjustment");
    assert_eq!(masked["effects"][0]["effect"]["type"], "cornerPin");
    assert_eq!(masked["masks"].as_array().unwrap().len(), 1);
    for (field, expected) in [
        ("upperLeftX", 0.4),
        ("upperLeftY", 0.25),
        ("upperRightX", 0.9),
        ("upperRightY", 0.25),
        ("lowerLeftX", 0.4),
        ("lowerLeftY", 0.75),
        ("lowerRightX", 0.9),
        ("lowerRightY", 0.75),
    ] {
        let actual = masked["effects"][0]["effect"][field].as_f64().unwrap();
        assert!((actual - expected).abs() < 1e-12, "{field}: {actual}");
    }

    let stream = "<FrameRate>8467200000</FrameRate><FrameRect>0,0,1920,1080</FrameRect>";
    let media = "<FrameRate>8467200000</FrameRate><FrameRect>0,0,1280,720</FrameRect>";
    let source = with_second_clip(SOURCE).replace(stream, media);
    let xml = with_chain(&source, DEFAULT_FLAGS, &[(20, masked_geometry(off_centre))]);
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    let kept: Vec<_> = project.sequences[0]
        .video_occurrences()
        .map(|clip| clip.id.as_deref())
        .collect();
    assert_eq!(kept, [Some("VideoClipTrackItem:9")]);
    assert!(
        omissions.iter().any(|omission| omission.scope == OmissionScope::Occurrence
            && omission.record == "3"
            && omission.reason.contains("an off-centre Geometry2 zoom on media that is rotated or not sequence-sized is not converted")),
        "{omissions:?}"
    );

    // A centered affine scale remains readable with its mask at the reader.
    // Size-mismatched media still meets the converter's existing frame rule.
    let xml = with_effects(&[(20, masked_geometry(DEFAULT_TRANSFORM))]).replace(stream, media);
    let (occurrence, omissions) = read(&xml);
    assert!(
        matches!(occurrence.effects.as_slice(), [effect] if matches!(effect.params, PrEffectParams::CornerPin(_)) && effect.mask.is_some()),
        "{:?}",
        occurrence.effects
    );
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.scope == OmissionScope::Occurrence),
        "{omissions:?}"
    );

    // A clip mask would make `read_effects` discard effects on both sides of
    // the mask boundary. Omit the unsafe occurrence rather than expose the
    // picture after its masked Geometry2 reaches zero width; keep its sibling.
    let collapse_keys = format!("0,100.,0,0,0,0,0,0;{},0.,0,0,0,0,0,0;", TICKS / 25);
    let mut collapsing = DEFAULT_TRANSFORM;
    collapsing[4] = ("100.", &collapse_keys);
    let xml = with_chain(
        &with_second_clip(SOURCE),
        DEFAULT_FLAGS,
        &[
            (20, masked_geometry(collapsing)),
            (60, top_crop(60)),
            (80, tint(80)),
        ],
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    let kept: Vec<_> = project.sequences[0]
        .video_occurrences()
        .map(|clip| clip.id.as_deref())
        .collect();
    assert_eq!(kept, [Some("VideoClipTrackItem:9")]);
    assert!(
        omissions
            .iter()
            .any(|omission| omission.scope == OmissionScope::Occurrence
                && omission.record == "3"
                && omission
                    .reason
                    .contains("masked Geometry2 does not convert on this clip")
                && omission
                    .reason
                    .contains("Crop, Linear Wipe or Track Matte Key")),
        "{omissions:?}"
    );
}

#[test]
fn geometry2_zoom_about_an_off_centre_anchor_lands_each_corner_there() {
    // A uniform zoom from 100 to 132.7 about an Anchor Point a third of the way
    // down the frame, with its Position 4 px above that point.
    let (anchor_y, position_y) = ("0.33445379137992859", "0.33055555820465088");
    let (anchor, position) = (
        [0.5, anchor_y.parse::<f64>().unwrap()],
        [0.5, position_y.parse::<f64>().unwrap()],
    );
    let keys = "0,100.,0,0,0,0,0,0;254016000000,132.7,0,0,0,0,0,0;";
    let (anchor_point, position_point) = (format!("0.5:{anchor_y}"), format!("0.5:{position_y}"));
    let mut values = DEFAULT_TRANSFORM;
    values[0] = (&anchor_point, "");
    values[1] = (&position_point, "");
    values[2] = ("true", "");
    values[3] = ("100.", keys);
    let records = transform_26_5_xml(20, values).replace("AE.ADBE Geometry", "AE.ADBE Geometry2");
    // Each frame corner lands at Position + Scale / 100 x (corner - Anchor).
    let landed = |scale: f64| {
        [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]].map(|corner: [f64; 2]| {
            [0, 1].map(|axis| position[axis] + scale / 100.0 * (corner[axis] - anchor[axis]))
        })
    };
    let close = |actual: [f64; 2], expected: [f64; 2]| {
        (0..2).all(|axis| (actual[axis] - expected[axis]).abs() < 1e-12)
    };
    let (occurrence, omissions) = read(&with_effects(&[(20, records.clone())]));
    assert!(omissions.is_empty(), "{omissions:?}");
    let [effect] = occurrence.effects.as_slice() else {
        panic!("{:?}", occurrence.effects);
    };
    let PrEffectParams::CornerPin(pin) = &effect.params else {
        panic!("{:?}", effect.params);
    };
    for (corner, expected) in pin.corners.into_iter().zip(landed(100.0)) {
        assert!(close(corner, expected), "{corner:?} {expected:?}");
    }
    assert_eq!(effect.animations.len(), 4);
    for (index, animation) in effect.animations.iter().enumerate() {
        let keys = animation.keys.point().unwrap();
        assert_eq!(
            keys.iter().map(|key| key.source_ticks).collect::<Vec<_>>(),
            [0, 254_016_000_000]
        );
        assert!(keys
            .iter()
            .all(|key| key.easing == PrKeyframeEasing::Linear));
        assert!(close(keys[0].value, landed(100.0)[index]));
        assert!(close(keys[1].value, landed(132.7)[index]));
    }
    // The rule is the sequence's frame, not one size: on a portrait sequence
    // with media of its size the corners land as on the landscape one.
    let portrait = with_effects(&[(20, records.clone())]).replace("0,0,1920,1080", "0,0,1080,1920");
    let (occurrence, omissions) = read(&portrait);
    assert!(omissions.is_empty(), "{omissions:?}");
    let [PrEffect {
        params: PrEffectParams::CornerPin(pin),
        ..
    }] = occurrence.effects.as_slice()
    else {
        panic!("{:?}", occurrence.effects);
    };
    for (corner, expected) in pin.corners.into_iter().zip(landed(100.0)) {
        assert!(close(corner, expected), "portrait: {corner:?} {expected:?}");
    }
    // The same placement holds for an Anchor Point and Position outside the
    // frame: every corner lands off the frame, statically and at each key.
    let (outside_anchor, outside_position) = ([1.25, -0.2], [-0.1, 1.3]);
    let mut outside = values;
    outside[0] = ("1.25:-0.2", "");
    outside[1] = ("-0.1:1.3", "");
    let outside = transform_26_5_xml(20, outside).replace("AE.ADBE Geometry", "AE.ADBE Geometry2");
    let (occurrence, omissions) = read(&with_effects(&[(20, outside)]));
    assert!(omissions.is_empty(), "{omissions:?}");
    let [PrEffect {
        params: PrEffectParams::CornerPin(pin),
        animations,
        ..
    }] = occurrence.effects.as_slice()
    else {
        panic!("{:?}", occurrence.effects);
    };
    let landed_outside = |scale: f64| {
        [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]].map(|corner: [f64; 2]| {
            [0, 1].map(|axis| {
                outside_position[axis] + scale / 100.0 * (corner[axis] - outside_anchor[axis])
            })
        })
    };
    for (corner, expected) in pin.corners.into_iter().zip(landed_outside(100.0)) {
        assert!(close(corner, expected), "outside: {corner:?} {expected:?}");
    }
    assert_eq!(animations.len(), 4);
    for (index, animation) in animations.iter().enumerate() {
        let keys = animation.keys.point().unwrap();
        assert!(close(keys[0].value, landed_outside(100.0)[index]));
        assert!(close(keys[1].value, landed_outside(132.7)[index]));
    }
    // On media that is not sequence-sized, or that its native orientation
    // turns, the frame of Anchor Point and Position is unmeasured: only the
    // centered zoom converts there, as for any other media.
    let stream = "<FrameRate>8467200000</FrameRate><FrameRect>0,0,1920,1080</FrameRect>";
    let turned =
        |code: &str| format!("<OriginalImageOrientationType>{code}</OriginalImageOrientationType>");
    values[0] = ("0.5:0.5", "");
    values[1] = ("0.5:0.5", "");
    let centred = transform_26_5_xml(20, values).replace("AE.ADBE Geometry", "AE.ADBE Geometry2");
    for (case, frame, orientation) in [
        ("smaller media", "0,0,1280,720", String::new()),
        (
            "quarter-turned sequence-sized media",
            "0,0,1920,1080",
            turned("6"),
        ),
        (
            "half-turned sequence-sized media",
            "0,0,1920,1080",
            turned("3"),
        ),
        (
            "media that a quarter turn makes sequence-sized",
            "0,0,1080,1920",
            turned("8"),
        ),
    ] {
        let media =
            format!("<FrameRate>8467200000</FrameRate><FrameRect>{frame}</FrameRect>{orientation}");
        let edit =
            |records: &str| with_effects(&[(20, records.to_owned())]).replace(stream, &media);
        let (occurrence, omissions) = read(&edit(&records));
        assert!(
            occurrence.effects.is_empty(),
            "{case}: {:?}",
            occurrence.effects
        );
        let [omission] = omissions.as_slice() else {
            panic!("{case}: {omissions:?}");
        };
        assert!(
            omission.reason.contains(
                "an off-centre Geometry2 zoom on media that is rotated or not sequence-sized is not converted"
            ),
            "{case}: {omission:?}"
        );
        let (occurrence, omissions) = read(&edit(&centred));
        assert!(omissions.is_empty(), "{case}: {omissions:?}");
        assert!(
            matches!(
                occurrence.effects.as_slice(),
                [PrEffect {
                    params: PrEffectParams::CornerPin(_),
                    ..
                }]
            ),
            "{case}"
        );
    }
}

#[test]
fn a_motion_crop_stages_above_an_off_centre_geometry2_zoom() {
    // Premiere applies the standard Geometry2 zoom and then Motion with its
    // Motion Crop, so on one sequence-sized clip the off-centre zoom stays the
    // video's Corner Pin and the Crop stages above it, its guide in the
    // video's frame under the group.
    let (anchor, position) = ([0.4, 0.3], [0.6, 0.65]);
    let mut values = DEFAULT_TRANSFORM;
    values[0] = ("0.4:0.3", "");
    values[1] = ("0.6:0.65", "");
    values[2] = ("true", "");
    values[3] = ("100.", "0,100.,0,0,0,0,0,0;254016000000,125.,0,0,0,0,0,0;");
    let zoom = transform_26_5_xml(30, values).replace("AE.ADBE Geometry", "AE.ADBE Geometry2");
    let xml = with_chain(
        SOURCE,
        EXPLICIT_MOTION_FLAGS,
        &[(199, motion_26_5("20.")), (30, zoom)],
    );
    let (clip, omissions) = read(&xml);
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(clip.crop.left, 20.0);
    assert_eq!(clip.effects_above_mask, 1);
    let [PrEffect {
        params: PrEffectParams::CornerPin(pin),
        animations,
        ..
    }] = clip.effects.as_slice()
    else {
        panic!("{:?}", clip.effects);
    };
    // Each frame corner lands at Position + Scale / 100 x (corner - Anchor).
    let landed = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]
        .map(|corner: [f64; 2]| [0, 1].map(|axis| position[axis] + corner[axis] - anchor[axis]));
    for (corner, expected) in pin.corners.into_iter().zip(landed) {
        assert!(
            (0..2).all(|axis| (corner[axis] - expected[axis]).abs() < 1e-12),
            "{corner:?} {expected:?}"
        );
    }
    assert_eq!(animations.len(), 4);
    let document = import(&xml);
    let (masked, video) = masked_and_video(&document);
    assert_eq!(masked["type"], "Group");
    let guide = masked["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == masked["masks"][0]["layer"])
        .unwrap();
    assert_eq!(
        (
            &guide["type"],
            &guide["rect"]["position"],
            &guide["rect"]["size"]
        ),
        (
            &serde_json::json!("Rect"),
            &serde_json::json!([384.0, 0.0]),
            &serde_json::json!([1536.0, 1080.0])
        )
    );
    let [effect] = video["effects"].as_array().unwrap().as_slice() else {
        panic!("{video}");
    };
    assert_eq!(effect["effect"]["type"], "cornerPin");
    assert!((effect["effect"]["upperLeftX"].as_f64().unwrap() - landed[0][0]).abs() < 1e-12);
}

#[test]
fn native_geometry2_keeps_nonpositive_scale_keys_and_geometry_marker_policy() {
    let records = include_str!("../../../tests/fixtures/cap2-native-geometry2.xml")
        .replace("<PremiereData Version=\"3\">", "")
        .replace("</PremiereData>", "")
        .replace("ObjectID=\"407\"", "ObjectID=\"20\"");
    for scale in [0.0, -118.0] {
        let changed = records.replace(
            "914545680048000,118.,",
            &format!("914545680048000,{scale},"),
        );
        let (occurrence, omissions) = read(&with_effects(&[(20, changed)]));
        assert!(omissions.is_empty(), "{omissions:?}");
        let [PrEffect {
            params: PrEffectParams::CornerPin(_),
            animations,
            ..
        }] = occurrence.effects.as_slice()
        else {
            panic!("{:?}", occurrence.effects);
        };
        assert_eq!(animations.len(), 4);
        assert_eq!(
            animations[0].keys.point().unwrap().last().unwrap().value[1],
            0.5 - scale / 200.0
        );
    }
    let reason = omitted_reason(records.replace("AE.ADBE Geometry2", "AE.ADBE Geometry"));
    assert!(reason.contains("empty time-varying Rotation"), "{reason}");
}

#[test]
fn sharpen_native_source_reads_amounts_and_source_keys() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_sharpen_strict.prproj");
    let xml = crate::format::read_xml(&path).unwrap();
    let (project, omissions) =
        inspect_project_with_omissions(&xml, Some("72a26059-6f85-4033-827d-63692bb9859b")).unwrap();
    let track = &project.sequences[0].video_tracks[0];
    for (index, amount) in [0, 40, 100, 20, 4000, 100].into_iter().enumerate() {
        let clip = track.clip(index);
        assert_eq!(clip.effects.len(), 1, "clip {index}: {omissions:?}");
        assert_eq!(clip.effects[0].spec().match_name, "AE.ADBE Sharpen");
        assert_eq!(
            clip.effects[0].params,
            PrEffectParams::Sharpen(crate::schema::PrSharpen { amount })
        );
    }
    let clip = track.clip(3);
    assert_eq!(clip.start_ticks, 6 * TICKS);
    assert_eq!(clip.in_ticks, TICKS / 2);
    assert_eq!(
        clip.effects[0].animations[0].keys.scalar().unwrap(),
        &[
            key(TICKS, 20.0),
            key(3 * TICKS / 2, 80.0),
            PrScalarKeyframe {
                easing: PrKeyframeEasing::Hold,
                ..key(5 * TICKS / 2, 50.0)
            },
        ]
    );
}

#[test]
fn sharpen_invalid_native_amount_or_keys_omit_only_the_effect() {
    let native = fixture_records("feature_sharpen_strict.prproj", &["134", "167"]);
    for changed in [
        native.replace(",80,4,", ",80.5,4,"),
        native.replace(",80,4,", ",4001,4,"),
        native.replace(",20,0,0,", ",20,1,0,"),
    ] {
        assert_ne!(changed, native);
        let (clip, omissions) = read(&with_effects(&[(134, changed), (20, blur(20))]));
        assert_eq!(clip.effects, [gaussian_blur(true, 25.0, false)]);
        assert!(
            omissions.iter().any(|note| note.reason.contains("Sharpen")),
            "{omissions:?}"
        );
    }
}

#[test]
fn sharpen_writer_uses_integer_amount_control() {
    let effect = PrEffect {
        mask: None,
        enabled: false,
        params: PrEffectParams::Sharpen(crate::schema::PrSharpen { amount: 137 }),
        animations: vec![],
    };
    let xml = project_xml(&project(vec![effect.clone()])).unwrap();
    assert!(xml.contains("<Name>Sharpen Amount</Name>"));
    assert!(xml.contains("<ParameterControlType>1</ParameterControlType>"));
    assert!(xml.contains("-91445760000000000,137,0,0,0,0,0,0"));
    assert!(xml.contains("<UpperBound>4000</UpperBound>"));
    assert_eq!(reread(&xml).effects, [effect]);
}

#[test]
fn interpretation_raw_active_effects_require_proved_static_semantics() {
    let mut unsupported_levels = NEUTRAL_LEVELS;
    unsupported_levels[6] = 200;
    let interpreted = with_second_clip(SOURCE).replace("<VideoStream ObjectID=\"8\">",
        "<VideoStream ObjectID=\"8\"><IsFrameRateOverridden>true</IsFrameRateOverridden><OveriddenFrameRate>8467200000</OveriddenFrameRate>");
    for source_chain in [false, true] {
        for (effect, retained) in [
            (blur(20), true),
            (levels(20, NEUTRAL_LEVELS, None), true),
            // The ordinary Levels fallback must not turn an unproved native
            // channel correction into admission of an interpreted occurrence.
            (levels(20, unsupported_levels, None), false),
            (
                levels(20, NEUTRAL_LEVELS, Some((3, FIXTURE_WHITE_OUTPUT_KEYS))),
                false,
            ),
            (keyed_blur(20, CORPUS_KEYED_BLURRINESS), false),
            (
                blur(20).replace("AE.ADBE Gaussian Blur 2", "AE.ADBE Offset"),
                false,
            ),
            (
                blur(20).replace("AE.ADBE Gaussian Blur 2", "AE.ADBE Lumetri"),
                false,
            ),
            (
                blur(20).replace("AE.ADBE Gaussian Blur 2", "AE.UnverifiedEffect"),
                false,
            ),
            (
                blur(20)
                    .replace("AE.ADBE Gaussian Blur 2", "AE.UnverifiedEffect")
                    .replace(ACTIVE, BYPASSED),
                true,
            ),
            (
                blur(20).replace(STATIC_BLUR, "<Keyframes>invalid</Keyframes>"),
                false,
            ),
        ] {
            let mut xml = with_chain(&interpreted, DEFAULT_FLAGS, &[(20, effect)]);
            if source_chain {
                xml = xml.replace("<Components ObjectRef=\"4\"/>", "<Components ObjectRef=\"98\"/>")
                    .replace("<Clip ObjectRef=\"6\"/>", "<Clip ObjectRef=\"6\"/><MasterClip ObjectURef=\"interpretation-master\"/>")
                    .replace("</PremiereData>", concat!(
                        "<VideoComponentChain ObjectID=\"98\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>",
                        "<MasterClip ObjectUID=\"interpretation-master\"><Clips><Clip ObjectRef=\"96\"/></Clips><VideoComponentChain ObjectRef=\"4\"/></MasterClip>",
                        "<VideoClip ObjectID=\"96\"><Clip><Source ObjectRef=\"7\"/></Clip></VideoClip></PremiereData>"));
            }
            let (project, omissions) =
                inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
            let clips: Vec<_> = project
                .single_sequence()
                .unwrap()
                .video_occurrences()
                .collect();
            assert_eq!(clips.len(), 2, "source={source_chain}: {omissions:?}");
            assert!(clips
                .iter()
                .any(|clip| clip.id.as_deref() == Some("VideoClipTrackItem:9")));
            let picture = clips
                .iter()
                .find(|clip| clip.id.as_deref() == Some("VideoClipTrackItem:3"))
                .unwrap();
            assert!(project.media.contains_key(&picture.media));
            assert_eq!(picture.timeline_ticks(), 0..5 * TICKS);
            if !retained {
                assert!(
                    !omissions
                        .iter()
                        .any(|o| o.scope == OmissionScope::Occurrence),
                    "{omissions:?}"
                );
            }
        }
    }
}

#[test]
fn levels_static_channel_rows_keep_master_and_neighboring_blur() {
    for (master, channel, keys) in [
        (0, [10, 255, 0, 255, 100], None),
        (1, [0, 255, 0, 0, 100], None),
        (0, [0, 255, 0, 255, 101], None),
        (
            0,
            [0, 255, 0, 0, 100],
            Some("0,0,0,0,0,0,0,0;254016000000,1,0,0,0,0,0,0;"),
        ),
    ] {
        let mut rows = NEUTRAL_LEVELS;
        rows[0] = master;
        rows[5..10].copy_from_slice(&channel);
        let (clip, omissions) = read(&with_effects(&[
            (20, levels(20, rows, keys.map(|keys| (0, keys)))),
            (50, blur(50)),
        ]));
        assert_eq!(clip.effects.len(), 3, "{omissions:?}");
        assert_eq!(clip.effects[0], gaussian_blur(true, 25.0, false));
        assert_eq!(
            clip.effects[1].params,
            PrEffectParams::Levels(PrLevels::Master {
                rgb: [f64::from(master), 255.0, 0.0, 255.0, 100.0]
            })
        );
        assert_eq!(
            clip.effects[1].animations.len(),
            usize::from(keys.is_some())
        );
        assert!(omissions.is_empty(), "{omissions:?}");
        assert!(matches!(
            clip.effects[2].params,
            PrEffectParams::Levels(PrLevels::Corrections(_))
        ));
    }
}

// Derived from the pinned Premiere RGB record shape, not independently authored Alpha proof.
#[test]
fn premiere_alpha_channel_is_retained_for_occurrence_lowering() {
    let (occurrence, omissions) = read(&with_effects(&[(
        20,
        invert_26_5_xml(20, "15", ("0.", "")),
    )]));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(occurrence.effects.len(), 1);
    assert!(matches!(
        occurrence.effects[0].params,
        PrEffectParams::Invert(PrInvert {
            channel: 15,
            blend: 0.0
        })
    ));
}
