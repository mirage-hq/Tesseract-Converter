//! Type-tool graphic reading from native XML. Records follow a Premiere 26.5
//! save; each case mutates one native field of that shape.

#[path = "graphic_ramp.rs"]
mod graphic_ramp;

use crate::format::{
    inspect_project, inspect_project_with_omissions,
    shape_payload::tests::{CENTRED, FILL, GRADIENT_B, RECTANGLE},
    text_payload::{
        decode, encode,
        tests::{BEFORE, CAPTION_STYLE_EFFECTS},
    },
};
use crate::schema::{
    text::{
        PrAppearance, PrFill, PrGraphicObject, PrJustification, PrRgb, PrTextFrame, PrVerticalAlign,
    },
    PrKeyframeEasing, PrPointKeyframe, PrPropertyAnimation, PrScalarKeyframe, PrVideoItem, TICKS,
};
use crate::{OmissionKind, OmissionScope};
use base64::{engine::general_purpose::STANDARD, Engine};

const SOURCE: &str = include_str!("../../../tests/fixtures/one-clip.xml");

const TWO_COMPONENTS: &str =
    r#"<Component Index="0" ObjectRef="30"/><Component Index="1" ObjectRef="40"/>"#;

const VECTOR_MOTION: &str = r#"
  <VideoFilterComponent ObjectID="30" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="9"><Component Version="7"><Params Version="1"><Param Index="0" ObjectRef="31"/><Param Index="1" ObjectRef="32"/><Param Index="2" ObjectRef="33"/><Param Index="3" ObjectRef="34"/><Param Index="4" ObjectRef="35"/><Param Index="5" ObjectRef="36"/></Params><ID>5</ID><Intrinsic>true</Intrinsic><DisplayName>Vector Motion</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Graphic Group</MatchName></VideoFilterComponent>
  <PointComponentParam ObjectID="31" ClassID="ca81d347-309b-44d2-acc7-1c572efb973c" Version="4"><Name>Position</Name><ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,0.25:0.25,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe></PointComponentParam>
  <VideoComponentParam ObjectID="32" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Scale</Name><ParameterID>2</ParameterID><UpperUIBound>200</UpperUIBound><StartKeyframe>-91445760000000000,50.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>10000</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="33" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Scale Width</Name><ParameterID>3</ParameterID><UpperUIBound>200</UpperUIBound><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>10000</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="34" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10"><Name> </Name><ParameterID>4</ParameterID><StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe></VideoComponentParam>
  <VideoComponentParam ObjectID="35" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Rotation</Name><ParameterControlType>3</ParameterControlType><ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,90.,0,0,0,0,0,0</StartKeyframe><LowerBound>-32768</LowerBound><UpperBound>32767</UpperBound></VideoComponentParam>
  <PointComponentParam ObjectID="36" ClassID="ca81d347-309b-44d2-acc7-1c572efb973c" Version="4"><Name>Anchor Point</Name><ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,0.5:0.5,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe></PointComponentParam>"#;

const TEXT_COMPONENT: &str = r#"
  <VideoFilterComponent ObjectID="40" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="9"><Component Version="7"><Node Version="1"><Properties Version="1"><ECP.Filter.Expanded>true</ECP.Filter.Expanded></Properties></Node><Params Version="1"><Param Index="0" ObjectRef="41"/><Param Index="1" ObjectRef="42"/><Param Index="2" ObjectRef="43"/><Param Index="3" ObjectRef="44"/><Param Index="4" ObjectRef="45"/><Param Index="5" ObjectRef="46"/><Param Index="6" ObjectRef="47"/><Param Index="7" ObjectRef="48"/><Param Index="8" ObjectRef="49"/><Param Index="9" ObjectRef="50"/><Param Index="10" ObjectRef="51"/><Param Index="11" ObjectRef="52"/><Param Index="12" ObjectRef="53"/><Param Index="13" ObjectRef="54"/><Param Index="14" ObjectRef="55"/><Param Index="15" ObjectRef="56"/><Param Index="16" ObjectRef="57"/><Param Index="17" ObjectRef="58"/><Param Index="18" ObjectRef="59"/><Param Index="19" ObjectRef="60"/><Param Index="20" ObjectRef="61"/><Param Index="21" ObjectRef="62"/></Params><ID>4</ID><DisplayName>Text</DisplayName><InstanceName>Before label</InstanceName></Component><PremiereFilterPrivateData Encoding="base64" BinaryHash="c40c6399-6b26-8c2c-feaf-d01b0000000d">AA==</PremiereFilterPrivateData><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Text</MatchName></VideoFilterComponent>
  <ArbVideoComponentParam ObjectID="41" ClassID="313e54d4-6903-49ad-b0bf-8262cdd10f4e" Version="3"><Node Version="1"><Properties Version="1"><ECP.Graphics.Expanded>true</ECP.Graphics.Expanded></Properties></Node><Name>Source Text</Name><ParameterControlType>9</ParameterControlType><ParameterID>1</ParameterID><StartKeyframePosition>-91445760000000000</StartKeyframePosition><StartKeyframeValue Encoding="base64" BinaryHash="5ed9ebfb-98e1-6486-992d-0b0600000160">PAYLOAD</StartKeyframeValue></ArbVideoComponentParam>
  <VideoComponentParam ObjectID="42" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10"><Name>Transform</Name><ParameterControlType>11</ParameterControlType><ParameterID>2</ParameterID><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><UpperBound>false</UpperBound></VideoComponentParam>
  <PointComponentParam ObjectID="43" ClassID="ca81d347-309b-44d2-acc7-1c572efb973c" Version="4"><Name>Position</Name><ParameterID>3</ParameterID><StartKeyframe>-91445760000000000,0.75:0.5,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe></PointComponentParam>
  <VideoComponentParam ObjectID="44" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Scale</Name><ParameterID>4</ParameterID><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>4000</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="45" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Horizontal Scale</Name><ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>4000</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="46" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10"><Name> </Name><ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe></VideoComponentParam>
  <VideoComponentParam ObjectID="47" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Rotation</Name><ParameterControlType>3</ParameterControlType><ParameterID>7</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>-32768</LowerBound><UpperBound>32767</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="48" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Opacity</Name><ParameterID>8</ParameterID><StartKeyframe>-91445760000000000,60.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound></VideoComponentParam>
  <PointComponentParam ObjectID="49" ClassID="ca81d347-309b-44d2-acc7-1c572efb973c" Version="4"><Name>Anchor Point</Name><ParameterID>9</ParameterID><StartKeyframe>-91445760000000000,0.01:0.02,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe></PointComponentParam>
  <VideoComponentParam ObjectID="50" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10"><ParameterControlType>12</ParameterControlType><ParameterID>10</ParameterID><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><UpperBound>false</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="51" ClassID="a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542" Version="10"><Name> </Name><ParameterID>11</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>32768</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="52" ClassID="a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542" Version="10"><Name> </Name><ParameterID>12</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>32768</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="53" ClassID="a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542" Version="10"><Name>start</Name><ParameterID>13</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>-100</LowerBound><UpperBound>1000000000</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="54" ClassID="a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542" Version="10"><Name>end</Name><ParameterID>14</ParameterID><StartKeyframe>-91445760000000000,6.,0,0,0,0,0,0</StartKeyframe><LowerBound>-100</LowerBound><UpperBound>1000000000</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="55" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10"><Name> </Name><ParameterID>15</ParameterID><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe></VideoComponentParam>
  <VideoComponentParam ObjectID="56" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10"><Name> </Name><ParameterID>16</ParameterID><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe></VideoComponentParam>
  <VideoComponentParam ObjectID="57" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10"><Name> </Name><ParameterID>17</ParameterID><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe></VideoComponentParam>
  <VideoComponentParam ObjectID="58" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10"><Name> </Name><ParameterID>18</ParameterID><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe></VideoComponentParam>
  <VideoComponentParam ObjectID="59" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Parent Width</Name><ParameterID>19</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>20000</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="60" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Parent Height</Name><ParameterID>20</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>20000</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="61" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Parent Rotation</Name><ParameterControlType>3</ParameterControlType><ParameterID>21</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>-32768</LowerBound><UpperBound>32767</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="62" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10"><Name> </Name><ParameterID>22</ParameterID><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe></VideoComponentParam>"#;

/// A Shape in the layout that Premiere 26.5.1 saves (calibration run 1):
/// run 1's rectangle (−300, −150)–(300, 150) with the Appearance
/// `APPEARANCE`, at Position 0.5:0.5 and Anchor Point 0:0.
const SHAPE_COMPONENT: &str = r#"
  <VideoFilterComponent ObjectID="80" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="9"><Component Version="7"><Params Version="1"><Param Index="0" ObjectRef="81"/><Param Index="1" ObjectRef="82"/><Param Index="2" ObjectRef="83"/><Param Index="3" ObjectRef="84"/><Param Index="4" ObjectRef="85"/><Param Index="5" ObjectRef="86"/><Param Index="6" ObjectRef="87"/><Param Index="7" ObjectRef="88"/><Param Index="8" ObjectRef="89"/><Param Index="9" ObjectRef="90"/><Param Index="10" ObjectRef="91"/><Param Index="11" ObjectRef="92"/><Param Index="12" ObjectRef="93"/><Param Index="13" ObjectRef="94"/><Param Index="14" ObjectRef="95"/><Param Index="15" ObjectRef="96"/><Param Index="16" ObjectRef="97"/><Param Index="17" ObjectRef="98"/></Params><ID>5</ID><DisplayName>Shape</DisplayName><InstanceName>Box</InstanceName></Component><PremiereFilterPrivateData Encoding="base64" BinaryHash="036b9652-ba57-8e23-9514-59270000001c">AAAAAAAAAAAAAAAAAAAAAA==</PremiereFilterPrivateData><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Shape</MatchName></VideoFilterComponent>
  <ArbVideoComponentParam ObjectID="81" ClassID="313e54d4-6903-49ad-b0bf-8262cdd10f4e" Version="3"><Name>Path</Name><ParameterControlType>22</ParameterControlType><ParameterID>1</ParameterID><StartKeyframePosition>-91445760000000000</StartKeyframePosition><StartKeyframeValue Encoding="base64" BinaryHash="59686de3-2d06-5d47-d06c-68a900000085">PATH</StartKeyframeValue></ArbVideoComponentParam>
  <ArbVideoComponentParam ObjectID="82" ClassID="313e54d4-6903-49ad-b0bf-8262cdd10f4e" Version="3"><Name>Appearance</Name><ParameterControlType>9</ParameterControlType><ParameterID>2</ParameterID><StartKeyframePosition>-91445760000000000</StartKeyframePosition><StartKeyframeValue Encoding="base64" BinaryHash="appearance-hash">APPEARANCE</StartKeyframeValue></ArbVideoComponentParam>
  <VideoComponentParam ObjectID="83" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10"><Name>Transform</Name><ParameterControlType>11</ParameterControlType><ParameterID>3</ParameterID><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><UpperBound>false</UpperBound></VideoComponentParam>
  <PointComponentParam ObjectID="84" ClassID="ca81d347-309b-44d2-acc7-1c572efb973c" Version="4"><Name>Position</Name><ParameterID>4</ParameterID><StartKeyframe>-91445760000000000,0.5:0.5,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe></PointComponentParam>
  <VideoComponentParam ObjectID="85" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Scale</Name><ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>4000</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="86" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Horizontal Scale</Name><ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>4000</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="87" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10"><Name> </Name><ParameterID>7</ParameterID><StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe></VideoComponentParam>
  <VideoComponentParam ObjectID="88" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Rotation</Name><ParameterControlType>3</ParameterControlType><ParameterID>8</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>-32768</LowerBound><UpperBound>32767</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="89" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Opacity</Name><ParameterID>9</ParameterID><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound></VideoComponentParam>
  <PointComponentParam ObjectID="90" ClassID="ca81d347-309b-44d2-acc7-1c572efb973c" Version="4"><Name>Anchor Point</Name><ParameterID>10</ParameterID><StartKeyframe>-91445760000000000,0:0,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe></PointComponentParam>
  <VideoComponentParam ObjectID="91" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10"><ParameterControlType>12</ParameterControlType><ParameterID>11</ParameterID><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><UpperBound>false</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="92" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10"><Name> </Name><ParameterID>12</ParameterID><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe></VideoComponentParam>
  <VideoComponentParam ObjectID="93" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10"><Name> </Name><ParameterID>13</ParameterID><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe></VideoComponentParam>
  <VideoComponentParam ObjectID="94" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10"><Name> </Name><ParameterID>14</ParameterID><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe></VideoComponentParam>
  <VideoComponentParam ObjectID="95" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10"><Name> </Name><ParameterID>15</ParameterID><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe></VideoComponentParam>
  <VideoComponentParam ObjectID="96" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Parent Width</Name><ParameterID>16</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>20000</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="97" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Parent Height</Name><ParameterID>17</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>20000</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="98" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Parent Rotation</Name><ParameterControlType>3</ParameterControlType><ParameterID>18</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>-32768</LowerBound><UpperBound>32767</UpperBound></VideoComponentParam>"#;

/// A closed Path of an equilateral triangle, in base64.
fn equilateral_triangle() -> String {
    let corner = |x: f32, y: f32| crate::schema::text::PrPathVertex {
        smooth: false,
        point: [x, y],
        in_tangent: [x, y],
        out_tangent: [x, y],
    };
    let path = crate::schema::text::PrShapePath {
        vertices: vec![
            corner(0.0, -200.0),
            corner(173.205_08, 100.0),
            corner(-173.205_08, 100.0),
        ],
        closed: true,
    };
    STANDARD.encode(crate::format::shape_payload::encode_path(&path).unwrap())
}

const TEXT_THEN_SHAPE: &str =
    r#"<Component Index="0" ObjectRef="40"/><Component Index="1" ObjectRef="80"/>"#;

/// `graphic_xml` whose chain is `components`, with the rectangle Shape of
/// `appearance` among its records.
fn shape_xml(components: &str, appearance: &str) -> String {
    let shape = SHAPE_COMPONENT
        .replace("PATH", RECTANGLE)
        .replace("APPEARANCE", appearance);
    graphic_xml(BEFORE)
        .replace(TWO_COMPONENTS, components)
        .replace("</PremiereData>", &format!("{shape}\n</PremiereData>"))
}

/// Every record of a graphic but its track item: chain 21 with the Text
/// component 40, whose Source Text is `payload`, and sub clip 22 of the
/// generator media. Track items that name both share one stored text.
pub(in crate::format) fn graphic_source_records(payload: &str) -> String {
    format!(
        "{}{VECTOR_MOTION}{}\n",
        GRAPHIC_SOURCE.replace("COMPONENTS", TWO_COMPONENTS),
        TEXT_COMPONENT.replace("PAYLOAD", payload)
    )
}

/// A 2 s graphic track item from `start` over [`graphic_source_records`].
pub(in crate::format) fn graphic_item_record(id: u32, start: i64) -> String {
    format!(
        r#"<VideoClipTrackItem ObjectID="{id}"><ClipTrackItem><ComponentOwner><Components ObjectRef="21"/></ComponentOwner><TrackItem><Start>{start}</Start><End>{}</End></TrackItem><SubClip ObjectRef="22"/></ClipTrackItem><ToneMapSettings>{{"peak":-1,"version":3}}</ToneMapSettings><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio></VideoClipTrackItem>
"#,
        start + 2 * TICKS
    )
}

const GRAPHIC_TRACK: &str = r#"
  <VideoClipTrack ObjectUID="track-2"><ClipTrack><Track><ID>2</ID><Index>1</Index></Track><ClipItems><TrackItems><TrackItem ObjectRef="20"/></TrackItems><Index>1</Index></ClipItems></ClipTrack></VideoClipTrack>
  <VideoClipTrackItem ObjectID="20"><ClipTrackItem><ComponentOwner><Components ObjectRef="21"/></ComponentOwner><TrackItem><Start>254016000000</Start><End>762048000000</End></TrackItem><SubClip ObjectRef="22"/></ClipTrackItem><ToneMapSettings>{"peak":-1,"version":3}</ToneMapSettings><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio></VideoClipTrackItem>"#;

const GRAPHIC_SOURCE: &str = r#"
  <VideoComponentChain ObjectID="21"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><DefaultMotionComponentID>1</DefaultMotionComponentID><DefaultOpacityComponentID>2</DefaultOpacityComponentID><ComponentChain Version="3"><Node Version="1"><Properties Version="1"><MZ.ComponentChain.ActiveComponentID>2</MZ.ComponentChain.ActiveComponentID></Properties></Node><Components Version="1">COMPONENTS</Components></ComponentChain></VideoComponentChain>
  <SubClip ObjectID="22"><Clip ObjectRef="23"/><MasterClip ObjectURef="graphic-master"/><Name>Graphic</Name><OrigChGrp>0</OrigChGrp></SubClip>
  <VideoClip ObjectID="23"><Clip><Node Version="1"><Properties Version="1"><BE.Prefs.SyntheticMedia.DefaultIsDropFrame>false</BE.Prefs.SyntheticMedia.DefaultIsDropFrame></Properties></Node><Source ObjectRef="24"/><ClipID>placed-graphic</ClipID><InPoint>914161248000000</InPoint><OutPoint>914669280000000</OutPoint></Clip></VideoClip>
  <VideoMediaSource ObjectID="24"><MediaSource Version="4"><Content Version="10"></Content><Media ObjectURef="graphic-media"/></MediaSource><OriginalDuration>10973491200000000</OriginalDuration></VideoMediaSource>
  <Media ObjectUID="graphic-media"><VideoStream ObjectRef="25"/><ImplementationID>42008e7a-de6f-4270-96de-7e287abb9b4b</ImplementationID><Infinite>true</Infinite><ActualMediaFilePath>1196574294</ActualMediaFilePath><FilePath>1196574294</FilePath><Title>Graphic</Title></Media>
  <VideoStream ObjectID="25"><Duration>10973491200000000</Duration><CodecType>1431194446</CodecType><IsStill>true</IsStill><IsContinuousTime>true</IsContinuousTime><FrameRate>8467200000</FrameRate><FrameRect>0,0,1920,1080</FrameRect><AlphaType>1</AlphaType></VideoStream>
  <MasterClip ObjectUID="graphic-master"><Clips Version="1"><Clip Index="0" ObjectRef="26"/></Clips><Name>Graphic</Name><MasterClipChangeVersion>5</MasterClipChangeVersion></MasterClip>
  <VideoClip ObjectID="26"><Clip><Source ObjectRef="24"/><ClipID>template-graphic</ClipID><InUse>false</InUse><InPoint>0</InPoint><OutPoint>1270080000000</OutPoint></Clip></VideoClip>"#;

/// The one-clip project plus a graphic from 1 s to 3 s on a second track.
fn graphic_xml(payload: &str) -> String {
    SOURCE
        .replace(
            r#"<Track ObjectURef="track-1"/>"#,
            r#"<Track ObjectURef="track-1"/><Track ObjectURef="track-2" Index="1"/>"#,
        )
        .replace(
            "</PremiereData>",
            &format!(
                "{GRAPHIC_TRACK}{}</PremiereData>",
                graphic_source_records(payload)
            ),
        )
}

fn graphic(xml: &str) -> crate::schema::PrGraphic {
    let sequence = inspect_project(xml, None).unwrap();
    let mut graphics = sequence.video_items().filter_map(PrVideoItem::graphic);
    graphics.next().expect("sequence has a graphic").clone()
}

// Unchanged native Text/Vector Motion records, with their BinaryHash definitions,
// in a media-free test placement. Provenance is beside the fragments.
fn point_title_xml(records: &str, components: &str) -> String {
    graphic_xml(BEFORE)
        .replace(TWO_COMPONENTS, components)
        .replace("0,0,1920,1080", "0,0,1080,1920")
        .replace("</PremiereData>", &format!("{records}</PremiereData>"))
}

fn point_title_document(xml: &str) -> serde_json::Value {
    let (project, mut omissions) = inspect_project_with_omissions(xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    crate::convert::premiere_to_tesseract(
        sequence,
        &project.media,
        &crate::tesseract_output::asset_ids_in_order(sequence, &project.media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap()
}

#[test]
fn native_point_titles_keep_the_centered_baselines_and_explicit_auto_leading() {
    for (records, components, first_baseline, spacing, lines) in [
        (
            include_str!("../../../tests/fixtures/point_text_lines/422.xml"),
            r#"<Component Index="0" ObjectRef="1619"/>"#,
            284.23648071,
            93.013519287,
            2,
        ),
        (
            include_str!("../../../tests/fixtures/point_text_lines/423.xml"),
            r#"<Component Index="0" ObjectRef="1621"/><Component Index="1" ObjectRef="1622"/>"#,
            271.252120972,
            80.491142273,
            3,
        ),
        (
            include_str!("../../../tests/fixtures/point_text_lines/424.xml"),
            r#"<Component Index="0" ObjectRef="1624"/><Component Index="1" ObjectRef="1625"/>"#,
            286.827625275,
            83.486091614,
            3,
        ),
    ] {
        let document = point_title_document(&point_title_xml(records, components));
        let layer = &document["composition"]["layers"][0];
        assert_eq!(layer["type"], "Text");
        assert_eq!(
            layer["sourceText"]["text"]
                .as_str()
                .unwrap()
                .split('\n')
                .count(),
            lines
        );
        assert!((layer["sourceText"]["leading"].as_f64().unwrap() - spacing).abs() < 1e-5);
        let transform = &layer["transform"];
        let baseline = transform["position"][1].as_f64().unwrap()
            - transform["anchorPoint"][1].as_f64().unwrap();
        assert!(
            (baseline - first_baseline).abs() < 0.001,
            "{baseline} != {first_baseline}"
        );
        assert_eq!(transform["scale"], serde_json::json!([100.0, 100.0]));
        assert_eq!(transform["rotation"], 0.0);
    }
}

#[test]
fn native_whole_line_styles_import_as_two_editable_text_children() {
    let mut document = point_title_document(&point_title_xml(
        include_str!("../../../tests/fixtures/point_text_lines/421.xml"),
        r#"<Component Index="0" ObjectRef="1617"/>"#,
    ));
    let group = &document["composition"]["layers"][0];
    assert_eq!(group["type"], "Group");
    let layers = group["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 2);
    assert!(layers.iter().all(|layer| layer["type"] == "Text"));
    let first = layers
        .iter()
        .find(|layer| layer["sourceText"]["fontFamily"] == "BowlbyOneSC-Regular")
        .unwrap();
    let second = layers
        .iter()
        .find(|layer| layer["sourceText"]["fontFamily"] == "MonaSans-Black")
        .unwrap();
    assert_eq!(first["sourceText"]["text"], "Bad at editing? ");
    assert_eq!(second["sourceText"]["text"], "Fix in one tap 🔥");
    assert_eq!(first["sourceText"]["allCaps"], true);
    assert_eq!(second["sourceText"]["allCaps"], false);
    assert!((first["sourceText"]["fontSize"].as_f64().unwrap() - 80.089752197).abs() < 1e-6);
    assert!((second["sourceText"]["fontSize"].as_f64().unwrap() - 59.953262329).abs() < 1e-6);
    assert_close(
        [
            group["transform"]["position"][0].as_f64().unwrap(),
            group["transform"]["position"][1].as_f64().unwrap(),
        ],
        [540.0, 330.74323654174805],
    );
    assert_eq!(
        group["playback"]["inputRange"],
        serde_json::json!({"start": 1000, "duration": 2000})
    );
    for (line, baseline, tracking, color) in [
        (first, -35.97195816040039, 6.0, [246.0, 205.0, 14.0]),
        (second, 35.97195816040039, -29.0, [246.0, 246.0, 246.0]),
    ] {
        assert_eq!(line["parent"], group["id"]);
        assert_eq!(
            line["activeRange"],
            serde_json::json!({"start": 0, "duration": 2000})
        );
        assert_close(
            [
                line["transform"]["position"][0].as_f64().unwrap(),
                line["transform"]["position"][1].as_f64().unwrap(),
            ],
            [0.0, baseline],
        );
        assert_eq!(line["sourceText"]["tracking"], tracking);
        assert_eq!(
            line["sourceText"]["fillColor"],
            serde_json::json!([color[0] / 255.0, color[1] / 255.0, color[2] / 255.0, 1.0])
        );
    }
    // The recovered title consists of native editable Text layers; changing
    // one child through the public document leaves the other child's style.
    let first_index = layers
        .iter()
        .position(|line| line["id"] == first["id"])
        .unwrap();
    let second_style = second["sourceText"].clone();
    document["composition"]["layers"][0]["layers"][first_index]["sourceText"]["text"] =
        serde_json::json!("Edited title");
    let editable = fx_schema::EditableFxCompositionDocument::from_json_value(document).unwrap();
    let mut edited = editable.to_json_value().unwrap();
    assert!(edited["composition"]["layers"][0]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|line| line["sourceText"] == second_style));
    edited["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .retain(|layer| layer["type"] == "Group");
    edited["duration"] = serde_json::json!(3.0);
    let mut canvas = crate::test_support::editable_document()["composition"]["layers"][1].clone();
    canvas["id"] = serde_json::json!(900);
    canvas["activeRange"]["duration"] = serde_json::json!(3000);
    canvas["rect"]["size"] = serde_json::json!([1080, 1920]);
    edited["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(canvas);
    let edited = fx_schema::EditableFxCompositionDocument::from_json_value(edited).unwrap();
    let mut omissions = Vec::new();
    let empty = std::collections::BTreeMap::new();
    let project = crate::convert::tesseract_to_premiere(
        &edited,
        &empty,
        &std::collections::BTreeMap::new(),
        &std::collections::BTreeMap::new(),
        crate::format::FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("edited-lines.prproj");
    crate::format::PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let (read, omissions) = crate::schema::PrProjectFile::load(&path).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let text: Vec<_> = read.sequences[0]
        .video_items()
        .filter_map(PrVideoItem::graphic)
        .flat_map(|graphic| graphic.texts())
        .collect();
    assert_eq!(text.len(), 2);
    assert!(text.iter().any(|text| text.document.text == "Edited title"));
    assert!(text
        .iter()
        .any(|text| text.document.text == "Fix in one tap 🔥"
            && text.document.font == "MonaSans-Black"
            && text.document.tracking == -29.0));
}

/// The opening title of `421.xml` again, with one static mask on its clip
/// Opacity (`mask::mask`: Feather 30, at ObjectIDs 300 on): the edited lines
/// and the edited mask export and read back as the unmasked title's lines do,
/// and the title imports as the unmasked one, its line group owning the mask
/// over a guide beside it at the identity.
#[test]
fn native_whole_line_styles_with_a_clip_opacity_mask_export_both_edited_lines_and_the_mask() {
    use crate::schema::{
        text::{PrPathVertex, PrShapePath},
        PrMask, MASK_FEATHER_APPROXIMATION,
    };
    use serde_json::{json, Value};

    /// Renames the line in BowlbyOneSC-Regular, at any depth of `layers`.
    fn rename_first_line(layers: &mut [Value]) -> bool {
        layers.iter_mut().any(|layer| {
            if layer.pointer("/sourceText/fontFamily") == Some(&json!("BowlbyOneSC-Regular")) {
                layer["sourceText"]["text"] = json!("Edited title");
                true
            } else {
                layer
                    .get_mut("layers")
                    .and_then(Value::as_array_mut)
                    .is_some_and(|layers| rename_first_line(layers))
            }
        })
    }

    let title = include_str!("../../../tests/fixtures/point_text_lines/421.xml");
    let control = point_title_xml(title, r#"<Component Index="0" ObjectRef="1617"/>"#);
    let masked = with_clip_opacity(
        r#"<Component Index="0" ObjectRef="70"/><Component Index="1" ObjectRef="1617"/>"#,
        "100.",
        "",
        (18, 0),
    )
    .replace("0,0,1920,1080", "0,0,1080,1920")
    .replace(
        "<MatchName>AE.ADBE Opacity</MatchName>",
        r#"<SubComponents Version="1"><SubComponent Index="0" ObjectRef="300"/></SubComponents><MatchName>AE.ADBE Opacity</MatchName>"#,
    )
    .replace(
        "</PremiereData>",
        &format!("{title}{}</PremiereData>", super::mask::mask(300, true)),
    );
    let import = |xml: &str| {
        let (project, mut omissions) = inspect_project_with_omissions(xml, None).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let sequence = project.single_sequence().unwrap();
        let document = crate::convert::premiere_to_tesseract(
            sequence,
            &project.media,
            &crate::tesseract_output::asset_ids_in_order(sequence, &project.media),
            &mut omissions,
        )
        .unwrap()
        .to_json_value()
        .unwrap();
        (document, omissions)
    };
    let (control, control_omissions) = import(&control);
    assert!(
        control_omissions
            .iter()
            .all(|omission| omission.scope != OmissionScope::Occurrence),
        "{control_omissions:?}"
    );
    let (mut masked, omissions) = import(&masked);
    // Both titles report their unpackaged fonts; the mask adds only its
    // Feather approximation.
    let (feather, omissions): (Vec<_>, Vec<_>) = omissions
        .into_iter()
        .partition(|omission| omission.reason == MASK_FEATHER_APPROXIMATION);
    assert_eq!(feather.len(), 1, "{feather:?}");
    assert_eq!(omissions, control_omissions);
    let layers = masked["composition"]["layers"].as_array().unwrap();
    let owner = layers
        .iter()
        .find(|layer| {
            layer
                .get("masks")
                .and_then(Value::as_array)
                .is_some_and(|masks| !masks.is_empty())
        })
        .unwrap_or_else(|| panic!("the masked title: {layers:?}"))
        .clone();
    let guide = layers
        .iter()
        .find(|layer| layer["id"] == owner["masks"][0]["layer"])
        .unwrap_or_else(|| panic!("the guide beside the title: {layers:?}"))
        .clone();

    // Edit the mask: a new outline in frame pixels, and Inverted.
    for layer in masked["composition"]["layers"].as_array_mut().unwrap() {
        if layer["id"] == guide["id"] {
            layer["shape"]["path"] = json!({"commands": [
                {"type": "moveTo", "x": 270.0, "y": 480.0},
                {"type": "lineTo", "x": 810.0, "y": 480.0},
                {"type": "lineTo", "x": 810.0, "y": 1440.0},
                {"type": "lineTo", "x": 270.0, "y": 1440.0},
                {"type": "close"}
            ]});
        } else if layer["id"] == owner["id"] {
            layer["masks"][0]["inverted"] = json!(true);
        }
    }
    // Edit the first line, then export the title, any guide and a canvas, and
    // read the written project back.
    let export = |mut document: Value, guide_id: Option<&Value>| {
        let layers = document["composition"]["layers"].as_array_mut().unwrap();
        assert!(rename_first_line(layers));
        layers.retain(|layer| layer["type"] == "Group" || Some(&layer["id"]) == guide_id);
        let mut canvas =
            crate::test_support::editable_document()["composition"]["layers"][1].clone();
        canvas["id"] = json!(900);
        canvas["activeRange"]["duration"] = json!(3000);
        canvas["rect"]["size"] = json!([1080, 1920]);
        layers.push(canvas);
        document["duration"] = json!(3.0);
        let document = fx_schema::EditableFxCompositionDocument::from_json_value(document).unwrap();
        let mut omissions = Vec::new();
        let empty = std::collections::BTreeMap::new();
        let project = crate::convert::tesseract_to_premiere(
            &document,
            &empty,
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
            crate::format::FrameRate::Fps30,
            &mut omissions,
        )
        .unwrap_or_else(|error| panic!("{error}: {omissions:?}"));
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("edited-lines.prproj");
        crate::format::PremiereProjectXml::new(&project)
            .unwrap()
            .write_new(&path)
            .unwrap();
        let (read, read_omissions) = crate::schema::PrProjectFile::load(&path).unwrap();
        assert!(read_omissions.is_empty(), "{read_omissions:?}");
        (project, read, omissions)
    };
    let corner = |x: f32, y: f32| PrPathVertex {
        smooth: false,
        point: [x, y],
        in_tangent: [x, y],
        out_tangent: [x, y],
    };
    let edited = PrMask {
        raster: None,
        feather_keys: Vec::new(),
        expansion: 0.0,
        expansion_keys: Vec::new(),
        opacity_keys: Vec::new(),
        path: PrShapePath {
            vertices: vec![
                corner(0.25, 0.25),
                corner(0.75, 0.25),
                corner(0.75, 0.75),
                corner(0.25, 0.75),
            ],
            closed: true,
        },
        path_keys: Vec::new(),
        feather: 30.0,
        opacity: 100.0,
        inverted: true,
    };
    for (document, guide_id, mask) in [
        (control.clone(), None, None),
        (masked, Some(&guide["id"]), Some(&edited)),
    ] {
        let (exported, read, omissions) = export(document, guide_id);
        // No occurrence is omitted; only the mask's Feather is approximated.
        assert!(
            omissions
                .iter()
                .all(|omission| omission.scope != OmissionScope::Occurrence
                    && omission.reason == MASK_FEATHER_APPROXIMATION),
            "{omissions:?}"
        );
        assert_eq!(omissions.is_empty(), mask.is_none(), "{omissions:?}");
        for project in [&exported, &read] {
            let graphics: Vec<_> = project.sequences[0]
                .video_items()
                .filter_map(PrVideoItem::graphic)
                .collect();
            let [graphic] = graphics.as_slice() else {
                panic!("one title, its guide consumed: {graphics:?}");
            };
            let text: Vec<_> = graphic.texts().collect();
            assert_eq!(text.len(), 2, "{graphic:?}");
            assert!(text.iter().any(|text| text.document.text == "Edited title"));
            assert!(text
                .iter()
                .any(|text| text.document.text == "Fix in one tap 🔥"
                    && text.document.font == "MonaSans-Black"
                    && text.document.tracking == -29.0));
            assert_eq!(graphic.opacity_mask.as_ref(), mask);
        }
    }

    // The masked title imports as the unmasked one: the same line group, ids,
    // transforms, line positions and order, visibility, range and clock. The
    // group owns the mask, whose guide is its sibling at the identity over
    // its range, so the mask stays in the sequence frame after the title's
    // transform, where Premiere applies a clip Opacity mask.
    let without_masks = |layer: &Value| {
        let mut layer = layer.clone();
        layer.as_object_mut().unwrap().remove("masks");
        layer
    };
    assert_eq!(
        without_masks(&owner),
        without_masks(&control["composition"]["layers"][0])
    );
    assert_eq!(
        owner["masks"],
        json!([{"id": owner["masks"][0]["id"], "mode": "add", "inverted": false, "layer": guide["id"], "feather": [30.0, 30.0], "expansion": 0.0, "opacity": 1.0}])
    );
    assert_eq!(guide["type"], "Shape");
    assert_eq!(guide.get("parent"), owner.get("parent"));
    assert_eq!(
        crate::test_support::layer_range(&guide),
        crate::test_support::layer_range(&owner)
    );
    assert_eq!(
        serde_json::from_value::<fx_schema::Transform>(guide["transform"].clone()).unwrap(),
        crate::convert::identity_transform()
    );
}

#[test]
fn native_mixed_line_source_text_keys_are_explicitly_rejected() {
    let xml = point_title_xml(
        include_str!("../../../tests/fixtures/point_text_lines/421.xml"),
        r#"<Component Index="0" ObjectRef="1617"/>"#,
    );
    let (prefix, text_parameter) = xml.split_at(
        xml.find("<ArbVideoComponentParam ObjectID=\"4009\"")
            .unwrap(),
    );
    let xml = format!("{prefix}{}", text_parameter.replacen(
        "<Name>Source Text</Name>",
        "<Name>Source Text</Name><IsTimeVarying>true</IsTimeVarying><Keyframes>0,AA==;</Keyframes>",
        1,
    ));
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(project
        .single_sequence()
        .unwrap()
        .video_items()
        .all(|item| item.graphic().is_none()));
    assert!(
        omissions.iter().any(|omission| omission
            .reason
            .contains("keyed mixed text styles are unsupported")),
        "{omissions:?}"
    );
}

fn assert_close(actual: [f64; 2], expected: [f64; 2]) {
    assert!(
        (actual[0] - expected[0]).abs() < 1e-9 && (actual[1] - expected[1]).abs() < 1e-9,
        "{actual:?} != {expected:?}"
    );
}

/// The generator time at the start of `graphic_xml`'s placement (`InPoint`).
const IN: i64 = 914_161_248_000_000;

/// Two point keys in the form that Premiere 26.5.1 keeps from an XML edit.
const TWO_POINT_KEYS: &str = "0,0.01:0.02,0,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;254016000000,0.02:0.02,0,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;";

/// Two Linear scalar keys in the form that Premiere 26.5.1 saves.
const TWO_SCALAR_KEYS: &str = "0,100.,0,0,0,0.16666666666666666,20,0.16666666666666666;254016000000,120.,0,0,20,0.16666666666666666,0,0.16666666666666666;";

/// Adds `keys` to the parameter record `object`, with `IsTimeVarying` after
/// its name as Premiere 26.5.1 saves it (`None` leaves the flag out, as
/// Premiere 14 did).
fn with_keys(xml: &str, object: u32, flag: Option<&str>, keys: &str) -> String {
    let start = xml
        .find(&format!(" ObjectID=\"{object}\""))
        .expect("parameter exists");
    let name_end = start + xml[start..].find("</Name>").unwrap() + "</Name>".len();
    let value_end =
        start + xml[start..].find("</StartKeyframe>").unwrap() + "</StartKeyframe>".len();
    let flag = flag.map_or_else(String::new, |flag| {
        format!("<IsTimeVarying>{flag}</IsTimeVarying>")
    });
    format!(
        "{}{flag}{}<Keyframes>{keys}</Keyframes>{}",
        &xml[..name_end],
        &xml[name_end..value_end],
        &xml[value_end..]
    )
}

fn keyed(xml: &str, object: u32, keys: &str) -> String {
    with_keys(xml, object, Some("true"), keys)
}

/// `graphic_xml` with only its Text component, so nothing composes.
fn text_only_xml() -> String {
    graphic_xml(BEFORE).replace(TWO_COMPONENTS, r#"<Component Index="0" ObjectRef="40"/>"#)
}

#[test]
fn a_graphic_on_a_custom_canvas_is_placed_in_that_frame_and_a_stale_one_is_omitted() {
    let landscape = text_only_xml();
    assert_eq!(
        landscape
            .matches("<FrameRect>0,0,1920,1080</FrameRect>")
            .count(),
        5
    );
    // Every frame is portrait: the sequence's, both placements' and both
    // streams'. Object positions are fractions of the frame; sizes are pixels.
    let portrait = landscape.replace(
        "<FrameRect>0,0,1920,1080</FrameRect>",
        "<FrameRect>0,0,1080,1920</FrameRect>",
    );
    let (_, omissions) = inspect_project_with_omissions(&portrait, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let (wide, tall) = (graphic(&landscape), graphic(&portrait));
    assert_eq!(wide.text().transform.position, [1440.0, 540.0]);
    assert_eq!(tall.text().transform.position, [810.0, 960.0]);
    assert_close(tall.text().transform.anchor, [0.01 * 1080.0, 0.02 * 1920.0]);
    assert_eq!(tall.text().document, wide.text().document);

    // The graphic placement and its generator left at 1920x1080 inside the
    // portrait sequence are omitted; the video clip stays.
    let stale = portrait
        .replacen(
            r#"</ToneMapSettings><FrameRect>0,0,1080,1920</FrameRect>"#,
            r#"</ToneMapSettings><FrameRect>0,0,1920,1080</FrameRect>"#,
            1,
        )
        .replacen(
            "<FrameRate>8467200000</FrameRate><FrameRect>0,0,1080,1920</FrameRect><AlphaType>",
            "<FrameRate>8467200000</FrameRate><FrameRect>0,0,1920,1080</FrameRect><AlphaType>",
            1,
        );
    assert_eq!(
        stale
            .matches("<FrameRect>0,0,1080,1920</FrameRect>")
            .count(),
        3
    );
    let (project, omissions) = inspect_project_with_omissions(&stale, None).unwrap();
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.video_occurrences().count(), 1);
    assert_eq!(
        sequence
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .count(),
        0
    );
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert_eq!(
        (omissions[0].scope, omissions[0].record.as_str()),
        (OmissionScope::Occurrence, "20")
    );
    assert!(
        omissions[0].reason.contains("unsupported graphic geometry"),
        "{omissions:?}"
    );
}

#[test]
fn a_graphic_stream_of_non_square_pixels_omits_only_its_graphic() {
    // Only the generator stream declares its pixel shape: the placement keeps
    // square pixels and the sequence frame, on 1920x1080 and on a portrait
    // canvas, and the video clip beside the graphic stays.
    let landscape = text_only_xml();
    let portrait = landscape.replace(
        "<FrameRect>0,0,1920,1080</FrameRect>",
        "<FrameRect>0,0,1080,1920</FrameRect>",
    );
    for (xml, frame) in [(landscape, "0,0,1920,1080"), (portrait, "0,0,1080,1920")] {
        let stream = format!("<FrameRect>{frame}</FrameRect><AlphaType>");
        assert_eq!(xml.matches(&stream).count(), 1, "{frame}");
        let declared = |ratio: &str| {
            xml.replace(
                &stream,
                &format!(
                    "<FrameRect>{frame}</FrameRect><PixelAspectRatio>{ratio}</PixelAspectRatio><AlphaType>"
                ),
            )
        };
        let (project, omissions) = inspect_project_with_omissions(&declared("1,1"), None).unwrap();
        assert!(omissions.is_empty(), "{frame}: {omissions:?}");
        assert_eq!(
            project.single_sequence().unwrap().video_items().count(),
            2,
            "{frame}"
        );

        let (project, omissions) = inspect_project_with_omissions(&declared("2,1"), None).unwrap();
        let sequence = project.single_sequence().unwrap();
        assert_eq!(sequence.video_items().count(), 1, "{frame}");
        assert_eq!(sequence.video_occurrences().count(), 1, "{frame}");
        assert_eq!(omissions.len(), 1, "{frame}: {omissions:?}");
        assert_eq!(
            (omissions[0].scope, omissions[0].record.as_str()),
            (OmissionScope::Occurrence, "20"),
            "{frame}"
        );
        assert!(
            omissions[0]
                .reason
                .contains("VideoStream:25: non-square source pixels"),
            "{frame}: {omissions:?}"
        );
    }
}

#[test]
fn a_graphic_placement_reads_an_equal_pixel_pair_as_square() {
    // Premiere saves a placement's square pixels as any equal pair. An
    // anamorphic pair omits only the graphic and names its ratio.
    let xml = text_only_xml();
    let placement = r#"<ToneMapSettings>{"peak":-1,"version":3}</ToneMapSettings><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio>"#;
    assert_eq!(xml.matches(placement).count(), 1);
    let declared = |ratio: &str| {
        xml.replace(
            placement,
            &placement.replace(">1,1<", &format!(">{ratio}<")),
        )
    };
    let (project, omissions) =
        inspect_project_with_omissions(&declared("1920,1920"), None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(project.single_sequence().unwrap().video_items().count(), 2);

    let (project, omissions) =
        inspect_project_with_omissions(&declared("1920,1440"), None).unwrap();
    assert_eq!(project.single_sequence().unwrap().video_items().count(), 1);
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert!(
        omissions[0]
            .reason
            .contains("unsupported graphic geometry: non-square pixels (1920:1440)"),
        "{omissions:?}"
    );
}

fn scalar(source_ticks: i64, value: f64, easing: PrKeyframeEasing) -> PrScalarKeyframe {
    PrScalarKeyframe {
        source_ticks,
        value,
        easing,
    }
}

fn point(source_ticks: i64, value: [f64; 2], easing: PrKeyframeEasing) -> PrPointKeyframe {
    PrPointKeyframe {
        source_ticks,
        value,
        easing,
        spatial_in_tangent: None,
        spatial_out_tangent: None,
    }
}

#[test]
fn text_object_keys_read_on_the_generator_clock_including_keys_outside_the_trim() {
    use PrKeyframeEasing::{Hold, Linear};
    // The placement shows generator time IN to IN + 2 s. Position keys fall
    // before, inside and after it; a Hold key's out influence is 1/3.
    let position = format!(
        "{},0.25:0.5,0,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;\
         {},0.3:0.55,4,0,0,0.16666666666666666,0,0.33333333333333331,0,0,0,0,0,0;\
         {},0.35:0.6,0,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;",
        IN - TICKS / 2,
        IN + TICKS,
        IN + 5 * TICKS / 2
    );
    let scale = format!(
        "{IN},100.,0,0,0,0.16666666666666666,20,0.16666666666666666;{},120.,0,0,20,0.16666666666666666,0,0.16666666666666666;",
        IN + TICKS
    );
    let rotation = format!(
        "{},0.,4,0,0,0.16666666666666666,0,0.33333333333333331;{},20.,0,0,20,0.16666666666666666,0,0.16666666666666666;",
        IN + TICKS / 2,
        IN + 3 * TICKS / 2
    );
    let opacity = format!(
        "{},100.,0,0,0,0.16666666666666666,-6,0.16666666666666666;{},40.,0,0,-6,0.16666666666666666,0,0.16666666666666666;",
        IN + TICKS / 2,
        IN + 3 * TICKS / 2
    );
    let mut xml = text_only_xml();
    for (object, keys) in [
        (43, &position),
        (44, &scale),
        (47, &rotation),
        (48, &opacity),
    ] {
        xml = keyed(&xml, object, keys);
    }
    let (_, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let graphic = graphic(&xml);
    assert_eq!(graphic.in_ticks, IN);
    // Static values stay the parameters' own; keys keep generator times.
    let transform = graphic.text().transform;
    assert_eq!(transform.position, [1440.0, 540.0]);
    assert_eq!((transform.scale, transform.opacity), (100.0, 60.0));
    assert_eq!(
        graphic.text().animations,
        [
            PrPropertyAnimation::Position(vec![
                point(IN - TICKS / 2, [0.25, 0.5], Linear),
                point(IN + TICKS, [0.3, 0.55], Linear),
                point(IN + 5 * TICKS / 2, [0.35, 0.6], Hold),
            ]),
            PrPropertyAnimation::UniformScale(vec![
                scalar(IN, 100.0, Linear),
                scalar(IN + TICKS, 120.0, Linear),
            ]),
            PrPropertyAnimation::Rotation(vec![
                scalar(IN + TICKS / 2, 0.0, Linear),
                scalar(IN + 3 * TICKS / 2, 20.0, Hold),
            ]),
            PrPropertyAnimation::Opacity(vec![
                scalar(IN + TICKS / 2, 100.0, Linear),
                scalar(IN + 3 * TICKS / 2, 40.0, Linear),
            ]),
        ]
    );
}

#[test]
fn a_static_vector_motion_composes_into_text_keys_and_their_spatial_tangents() {
    // Vector Motion at (480, 270), 50%, 90° clockwise, with an off-centre
    // anchor (768, 486): p -> (480, 270) + 0.5 * R(90°) * (p - (768, 486)).
    let xml = graphic_xml(BEFORE).replace(
        "<Name>Anchor Point</Name><ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,0.5:0.5,",
        "<Name>Anchor Point</Name><ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,0.4:0.45,",
    );
    // A curved path in Premiere's UI form (spatial mode 5, automatic flag 4).
    let position = format!(
        "{IN},0.25:0.5,0,0,0,0.16666666666666666,0,0.16666666666666666,5,4,0,0,0.05,0;{},0.3:0.55,0,0,0,0.16666666666666666,0,0.16666666666666666,5,4,-0.05,0,0,0;",
        IN + TICKS
    );
    let xml = keyed(
        &keyed(&keyed(&xml, 43, &position), 44, TWO_SCALAR_KEYS),
        47,
        TWO_SCALAR_KEYS,
    );
    let graphic = graphic(&xml);
    let transform = graphic.text().transform;
    assert_close(transform.position, [453.0, 606.0]);
    assert_eq!((transform.scale, transform.rotation), (50.0, 90.0));
    let [PrPropertyAnimation::Position(position), PrPropertyAnimation::UniformScale(scale), PrPropertyAnimation::Rotation(rotation)] =
        graphic.text().animations.as_slice()
    else {
        panic!("{:?}", graphic.text().animations);
    };
    // (480, 540) -> (453, 126) and (576, 594) -> (426, 174); the 96 px
    // horizontal tangents turn into 48 px vertical ones.
    let normalized = |x: f64, y: f64| [x / 1920.0, y / 1080.0];
    assert_close(position[0].value, normalized(453.0, 126.0));
    assert_close(position[1].value, normalized(426.0, 174.0));
    assert_close(
        position[0].spatial_out_tangent.unwrap(),
        normalized(0.0, 48.0),
    );
    assert_close(
        position[1].spatial_in_tangent.unwrap(),
        normalized(0.0, -48.0),
    );
    assert_eq!(
        (position[0].source_ticks, position[1].easing),
        (IN, PrKeyframeEasing::Linear)
    );
    let values = |keys: &[PrScalarKeyframe]| keys.iter().map(|key| key.value).collect::<Vec<_>>();
    assert_eq!(values(scale), [50.0, 60.0]);
    assert_eq!(values(rotation), [190.0, 210.0]);
}

#[test]
fn keyed_vector_motion_stays_separate_from_the_text_it_moves() {
    use PrKeyframeEasing::{Hold, Linear};
    // Vector Motion keys like case A's: Position with a Hold, Scale 100 -> 70
    // Linear and Rotation 0 -> 20 Hold; the text has its own Opacity keys.
    let position = format!(
        "{IN},0.25:0.25,0,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;\
         {},0.3:0.2,4,0,0,0.16666666666666666,0,0.33333333333333331,0,0,0,0,0,0;\
         {},0.35:0.2,0,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;",
        IN + TICKS / 2,
        IN + TICKS
    );
    let scale = format!(
        "{IN},100.,0,0,0,0.16666666666666666,-3,0.16666666666666666;{},70.,0,0,-3,0.16666666666666666,0,0.16666666666666666;",
        IN + TICKS
    );
    let rotation = format!(
        "{IN},0.,4,0,0,0.16666666666666666,0,0.33333333333333331;{},20.,0,0,20,0.16666666666666666,0,0.16666666666666666;",
        IN + TICKS
    );
    let opacity = format!(
        "{IN},60.,0,0,0,0.16666666666666666,-6,0.16666666666666666;{},40.,0,0,-6,0.16666666666666666,0,0.16666666666666666;",
        IN + TICKS
    );
    let mut xml = graphic_xml(BEFORE);
    for (object, keys) in [
        (31, &position),
        (32, &scale),
        (35, &rotation),
        (48, &opacity),
    ] {
        xml = keyed(&xml, object, keys);
    }
    let (_, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let graphic = graphic(&xml);
    let motion = graphic.vector_motion.clone().expect("keyed Vector Motion");
    assert_eq!(
        (motion.position, motion.anchor),
        ([480.0, 270.0], [960.0, 540.0])
    );
    assert_eq!((motion.scale, motion.rotation), (50.0, 90.0));
    assert_eq!(
        motion.animations,
        [
            PrPropertyAnimation::Position(vec![
                point(IN, [0.25, 0.25], Linear),
                point(IN + TICKS / 2, [0.3, 0.2], Linear),
                point(IN + TICKS, [0.35, 0.2], Hold),
            ]),
            PrPropertyAnimation::UniformScale(vec![
                scalar(IN, 100.0, Linear),
                scalar(IN + TICKS, 70.0, Linear),
            ]),
            PrPropertyAnimation::Rotation(vec![
                scalar(IN, 0.0, Linear),
                scalar(IN + TICKS, 20.0, Hold),
            ]),
        ]
    );
    // The text keeps its own transform and keys, in the group's frame.
    let transform = graphic.text().transform;
    assert_eq!(transform.position, [1440.0, 540.0]);
    assert_eq!((transform.scale, transform.rotation), (100.0, 0.0));
    assert_eq!(
        graphic.text().animations,
        [PrPropertyAnimation::Opacity(vec![
            scalar(IN, 60.0, Linear),
            scalar(IN + TICKS, 40.0, Linear),
        ])]
    );
}

#[test]
fn linear_and_hold_vector_motion_keys_import_with_straight_spatial_paths() {
    // A slide in, a hold at one value and a slide out, with Premiere's
    // automatic spatial tangents (spatial mode 5, flag 4). Linear and Hold
    // timing ignores the stored speeds: Scale 70 -> 0 stores a tenth of its
    // value per second, as Premiere 26.5.1 saved Vector Motion Scale.
    let four = format!(
        "{IN},0.5:0.7,0,0,0,0.16666666666666666,0.3,0.16666666666666666,5,4,0,0,0,-0.03;\
         {},0.5:0.5,4,0,0.3,0.16666666666666666,0,0.33333333333333331,5,4,0,0.03,0,0;\
         {},0.5:0.5,0,0,0,0.16666666666666666,0.3,0.16666666666666666,5,4,0,0,0,0.03;\
         {},0.5:0.7,0,0,0.3,0.16666666666666666,0,0.16666666666666666,5,4,0,-0.03,0,0;",
        IN + TICKS / 2,
        IN + TICKS,
        IN + 3 * TICKS / 2
    );
    let two_linear = format!(
        "{IN},0.5:0.7,0,0,0,0.16666666666666666,0.3,0.16666666666666666,5,4,0,0,0,-0.03;{},0.5:0.5,0,0,0.3,0.16666666666666666,0,0.16666666666666666,5,4,0,0.03,0,0;",
        IN + TICKS / 2
    );
    let two_hold = format!(
        "{IN},0.5:0.7,4,0,0,0.16666666666666666,0,0.33333333333333331,5,4,0,0,0,-0.03;{},0.5:0.5,0,0,0,0.16666666666666666,0,0.16666666666666666,5,4,0,0.03,0,0;",
        IN + TICKS / 2
    );
    let scale = format!(
        "{IN},70.,0,0,0,0.16666666666666666,-7,0.16666666666666666;{},0.,0,0,-7,0.16666666666666666,0,0.16666666666666666;",
        IN + TICKS
    );
    for (position, easings) in [
        (&four, &["linear", "linear", "hold", "linear"][..]),
        (&two_linear, &["linear", "linear"][..]),
        (&two_hold, &["linear", "hold"][..]),
    ] {
        let xml = keyed(&keyed(&graphic_xml(BEFORE), 31, position), 32, &scale);
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let graphic = graphic(&xml);
        let motion = graphic.vector_motion.as_ref().expect("keyed Vector Motion");
        let PrPropertyAnimation::Position(keys) = &motion.animations[0] else {
            panic!("{:?}", motion.animations);
        };
        assert_eq!(keys.len(), easings.len());
        assert_eq!(keys[0].spatial_out_tangent, Some([0.0, -0.03]));
        assert_eq!(keys[1].spatial_in_tangent, Some([0.0, 0.03]));
        // The native model keeps resolved handles; the group tracks normalize
        // these straight segments so only their temporal easing drives motion.
        let sequence = project.single_sequence().unwrap();
        let document = crate::tests::support::project_document_with_media(sequence, &project.media);
        let entries = document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap();
        let track = |property: &str| {
            entries
                .iter()
                .find(|entry| entry["target"]["propertyType"] == property)
                .unwrap()["animator"]["keyframes"]
                .as_array()
                .unwrap()
                .clone()
        };
        let position_y = track("positionY");
        let easing_types: Vec<_> = position_y
            .iter()
            .map(|key| key["easing"]["type"].as_str().unwrap())
            .collect();
        assert_eq!(easing_types, easings);
        for property in ["positionX", "positionY"] {
            assert!(track(property).iter().all(|key| {
                key.get("spatialInTangent").is_none() && key.get("spatialOutTangent").is_none()
            }));
        }
        let scale_keys: Vec<_> = track("scaleX")
            .iter()
            .map(|key| {
                (
                    key["layerTime"].as_i64().unwrap(),
                    key["value"]["value"].as_f64().unwrap(),
                    key["easing"]["type"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        assert_eq!(
            scale_keys,
            [
                (0, 70.0, "linear".to_owned()),
                (1000, 0.0, "linear".to_owned())
            ]
        );
    }
}

#[test]
fn unprobed_bezier_graphic_keys_omit_the_graphic_until_their_speed_unit_is_verified() {
    // C1 measured only straight Bezier→Bezier Position and Text Rotation.
    // Curved Position and mixed Position/Rotation modes stay omitted. A
    // Bezier last key after a Linear key is still an unmeasured pair.
    let four_positions = format!(
        "{IN},0.5:0.7,5,0,0,0.33333333333333331,0.3,0.56,5,4,0,0,0,-0.03;\
         {},0.5:0.5,5,0,0,0.88,0,0.16666666666666666,5,4,0,0.03,0,0;\
         {},0.5:0.5,5,0,0,0.33333333333333331,0,0.47,5,4,0,0,0,0.03;\
         {},0.5:0.7,5,0,0.05,0.79,0.2,0.33333333333333331,5,4,0,-0.03,0,0;",
        IN + TICKS / 2,
        IN + TICKS,
        IN + 3 * TICKS / 2
    );
    let two_positions = format!(
        "{IN},0.5:0.7,5,0,0,0.33333333333333331,0.3,0.56,5,4,0,0,0,-0.03;{},0.5:0.5,0,0,0,0.88,0,0.16666666666666666,5,4,0,0.03,0,0;",
        IN + TICKS / 2
    );
    let text_positions = format!(
        "{IN},0.25:0.5,5,0,0,0.16666666666666666,0.1,0.3,0,0,0,0,0,0;{},0.3:0.55,0,0,0.1,0.3,0,0.16666666666666666,0,0,0,0,0,0;",
        IN + TICKS
    );
    let scalars = |from: f64, to: f64, speed: f64| {
        format!(
            "{IN},{from},5,0,0,0.16666666666666666,{speed},0.3;{},{to},0,0,{speed},0.3,0,0.16666666666666666;",
            IN + TICKS
        )
    };
    let bezier_last_key = format!(
        "{IN},0.,0,0,0,0.16666666666666666,20,0.16666666666666666;{},20.,5,0,20,0.16666666666666666,0,0.3;",
        IN + TICKS
    );
    for (object, keys, name) in [
        (31, four_positions, "Position"),
        (31, two_positions, "Position"),
        (43, text_positions, "Position"),
        (47, scalars(0.0, 20.0, 20.0), "Rotation"),
        (47, bezier_last_key, "Rotation"),
        (
            31,
            format!("{IN},0.5:0.5,4,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;{},0.6:0.6,5,0,0.1,0.3,0,0.16666666666666666,0,0,0,0,0,0;", IN + TICKS),
            "Position",
        ),
        (47, into_a_bezier_key(4, -6.0, 0.25), "Rotation"),
    ] {
        let xml = keyed(&graphic_xml(BEFORE), object, &keys);
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        let sequence = project.single_sequence().unwrap();
        assert_eq!(sequence.video_items().count(), 1, "{object}: {keys}");
        assert_eq!(sequence.video_occurrences().count(), 1, "{object}: {keys}");
        let reason = format!(
            "Bezier keys on graphic {name:?} are unsupported until their speed unit is verified"
        );
        assert!(
            omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Occurrence
                    && omission.record == "20"
                    && omission.reason.contains(&reason)),
            "{object}: {keys}: {omissions:?}"
        );
    }
}

/// The placement `InPoint` (one hour into the generator) of the Bezier speed
/// probe that Premiere 26.5.1 saved.
const PROBE_IN: i64 = 914_457_600_000_000;

/// The probe's key strings as Premiere saved them: one 1.2 s segment per
/// parameter, with Bezier (mode 5) on both keys.
const PROBE_VM_SCALE: &str = "914533804800000,100.,5,0,0,0.16666666666666666,-6.5,0.5;914838624000000,50.,5,0,-1,0.25,0,0.16666666666666666;";
const PROBE_VM_ROTATION: &str = "914914828800000,0.,5,0,0,0.16666666666666666,8,0.5;915219648000000,60.,5,0,12,0.25,0,0.16666666666666666;";
const PROBE_TEXT_SCALE: &str = "915295852800000,100.,5,0,0,0.16666666666666666,13,0.5;915600672000000,200.,5,0,20,0.25,0,0.16666666666666666;";
const PROBE_TEXT_OPACITY: &str = "915676876800000,100.,5,0,0,0.16666666666666666,-10.5,0.5;915981696000000,20.,5,0,-16,0.25,0,0.16666666666666666;";

/// Eased progress of an FX cubic Bezier at linear progress `x`: the curve's
/// y where its x is `x`, the root that `fx_model`'s
/// `cubic_bezier_y_and_derivative_for_x` finds, here by bisection.
fn cubic_bezier_progress(easing: &serde_json::Value, x: f64) -> f64 {
    let handle = |name: &str| easing[name].as_f64().unwrap();
    let curve = |first: f64, second: f64, t: f64| {
        let inverse = 1.0 - t;
        3.0 * inverse * inverse * t * first + 3.0 * inverse * t * t * second + t * t * t
    };
    let (mut lower, mut upper) = (0.0, 1.0);
    for _ in 0..60 {
        let middle = (lower + upper) / 2.0;
        if curve(handle("x1"), handle("x2"), middle) < x {
            lower = middle;
        } else {
            upper = middle;
        }
    }
    curve(handle("y1"), handle("y2"), (lower + upper) / 2.0)
}

/// A scalar FX track's value at `layer_ms`, as `fx_composition` evaluates
/// keys: a key's own value on it, the nearest key's value outside the keys,
/// and the later key's easing between two keys.
fn evaluate(keys: &[serde_json::Value], layer_ms: f64) -> f64 {
    let time = |key: &serde_json::Value| key["layerTime"].as_f64().unwrap();
    let value = |key: &serde_json::Value| key["value"]["value"].as_f64().unwrap();
    let Some(right) = keys.iter().position(|key| time(key) > layer_ms) else {
        return value(keys.last().unwrap());
    };
    if right == 0 {
        return value(&keys[0]);
    }
    let (from, to) = (&keys[right - 1], &keys[right]);
    let linear = (layer_ms - time(from)) / (time(to) - time(from));
    let progress = match to["easing"]["type"].as_str().unwrap() {
        "linear" => linear,
        "hold" => 0.0,
        "cubicBezier" => cubic_bezier_progress(&to["easing"], linear),
        other => panic!("unexpected easing {other}"),
    };
    value(from) + (value(to) - value(from)) * progress
}

#[test]
fn probed_bezier_graphic_keys_import_as_premiere_reads_them_back() {
    // The probe's segments on a placement that starts one hour into the
    // generator, as in the probe, so layer time is key time minus one hour.
    let mut xml = graphic_xml(BEFORE).replace(
        "<InPoint>914161248000000</InPoint><OutPoint>914669280000000</OutPoint>",
        &format!(
            "<InPoint>{PROBE_IN}</InPoint><OutPoint>{}</OutPoint>",
            PROBE_IN + 2 * TICKS
        ),
    );
    for (object, keys) in [
        (32, PROBE_VM_SCALE),
        (35, PROBE_VM_ROTATION),
        (44, PROBE_TEXT_SCALE),
        (48, PROBE_TEXT_OPACITY),
    ] {
        xml = keyed(&xml, object, keys);
    }
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let document = crate::tests::support::project_document_with_media(sequence, &project.media);
    let group = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group")
        .expect("keyed Vector Motion makes a graphic group");
    let text = &group["layers"][0];
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    let track = |layer: &serde_json::Value, property: &str| {
        entries
            .iter()
            .find(|entry| {
                entry["target"]["layerId"] == layer["id"]
                    && entry["target"]["propertyType"] == property
            })
            .unwrap_or_else(|| panic!("{property} track of layer {}", layer["id"]))["animator"]
            ["keyframes"]
            .as_array()
            .unwrap()
            .clone()
    };
    // What Premiere reported for the saved keys (`get_value_at_time`) at 10,
    // 25, 50, 75 and 90 % of each segment.
    let vm_scale = [98.6232, 94.2207, 80.5251, 61.5361, 52.3142];
    let text_scale = [102.6819, 111.1185, 137.3743, 174.5298, 193.7557];
    for (layer, property, start_ms, readback) in [
        (group, "scaleX", 300.0, vm_scale),
        (group, "scaleY", 300.0, vm_scale),
        (
            group,
            "rotation",
            1800.0,
            [1.6306, 6.715, 22.4763, 44.7432, 56.2589],
        ),
        (text, "scaleX", 3300.0, text_scale),
        (text, "scaleY", 3300.0, text_scale),
        (
            text,
            "opacity",
            4800.0,
            [97.8438, 91.0833, 70.0747, 40.3635, 24.9927],
        ),
    ] {
        let keys = track(layer, property);
        assert_eq!(keys[1]["easing"]["type"], "cubicBezier", "{property}");
        for (fraction, expected) in [0.1, 0.25, 0.5, 0.75, 0.9].into_iter().zip(readback) {
            let actual = evaluate(&keys, start_ms + 1200.0 * fraction);
            assert!(
                (actual - expected).abs() < 1e-4,
                "{property} at {fraction} of its segment: {actual} != {expected}"
            );
        }
    }
}

/// A 60 to 40 segment over one second (-20 per second on average) from a
/// Linear (`0`) or Hold (`4`) key into a Bezier key with the given in-handle.
/// The start key's stored out-handle (speed 0 at 1/6) bends the segment after
/// a Linear key, which the Motion reader eases with both handles (F23).
fn into_a_bezier_key(start_mode: u8, speed: f64, influence: f64) -> String {
    format!(
        "{IN},60.,{start_mode},0,0,0.16666666666666666,0,0.16666666666666666;{},40.,5,0,{speed},{influence},0,0.16666666666666666;",
        IN + TICKS
    )
}

/// [`into_a_bezier_key`] from a Linear key whose stored out-handle is neutral
/// (no influence): with a neutral in-handle too, the one Linear-into-Bezier
/// form that a graphic converts.
fn into_a_bezier_key_after_a_neutral_linear_key(speed: f64, influence: f64) -> String {
    format!(
        "{IN},60.,0,0,0,0.16666666666666666,0,0;{},40.,5,0,{speed},{influence},0,0.16666666666666666;",
        IN + TICKS
    )
}

#[test]
fn a_probed_bezier_key_after_a_linear_or_hold_key_omits_the_graphic_unless_its_in_handle_is_neutral(
) {
    use PrKeyframeEasing::{Hold, Linear};
    // The probe had Bezier on both keys: whether Premiere bends a Linear or
    // Hold segment into a Bezier key is unmeasured on graphics. Neutral
    // handles, with no influence or at the segment's average speed (-20 per
    // second here), keep the segment straight either way.
    for (object, keys, name) in [
        (
            48,
            into_a_bezier_key(0, -6.0, 0.16666666666666666),
            "Opacity",
        ),
        (48, into_a_bezier_key(0, -6.0, 0.0001), "Opacity"),
        (35, into_a_bezier_key(4, 12.0, 0.25), "Rotation"),
        // A neutral in-handle after the Linear key's bent out-handle.
        (48, into_a_bezier_key(0, -6.0, 0.0), "Opacity"),
        (48, into_a_bezier_key(0, -20.0, 0.25), "Opacity"),
    ] {
        let xml = keyed(&graphic_xml(BEFORE), object, &keys);
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        let items = project.single_sequence().unwrap().video_items().count();
        assert_eq!(items, 1, "{object}: {keys}");
        let reason = format!(
            "Bezier key after a Linear or Hold key on graphic {name:?} is unsupported until its interpolation is verified"
        );
        assert!(
            omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Occurrence
                    && omission.record == "20"
                    && omission.reason.contains(&reason)),
            "{object}: {keys}: {omissions:?}"
        );
    }
    let opacity = |easing| {
        vec![PrPropertyAnimation::Opacity(vec![
            scalar(IN, 60.0, Linear),
            scalar(IN + TICKS, 40.0, easing),
        ])]
    };
    for keys in [
        into_a_bezier_key_after_a_neutral_linear_key(-6.0, 0.0),
        into_a_bezier_key_after_a_neutral_linear_key(-20.0, 0.25),
    ] {
        let graphic = graphic(&keyed(&graphic_xml(BEFORE), 48, &keys));
        assert_eq!(graphic.text().animations, opacity(Linear), "{keys}");
    }
    let graphic = graphic(&keyed(
        &graphic_xml(BEFORE),
        35,
        &into_a_bezier_key(4, -20.0, 0.25),
    ));
    let motion = graphic.vector_motion.expect("keyed Vector Motion");
    assert_eq!(
        motion.animations,
        [PrPropertyAnimation::Rotation(vec![
            scalar(IN, 60.0, Linear),
            scalar(IN + TICKS, 40.0, Hold),
        ])]
    );
}

#[test]
fn measured_text_scale_still_rejects_invalid_bezier_influences() {
    let keys = into_a_bezier_key(0, -6.0, -0.25);
    let (project, omissions) =
        inspect_project_with_omissions(&keyed(&text_only_xml(), 44, &keys), None).unwrap();
    assert_eq!(project.single_sequence().unwrap().video_items().count(), 1);
    assert!(
        omissions.iter().any(|omission| omission
            .reason
            .contains("Bezier influence must be between zero and one")),
        "{omissions:?}"
    );
}

#[test]
fn unmeasured_graphic_mode_pairs_require_exactly_neutral_bezier_handles() {
    use crate::schema::PrAnimatedProperty::{Opacity, Rotation, UniformScale};
    use PrKeyframeEasing::{Hold, Linear};
    // The in-handle is compared exactly. A speed a hair off the average of
    // -20 per second bends the segment, and so does any nonzero influence,
    // even a negative one, which Premiere never saves. Only an influence of
    // zero or the exact average speed reads the segment as its start key says;
    // after a Linear key, only with the key's out-handle neutral too.
    for (start_mode, easing) in [(0, Linear), (4, Hold)] {
        for (xml, object, name, property) in [
            (text_only_xml(), 44, "Scale", UniformScale),
            (text_only_xml(), 48, "Opacity", Opacity),
            (graphic_xml(BEFORE), 32, "Scale", UniformScale),
            (graphic_xml(BEFORE), 35, "Rotation", Rotation),
        ] {
            // C1 independently measured these two Text mode pairs. Their
            // positive values and curves are asserted on the native fixture.
            if matches!((object, start_mode), (44, 0) | (48, 4)) {
                continue;
            }
            for (speed, influence) in [(-20.00000001, 0.25), (-19.99999999, 0.25), (-6.0, -0.25)] {
                let keys = into_a_bezier_key(start_mode, speed, influence);
                let (project, omissions) =
                    inspect_project_with_omissions(&keyed(&xml, object, &keys), None).unwrap();
                let graphics = project
                    .single_sequence()
                    .unwrap()
                    .video_items()
                    .filter(|item| item.graphic().is_some())
                    .count();
                assert_eq!(graphics, 0, "{object}: {keys}");
                // After a Linear key the Motion reader eases into the Bezier
                // key (F23), whose check rejects a negative influence first.
                let reason = if start_mode == 0 && influence < 0.0 {
                    "Bezier influence must be between zero and one".to_owned()
                } else {
                    format!(
                        "Bezier key after a Linear or Hold key on graphic {name:?} is unsupported until its interpolation is verified"
                    )
                };
                assert!(
                    omissions
                        .iter()
                        .any(|omission| omission.scope == OmissionScope::Occurrence
                            && omission.record == "20"
                            && omission.reason.contains(&reason)),
                    "{object}: {keys}: {omissions:?}"
                );
            }
            for (speed, influence) in [(-20.0, 0.25), (-6.0, 0.0)] {
                let keys = match start_mode {
                    0 => into_a_bezier_key_after_a_neutral_linear_key(speed, influence),
                    _ => into_a_bezier_key(start_mode, speed, influence),
                };
                let graphic = graphic(&keyed(&xml, object, &keys));
                let animations = match &graphic.vector_motion {
                    Some(motion) => motion.animations.clone(),
                    None => graphic.text().animations.clone(),
                };
                let [animation] = animations.as_slice() else {
                    panic!("{object}: {keys}: {animations:?}");
                };
                assert_eq!(animation.property(), property, "{object}: {keys}");
                assert_eq!(
                    animation.keys(),
                    [scalar(IN, 60.0, Linear), scalar(IN + TICKS, 40.0, easing)],
                    "{object}: {keys}"
                );
            }
        }
    }
}

#[test]
fn graphic_keys_spanning_the_whole_tick_range_read_as_the_motion_reader_reads_them() {
    use super::animation::animation_fixture::animated_xml;
    // Malformed but increasing times, from i64::MIN to i64::MAX: the clip
    // Motion reader keeps them. Vector Motion Rotation takes the in-handle
    // rule, which divides by each segment's duration, and must keep the same
    // keys as Motion: an all-Linear track, a Linear key into a Bezier key
    // with handles of no influence or at exactly the average speed over the
    // whole span, and a Bezier key into a Linear one.
    let (min, max) = (i64::MIN, i64::MAX);
    let average = -20.0 / ((i128::from(max) - i128::from(min)) as f64 / TICKS as f64);
    for keys in [
        format!("{min},60.,0,0,0,0.16666666666666666,0,0.16666666666666666;{max},40.,0,0,0,0.16666666666666666,0,0.16666666666666666;"),
        format!("{min},60.,0,0,0,0.16666666666666666,0,0;{max},40.,5,0,-20,0,0,0.16666666666666666;"),
        format!("{min},60.,0,0,0,0.16666666666666666,{average},0.16666666666666666;{max},40.,5,0,{average},0.25,0,0.16666666666666666;"),
        format!("{min},0.,5,0,0,0,3,0.4;{max},90.,0,0,2,0.2,0,0;"),
    ] {
        let project = inspect_project(&animated_xml(&keys), Some("sequence-1")).unwrap();
        let clip = project.video_occurrences().next().unwrap();
        assert_eq!(clip.animations.len(), 1, "{keys}");
        let graphic = graphic(&keyed(&graphic_xml(BEFORE), 35, &keys));
        let motion = graphic.vector_motion.expect("keyed Vector Motion");
        assert_eq!(motion.animations, clip.animations, "{keys}");
    }
    // A bent in-handle over that span still omits the graphic.
    let keys = format!("{min},60.,0,0,0,0.16666666666666666,0,0.16666666666666666;{max},40.,5,0,-20,0.25,0,0.16666666666666666;");
    let (project, omissions) =
        inspect_project_with_omissions(&keyed(&graphic_xml(BEFORE), 35, &keys), None).unwrap();
    assert_eq!(project.single_sequence().unwrap().video_items().count(), 1);
    assert!(
        omissions.iter().any(|omission| omission.record == "20"
            && omission
                .reason
                .contains("Bezier key after a Linear or Hold key on graphic \"Rotation\"")),
        "{omissions:?}"
    );
}

/// The intrinsic clip Opacity of a graphic as Premiere 26.5.1 saved a keyed
/// one (case B, `premiere_isolated_graphic_clip_opacity_keys_26_5`): component
/// `ID` 2 in the 26.5 layout, renumbered to records 70 to 73.
pub(super) const CLIP_OPACITY: &str = r#"
  <VideoFilterComponent ObjectID="70" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="9"><Component Version="7"><Params Version="1"><Param Index="0" ObjectRef="71"/><Param Index="1" ObjectRef="72"/><Param Index="2" ObjectRef="73"/></Params><ID>2</ID><Intrinsic>true</Intrinsic><DisplayName>Opacity</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Opacity</MatchName></VideoFilterComponent>
  <VideoComponentParam ObjectID="71" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Opacity</Name>KEYED<ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,OPACITY,0,0,0,0,0,0</StartKeyframe>KEYS<LowerBound>0</LowerBound><UpperBound>100</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="72" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="10"><Name>Blend Mode</Name><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>10</ParameterControlType><ParameterID>2</ParameterID><StartKeyframe>-91445760000000000,PRIMARY,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>27</UpperBound></VideoComponentParam>
  <VideoComponentParam ObjectID="73" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="10"><Name>Blend Mode</Name><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterID>3</ParameterID><StartKeyframe>-91445760000000000,LEGACY,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>31</UpperBound></VideoComponentParam>"#;

/// The chain components of a graphic that keeps its clip Opacity: case B's
/// order, and the same with Vector Motion between the Opacity and the Text,
/// an order inferred from case B and case A's [Vector Motion, Text]: no Adobe
/// save has both.
const OPACITY_AND_TEXT: &str =
    r#"<Component Index="0" ObjectRef="70"/><Component Index="1" ObjectRef="40"/>"#;
const OPACITY_MOTION_AND_TEXT: &str = r#"<Component Index="0" ObjectRef="70"/><Component Index="1" ObjectRef="30"/><Component Index="2" ObjectRef="40"/>"#;

/// The default fields of `graphic_xml`'s graphic chain, and the fields that
/// Premiere 26.5.1 keeps when the clip keeps its Opacity.
const GRAPHIC_CHAIN_DEFAULTS: &str = r#"<VideoComponentChain ObjectID="21"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><DefaultMotionComponentID>1</DefaultMotionComponentID><DefaultOpacityComponentID>2</DefaultOpacityComponentID>"#;
const OPACITY_CHAIN_DEFAULTS: &str = r#"<VideoComponentChain ObjectID="21"><DefaultMotion>true</DefaultMotion><DefaultMotionComponentID>1</DefaultMotionComponentID>"#;

/// `graphic_xml` whose graphic keeps its own clip Opacity, as Premiere 26.5.1
/// saves it: `components` in the chain, no `DefaultOpacity`, and the clip
/// Opacity starting at `opacity` with `keys` and the blend pair `blend`.
fn with_clip_opacity(components: &str, opacity: &str, keys: &str, blend: (u8, u8)) -> String {
    let (keyed, keys) = if keys.is_empty() {
        (String::new(), String::new())
    } else {
        (
            "<IsTimeVarying>true</IsTimeVarying>".to_owned(),
            format!("<Keyframes>{keys}</Keyframes>"),
        )
    };
    let records = CLIP_OPACITY
        .replace("KEYED", &keyed)
        .replace("OPACITY", opacity)
        .replace("KEYS", &keys)
        .replace("PRIMARY", &blend.0.to_string())
        .replace("LEGACY", &blend.1.to_string());
    graphic_xml(BEFORE)
        .replace(GRAPHIC_CHAIN_DEFAULTS, OPACITY_CHAIN_DEFAULTS)
        .replace(TWO_COMPONENTS, components)
        .replace("</PremiereData>", &format!("{records}\n</PremiereData>"))
}

/// Case B's saved clip Opacity keys (Linear, Hold, then a last key after the
/// Out), `in_ticks` into the generator instead of at zero.
fn case_b_opacity_keys(in_ticks: i64) -> String {
    format!(
        "{},100.,0,0,0,0.16666666666666666,-1,0.16666666666666666;{},0.,4,0,-1,0.16666666666666666,0,0.33333333333333331;{},70.,0,0,70,0.16666666666666666,0,0.16666666666666666;",
        in_ticks + TICKS / 2,
        in_ticks + 3 * TICKS / 2,
        in_ticks + 5 * TICKS / 2
    )
}

#[test]
fn a_keyed_graphic_clip_opacity_reads_on_the_generator_clock() {
    use crate::schema::PrBlendMode;
    use PrKeyframeEasing::{Hold, Linear};
    // Key times are generator times, as for the text keys: the Bezier probe
    // keyed a clip Opacity one hour into the generator.
    for components in [OPACITY_AND_TEXT, OPACITY_MOTION_AND_TEXT] {
        let graphic = graphic(&with_clip_opacity(
            components,
            "100.",
            &case_b_opacity_keys(IN),
            (18, 0),
        ));
        assert_eq!(
            (graphic.opacity, graphic.blend_mode),
            (100.0, PrBlendMode::Normal)
        );
        assert_eq!(
            graphic.animations,
            [PrPropertyAnimation::Opacity(vec![
                scalar(IN + TICKS / 2, 100.0, Linear),
                scalar(IN + 3 * TICKS / 2, 0.0, Linear),
                scalar(IN + 5 * TICKS / 2, 70.0, Hold),
            ])],
            "{components}"
        );
        // The text keeps its own Opacity, and a static Vector Motion still
        // composes into the text.
        assert_eq!(graphic.text().transform.opacity, 60.0);
        assert!(graphic.vector_motion.is_none());
    }
    // The clip Opacity's blend pair reads as for a media clip.
    for (pair, blend_mode) in [
        ((18, 0), PrBlendMode::Normal),
        ((22, 10), PrBlendMode::Screen),
    ] {
        let graphic = graphic(&with_clip_opacity(OPACITY_AND_TEXT, "50.", "", pair));
        assert_eq!((graphic.opacity, graphic.blend_mode), (50.0, blend_mode));
        assert!(graphic.animations.is_empty());
    }
}

#[test]
fn graphic_clip_opacity_forms_that_do_not_convert_omit_the_graphic() {
    let keys = case_b_opacity_keys(IN);
    for (xml, reason) in [
        (
            with_clip_opacity(OPACITY_AND_TEXT, "100.", &keys, (18, 0)).replace(
                OPACITY_CHAIN_DEFAULTS,
                &OPACITY_CHAIN_DEFAULTS.replace(
                    "</DefaultMotion>",
                    "</DefaultMotion><DefaultOpacity>true</DefaultOpacity>",
                ),
            ),
            "explicit Opacity conflicts with DefaultOpacity",
        ),
        // The clip Opacity comes first; after the Text it is no graphic layout.
        (
            with_clip_opacity(
                r#"<Component Index="0" ObjectRef="40"/><Component Index="1" ObjectRef="70"/>"#,
                "100.",
                &keys,
                (18, 0),
            ),
            "effect \"AE.ADBE Opacity\" in a graphic is unsupported",
        ),
        // Premiere's Opacity range bounds the keys, as for media clips.
        (
            with_clip_opacity(
                OPACITY_AND_TEXT,
                "100.",
                &keys.replace(",70.,", ",120.,"),
                (18, 0),
            ),
            "graphic clip Opacity keys must be between 0 and 100",
        ),
        // Clip Motion keys keep today's message.
        (
            with_clip_opacity(OPACITY_AND_TEXT, "100.", &keys, (18, 0)).replace(
                OPACITY_CHAIN_DEFAULTS,
                &OPACITY_CHAIN_DEFAULTS.replace(">true</DefaultMotion>", ">false</DefaultMotion>"),
            ),
            "graphic clip Motion and Opacity must keep their defaults",
        ),
    ] {
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        let sequence = project.single_sequence().unwrap();
        assert_eq!(sequence.video_items().count(), 1, "{reason}");
        assert!(
            omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Occurrence
                    && omission.record == "20"
                    && omission.reason.contains(reason)),
            "{reason}: {omissions:?}"
        );
    }
}

/// The media clip's chain in `graphic_xml`'s one-clip project, and the
/// `DefaultOpacity` field of both default chains.
const MEDIA_CHAIN: &str = r#"<VideoComponentChain ObjectID="4"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>"#;
const DEFAULT_OPACITY: &str = "<DefaultOpacity>true</DefaultOpacity>";

#[test]
fn a_graphic_chain_reads_default_opacity_as_a_media_clip_chain_does() {
    use crate::{schema::PrBlendMode, Omission};
    // One shared video reader reads both chains, so each form gives the graphic
    // (record 20) and the media clip (record 3) the same outcome: the opacity
    // read, or the reason the occurrence is omitted. Without a clip Opacity,
    // `DefaultOpacity` `true` or none reads 100, and any other value omits the
    // clip. No Adobe-saved or corpus graphic has neither, so reading that form
    // on a graphic is inferred from media clips. Beside a kept clip Opacity
    // (static 50 here) only `true` omits the clip: none, as Premiere 26.5.1
    // saves it, `false` and any other value read the Opacity component. Either
    // way, a value other than `true` is also reported on the chain as a feature
    // that is not converted.
    for (kept_opacity, default_opacity, expected) in [
        (false, None, Ok(100.0)),
        (false, Some("true"), Ok(100.0)),
        (false, Some("false"), Err("nondefault opacity")),
        (false, Some("1"), Err("nondefault opacity")),
        (true, None, Ok(50.0)),
        (
            true,
            Some("true"),
            Err("explicit Opacity conflicts with DefaultOpacity"),
        ),
        (true, Some("false"), Ok(50.0)),
        (true, Some("1"), Ok(50.0)),
    ] {
        let field = default_opacity.map_or_else(String::new, |value| {
            format!("<DefaultOpacity>{value}</DefaultOpacity>")
        });
        // Each project gives one chain the form under test and keeps the other
        // chain's default, because a project with nothing left to convert fails.
        let graphic_form = if kept_opacity {
            with_clip_opacity(OPACITY_AND_TEXT, "50.", "", (18, 0)).replace(
                OPACITY_CHAIN_DEFAULTS,
                &OPACITY_CHAIN_DEFAULTS
                    .replace("</DefaultMotion>", &format!("</DefaultMotion>{field}")),
            )
        } else {
            graphic_xml(BEFORE).replace(
                GRAPHIC_CHAIN_DEFAULTS,
                &GRAPHIC_CHAIN_DEFAULTS.replace(DEFAULT_OPACITY, &field),
            )
        };
        let media_form = if kept_opacity {
            // The media clip keeps its own copy of the clip Opacity, records 80
            // to 83.
            let records = CLIP_OPACITY
                .replace(r#"ObjectID="7"#, r#"ObjectID="8"#)
                .replace(r#"ObjectRef="7"#, r#"ObjectRef="8"#)
                .replace("KEYED", "")
                .replace("OPACITY", "50.")
                .replace("KEYS", "")
                .replace("PRIMARY", "18")
                .replace("LEGACY", "0");
            let chain = format!(
                r#"<VideoComponentChain ObjectID="4"><DefaultMotion>true</DefaultMotion>{field}<ComponentChain><Components><Component Index="0" ObjectRef="80"/></Components></ComponentChain></VideoComponentChain>"#
            );
            graphic_xml(BEFORE)
                .replace(MEDIA_CHAIN, &chain)
                .replace("</PremiereData>", &format!("{records}\n</PremiereData>"))
        } else {
            graphic_xml(BEFORE).replace(MEDIA_CHAIN, &MEDIA_CHAIN.replace(DEFAULT_OPACITY, &field))
        };
        for (xml, record, chain, opacity_component) in [
            (
                graphic_form,
                "20",
                "VideoComponentChain:21",
                r#"ObjectRef="70""#,
            ),
            (
                media_form,
                "3",
                "VideoComponentChain:4",
                r#"ObjectRef="80""#,
            ),
        ] {
            let context = format!(
                "{record}: kept clip Opacity {kept_opacity}, DefaultOpacity {default_opacity:?}"
            );
            // The other chain keeps its `DefaultOpacity` `true`.
            assert_eq!(
                (
                    xml.matches("<DefaultOpacity>").count(),
                    xml.contains(opacity_component)
                ),
                (1 + usize::from(default_opacity.is_some()), kept_opacity),
                "{context}"
            );
            let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
            let sequence = project.single_sequence().unwrap();
            let read = if record == "20" {
                sequence
                    .video_items()
                    .find_map(PrVideoItem::graphic)
                    .map(|graphic| {
                        (
                            graphic.opacity,
                            graphic.blend_mode,
                            graphic.animations.len(),
                        )
                    })
            } else {
                sequence
                    .video_items()
                    .find_map(PrVideoItem::media)
                    .map(|clip| (clip.opacity, clip.blend_mode, clip.animations.len()))
            };
            let mut omitted = Vec::new();
            if default_opacity.is_some_and(|value| value != "true") {
                omitted.push(Omission {
                    scope: OmissionScope::Feature,
                    kind: OmissionKind::Omitted,
                    record: chain.to_owned(),
                    reason: "nondefault DefaultOpacity not converted".to_owned(),
                });
            }
            if let Err(reason) = expected {
                omitted.push(Omission {
                    scope: OmissionScope::Occurrence,
                    kind: OmissionKind::Omitted,
                    record: record.to_owned(),
                    reason: format!("unsupported conversion: {chain}: {reason}"),
                });
            }
            assert_eq!(omissions, omitted, "{context}");
            assert_eq!(
                read,
                expected
                    .ok()
                    .map(|opacity| (opacity, PrBlendMode::Normal, 0)),
                "{context}"
            );
        }
    }
}

/// The probe's clip Opacity key string as Premiere saved it: one 1.2 s
/// segment with Bezier (mode 5) on both keys.
const PROBE_CLIP_OPACITY: &str = "916057900800000,100.,5,0,0,0.16666666666666666,-10.5,0.5;916362720000000,20.,5,0,-16,0.25,0,0.16666666666666666;";

#[test]
fn probed_bezier_clip_opacity_keys_import_as_premiere_reads_them_back() {
    // On a placement that starts one hour into the generator, as in the
    // probe, so layer time is key time minus one hour.
    let xml = with_clip_opacity(OPACITY_AND_TEXT, "100.", PROBE_CLIP_OPACITY, (18, 0)).replace(
        "<InPoint>914161248000000</InPoint><OutPoint>914669280000000</OutPoint>",
        &format!(
            "<InPoint>{PROBE_IN}</InPoint><OutPoint>{}</OutPoint>",
            PROBE_IN + 2 * TICKS
        ),
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let document = crate::tests::support::project_document_with_media(sequence, &project.media);
    let group = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group")
        .expect("a keyed clip Opacity makes a graphic group");
    let keys = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == group["id"]
                && entry["target"]["propertyType"] == "opacity"
        })
        .expect("the group's opacity track")["animator"]["keyframes"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(keys[1]["easing"]["type"], "cubicBezier");
    // What Premiere reported for the saved keys (`get_value_at_time`) at 10,
    // 25, 50, 75 and 90 % of the segment.
    for (fraction, expected) in [0.1, 0.25, 0.5, 0.75, 0.9]
        .into_iter()
        .zip([97.8438, 91.0833, 70.0747, 40.3635, 24.9927])
    {
        let actual = evaluate(&keys, 6300.0 + 1200.0 * fraction);
        assert!(
            (actual - expected).abs() < 1e-4,
            "at {fraction} of the segment: {actual} != {expected}"
        );
    }
}

#[test]
fn a_bezier_key_after_a_linear_or_hold_key_on_graphic_clip_opacity_omits_the_graphic_unless_its_in_handle_is_neutral(
) {
    use PrKeyframeEasing::Linear;
    // The rule of the probed graphic parameters: the probe had Bezier on both
    // keys, so a bent in-handle after a Linear or Hold key, or a bent
    // out-handle of the Linear key, is unmeasured.
    for keys in [
        into_a_bezier_key(0, -6.0, 0.16666666666666666),
        into_a_bezier_key(4, -6.0, 0.0001),
        into_a_bezier_key(0, -6.0, 0.0),
        into_a_bezier_key(0, -20.0, 0.25),
    ] {
        let xml = with_clip_opacity(OPACITY_AND_TEXT, "60.", &keys, (18, 0));
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        assert_eq!(project.single_sequence().unwrap().video_items().count(), 1);
        assert!(
            omissions.iter().any(|omission| {
                omission.scope == OmissionScope::Occurrence
                    && omission.record == "20"
                    && omission.reason.contains(
                        "Bezier key after a Linear or Hold key on graphic clip Opacity is unsupported until its interpolation is verified",
                    )
            }),
            "{keys}: {omissions:?}"
        );
    }
    for keys in [
        into_a_bezier_key_after_a_neutral_linear_key(-6.0, 0.0),
        into_a_bezier_key_after_a_neutral_linear_key(-20.0, 0.25),
    ] {
        let graphic = graphic(&with_clip_opacity(OPACITY_AND_TEXT, "60.", &keys, (18, 0)));
        assert_eq!(
            graphic.animations,
            [PrPropertyAnimation::Opacity(vec![
                scalar(IN, 60.0, Linear),
                scalar(IN + TICKS, 40.0, Linear),
            ])],
            "{keys}"
        );
    }
}

/// The clip Opacity keys that the first export of an edited graphic clip
/// Opacity wrote and Premiere 26.5.1 reopened: 100 at generator 3600.5 s with
/// a Bezier out-handle (-33.33/s over 0.4), 0 at 3601.25 s starting a Hold
/// (stored in-handle -26.67/s over 0.25), and 70 at 3602.5 s.
const HOLD_END_GATE_CLIP_OPACITY: &str = "914584608000000,100,5,0,0,0,-33.333333333333336,0.4;914775120000000,0,4,0,-26.666666666666693,0.25,0,0;915092640000000,70,0,0,0,0,0,0;";

#[test]
fn a_bezier_segment_into_a_hold_key_on_graphic_clip_opacity_reads_as_premiere_reads_it_back() {
    use super::animation::bezier_progress;
    let graphic = graphic(&with_clip_opacity(
        OPACITY_AND_TEXT,
        "100.",
        HOLD_END_GATE_CLIP_OPACITY,
        (18, 0),
    ));
    let [PrPropertyAnimation::Opacity(keys)] = graphic.animations.as_slice() else {
        panic!("{:?}", graphic.animations);
    };
    // The segment follows the first key's out-handle and arrives at the key
    // that starts the Hold with a zero-length handle, ignoring the stored one.
    let PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = keys[1].easing else {
        panic!("{keys:?}");
    };
    assert_eq!((x1, x2, y2), (0.4, 1.0, 1.0));
    assert!((y1 - 0.1).abs() < 1e-12, "{y1}");
    assert_eq!(keys[2].easing, PrKeyframeEasing::Hold);
    // Premiere's readbacks of the segment (generator seconds), which the
    // stored in-handle misses by up to 12.2.
    for (seconds, readback) in [
        (3600.575, 96.122032),
        (3600.6875, 86.353004),
        (3600.875, 63.138905),
        (3601.0625, 33.993603),
        (3601.175, 14.21875),
        (3601.2125, 7.238911),
    ] {
        let value = 100.0 - 100.0 * bezier_progress(keys[1].easing, (seconds - 3600.5) / 0.75);
        assert!(
            (value - readback).abs() < 1e-5,
            "{seconds}: {value} != {readback}"
        );
    }
}

#[test]
fn animation_flags_follow_the_motion_rules_for_graphic_keys() {
    // Premiere 14 omits the flag on keyed parameters; `false` would hide keys.
    for (flag, converts) in [(None, true), (Some("true"), true), (Some("false"), false)] {
        let xml = with_keys(&text_only_xml(), 44, flag, TWO_SCALAR_KEYS);
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        let graphics = project
            .single_sequence()
            .unwrap()
            .video_items()
            .filter(|item| item.graphic().is_some())
            .count();
        assert_eq!(graphics, usize::from(converts), "{flag:?}: {omissions:?}");
        if !converts {
            assert!(
                omissions.iter().any(|omission| omission
                    .reason
                    .contains("animated or unknown graphic parameters are unsupported")),
                "{omissions:?}"
            );
        }
    }
}

#[test]
fn graphic_text_reads_with_its_vector_motion_composed() {
    let xml = graphic_xml(BEFORE);
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    // The generator media is not a project media source.
    assert_eq!(project.media.len(), 1);
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.video_occurrences().count(), 1);

    let graphic = graphic(&xml);
    assert_eq!(graphic.timeline_ticks(), TICKS..3 * TICKS);
    assert_eq!(graphic.text().name, "Before label");
    let document = &graphic.text().document;
    assert_eq!(
        (document.text.as_str(), document.font.as_str()),
        ("Before", "OpenSans-Bold")
    );
    assert_eq!(document.justification, PrJustification::Left);
    assert_eq!(
        document.frame,
        PrTextFrame::Point {
            vertical: PrVerticalAlign::Top
        }
    );
    // Vector Motion maps (x, y) to (480, 270) + 0.5 * R(90°) * ((x, y) - (960, 540)).
    // The text origin (1440, 540) therefore lands at (480, 510).
    let transform = graphic.text().transform;
    assert_close(transform.position, [480.0, 510.0]);
    assert_close(transform.anchor, [19.2, 21.6]);
    assert_eq!(
        (transform.scale, transform.rotation, transform.opacity),
        (50.0, 90.0, 60.0)
    );

    let layer_only =
        graphic_xml(BEFORE).replace(TWO_COMPONENTS, r#"<Component Index="0" ObjectRef="40"/>"#);
    let transform = self::graphic(&layer_only).text().transform;
    assert_eq!(transform.position, [1440.0, 540.0]);
    assert_eq!((transform.scale, transform.rotation), (100.0, 0.0));
}

#[test]
fn color_matte_and_graphic_text_route_by_their_generator_markers() {
    // Structural interaction only: replace the existing video under the graphic
    // with COLR media; this is not an Adobe-authored matte/text reference.
    let xml = graphic_xml(BEFORE)
        .replace(
            r#"<Media ObjectUID="media-1"><VideoStream ObjectRef="8"/><RelativePath>media/source.mp4</RelativePath></Media>"#,
            r#"<Media ObjectUID="media-1"><VideoStream ObjectRef="8"/><ImporterPrefs Encoding="base64" BinaryHash="8faeedf7-eb02-d2a5-c178-492000000014">/wAAAAEAAAA=</ImporterPrefs><FilePath>1129270354</FilePath><Infinite>true</Infinite><ImplementationID>42008e7a-de6f-4270-96de-7e287abb9b4b</ImplementationID><Title>Red matte</Title><ActualMediaFilePath>1129270354</ActualMediaFilePath></Media>"#,
        )
        .replace(
            "<Duration>2540160000000</Duration>",
            "<IsStill>true</IsStill><Duration>10973491200000000</Duration>",
        )
        .replace(
            "<OriginalDuration>2540160000000</OriginalDuration>",
            "<OriginalDuration>10973491200000000</OriginalDuration>",
        )
        .replacen(
            "<InPoint>0</InPoint><OutPoint>1270080000000</OutPoint>",
            "<InPoint>914457600000000</InPoint><OutPoint>915727680000000</OutPoint>",
            1,
        );
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let items: Vec<_> = sequence.video_items().collect();
    assert_eq!(items.len(), 2);
    let matte = items[0].media().unwrap();
    assert_eq!(matte.timeline_ticks(), 0..5 * TICKS);
    assert_eq!(
        project.media(matte).unwrap().video.as_ref().unwrap().kind,
        crate::schema::PrMediaKind::ColorMatte(crate::schema::PrColorMatte { rgb: [255, 0, 0] })
    );
    let graphic = items[1].graphic().unwrap();
    assert_eq!(graphic.timeline_ticks(), TICKS..3 * TICKS);
    assert_eq!(graphic.text().document.text, "Before");
}

#[test]
fn enabled_text_shadow_is_read_and_background_is_a_reported_feature() {
    let xml = graphic_xml(CAPTION_STYLE_EFFECTS);
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert_eq!(project.single_sequence().unwrap().video_items().count(), 2);
    // Whether a scaled or rotated text keeps it is a mapping decision.
    let shadow = graphic(&xml).text().document.shadow.unwrap();
    assert_eq!(
        (
            shadow.opacity,
            shadow.angle,
            shadow.distance,
            shadow.size,
            shadow.blur
        ),
        (100.0, 135.0, 3.0, 6.0, 12.0)
    );
    let reasons: Vec<_> = omissions
        .iter()
        .map(|omission| {
            (
                omission.scope,
                omission.record.as_str(),
                omission.reason.as_str(),
            )
        })
        .collect();
    assert_eq!(
        reasons,
        [(
            OmissionScope::Feature,
            "VideoClipTrackItem:20",
            "text background not converted"
        )]
    );
}

#[test]
fn shadow_values_outside_premiere_ranges_keep_the_graphic() {
    // Patch the pinned preview's opacity (slot 12), its only stored 100.0, to 150.
    let mut payload = STANDARD.decode(CAPTION_STYLE_EFFECTS).unwrap();
    let stored: Vec<usize> = payload
        .windows(4)
        .enumerate()
        .filter(|(_, bytes)| *bytes == 100.0_f32.to_le_bytes())
        .map(|(at, _)| at)
        .collect();
    let [at] = stored[..] else {
        panic!("expected one stored 100.0: {stored:?}");
    };
    payload[at..at + 4].copy_from_slice(&150.0_f32.to_le_bytes());
    let xml = graphic_xml(&STANDARD.encode(&payload));
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    // The reader keeps the text; converting it omits only the shadow.
    assert_eq!(project.single_sequence().unwrap().video_items().count(), 2);
    let reasons: Vec<_> = omissions
        .iter()
        .map(|omission| (omission.scope, omission.reason.as_str()))
        .collect();
    assert_eq!(
        reasons,
        [(OmissionScope::Feature, "text background not converted")]
    );
    assert_eq!(graphic(&xml).text().document.shadow.unwrap().opacity, 150.0);
}

#[test]
fn graphic_objects_read_in_chain_order_and_one_object_composes_a_static_vector_motion() {
    let kinds = |graphic: &crate::schema::PrGraphic| -> Vec<&str> {
        graphic
            .objects
            .iter()
            .map(|object| match object {
                PrGraphicObject::Text(_) | PrGraphicObject::TextLines(_) => "text",
                PrGraphicObject::Shape(_) => "shape",
                PrGraphicObject::Group(_) => "group",
            })
            .collect()
    };
    let chain = |references: &[u32]| -> String {
        references
            .iter()
            .enumerate()
            .map(|(index, reference)| {
                format!(r#"<Component Index="{index}" ObjectRef="{reference}"/>"#)
            })
            .collect()
    };
    // Several objects keep a static Vector Motion (Scale 50) for the group.
    for (references, expected, motion) in [
        (&[40, 80][..], &["text", "shape"][..], None),
        (&[80, 40], &["shape", "text"], None),
        (&[30, 80, 40], &["shape", "text"], Some(50.0)),
        (&[30, 80], &["shape"], None),
    ] {
        let graphic = graphic(&shape_xml(&chain(references), FILL));
        assert_eq!(kinds(&graphic), expected, "{references:?}");
        let scale = graphic.vector_motion.as_ref().map(|motion| motion.scale);
        assert_eq!(scale, motion, "{references:?}");
        let shape = graphic
            .objects
            .iter()
            .find_map(|object| match object {
                PrGraphicObject::Shape(shape) => Some(shape),
                PrGraphicObject::Text(_)
                | PrGraphicObject::TextLines(_)
                | PrGraphicObject::Group(_) => None,
            })
            .unwrap();
        assert_eq!(shape.name, "Box");
        assert_eq!(shape.path.vertices.len(), 4);
        assert_eq!(
            shape.appearance,
            PrAppearance {
                mask_source: None,
                fill: Some(PrFill::Solid(PrRgb([0, 96, 255]))),
                stroke: None,
                shadow: None,
            }
        );
        // One Shape composes the Vector Motion (Position 0.25:0.25, Anchor
        // 0.5:0.5, Scale 50, Rotation 90) as one text does.
        let transform = shape.transform;
        let composed = references == [30, 80];
        assert_eq!(
            (transform.position, transform.scale, transform.rotation),
            if composed {
                ([480.0, 270.0], 50.0, 90.0)
            } else {
                ([960.0, 540.0], 100.0, 0.0)
            },
            "{references:?}"
        );
    }
}

#[test]
fn a_gradient_shape_reads_under_any_transform_and_with_its_shadow() {
    use crate::format::shape_payload::{decode_appearance, encode_appearance};
    use crate::schema::{
        text::{PrGradient, PrGradientOpacityStop, PrShapeStroke, SHAPE_SHADOW_ANGLE},
        text_shadow::PrTextShadow,
    };
    // The gradient fixture's linear B, with a centred stroke or a shadow in
    // the form that calibration run 1 measured.
    let gradient = decode_appearance(&STANDARD.decode(GRADIENT_B).unwrap()).unwrap();
    let stroked = PrAppearance {
        stroke: Some(PrShapeStroke {
            color: PrRgb([0; 3]),
            width: 12.0,
        }),
        ..gradient.clone()
    };
    let shadowed = PrAppearance {
        shadow: Some(PrTextShadow {
            color: PrRgb([40; 3]),
            opacity: 100.0,
            angle: SHAPE_SHADOW_ANGLE,
            distance: 50.0,
            size: 20.0,
            blur: 0.0,
        }),
        ..gradient.clone()
    };
    let payload =
        |appearance: &PrAppearance| STANDARD.encode(encode_appearance(appearance).unwrap());
    // `xml` with the static value of parameter record `object` set to `value`.
    let set = |xml: String, object: u32, value: &str| {
        let record = xml.find(&format!(" ObjectID=\"{object}\"")).unwrap();
        let start = "<StartKeyframe>-91445760000000000,";
        let from = record + xml[record..].find(start).unwrap() + start.len();
        let to = from + xml[from..].find(',').unwrap();
        format!("{}{value}{}", &xml[..from], &xml[to..])
    };
    let shape = r#"<Component Index="0" ObjectRef="80"/>"#;
    let moved_shape =
        r#"<Component Index="0" ObjectRef="30"/><Component Index="1" ObjectRef="80"/>"#;
    let moved_pair = r#"<Component Index="0" ObjectRef="30"/><Component Index="1" ObjectRef="80"/><Component Index="2" ObjectRef="40"/>"#;
    // Vector Motion 30 without its Scale 50 and Rotation 90 only moves.
    let translating = |xml: String| set(set(xml, 32, "100."), 35, "0.");
    // Import converts each with its approximation warnings (convert::graphic).
    for (xml, appearance) in [
        (set(shape_xml(shape, GRADIENT_B), 85, "50."), &gradient),
        (
            // Horizontal Scale with Uniform Scale off.
            set(set(shape_xml(shape, GRADIENT_B), 87, "false"), 86, "150."),
            &gradient,
        ),
        (set(shape_xml(shape, GRADIENT_B), 88, "30."), &gradient),
        // A static Vector Motion composed into its one shape.
        (shape_xml(moved_shape, GRADIENT_B), &gradient),
        // A kept Vector Motion that scales, rotates or has keys.
        (set(shape_xml(moved_pair, GRADIENT_B), 35, "0."), &gradient),
        (
            set(shape_xml(moved_pair, GRADIENT_B), 32, "100."),
            &gradient,
        ),
        (
            keyed(
                &translating(shape_xml(moved_shape, GRADIENT_B)),
                31,
                TWO_POINT_KEYS,
            ),
            &gradient,
        ),
        (shape_xml(shape, &payload(&shadowed)), &shadowed),
        // Position, Anchor Point and a Vector Motion that only moves.
        (
            set(
                set(
                    translating(shape_xml(moved_pair, &payload(&stroked))),
                    84,
                    "0.25:0.75",
                ),
                90,
                "0.01:0.02",
            ),
            &stroked,
        ),
    ] {
        let (_, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let PrGraphicObject::Shape(shape) = &graphic(&xml).objects[0] else {
            panic!("the shape is listed first");
        };
        assert_eq!(&shape.appearance, appearance);
    }
    // An opacity outside 0..=1, as a percent would be, has no measured
    // meaning and omits the occurrence.
    let Some(PrFill::Gradient(ramp)) = &gradient.fill else {
        panic!("B is a gradient");
    };
    let percent = PrAppearance {
        fill: Some(PrFill::Gradient(PrGradient {
            opacity_stops: vec![PrGradientOpacityStop {
                position: 0.0,
                opacity: 50.0,
            }],
            ..ramp.clone()
        })),
        ..gradient.clone()
    };
    let (_, omissions) =
        inspect_project_with_omissions(&shape_xml(shape, &payload(&percent)), None).unwrap();
    let omitted: Vec<_> = omissions
        .iter()
        .map(|omission| (omission.scope, omission.reason.as_str()))
        .collect();
    assert_eq!(
        omitted,
        [(
            OmissionScope::Occurrence,
            "invalid Premiere project: a gradient needs one or more opacity stops within 0..=1, in order within 0..=1"
        )]
    );
}

#[test]
fn legacy_json_appearance_shape_reads_and_an_enabled_shadow_omits_only_its_graphic() {
    use crate::tests::support::{legacy_appearance, legacy_json};
    // The native Shape with our own legacy Appearance in place of its own.
    let xml = |changes: &[(&str, Option<&str>)]| {
        shape_xml(
            r#"<Component Index="0" ObjectRef="80"/>"#,
            &STANDARD.encode(legacy_appearance(&legacy_json(changes))),
        )
    };
    let gray = xml(&[]);
    let (_, omissions) = inspect_project_with_omissions(&gray, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let read = graphic(&gray);
    let [PrGraphicObject::Shape(shape)] = read.objects.as_slice() else {
        panic!("one shape, not {:?}", read.objects);
    };
    assert_eq!(
        shape.appearance,
        PrAppearance {
            mask_source: None,
            fill: Some(PrFill::Solid(PrRgb([128; 3]))),
            stroke: None,
            shadow: None,
        }
    );
    assert_eq!(
        (
            shape.name.as_str(),
            shape.path.vertices.len(),
            shape.transform.position
        ),
        ("Box", 4, [960.0, 540.0])
    );
    // An enabled legacy shadow omits only its graphic; the video stays.
    let (project, omissions) =
        inspect_project_with_omissions(&xml(&[("mShadowVisible", Some("true"))]), None).unwrap();
    let sequence = project.single_sequence().unwrap();
    assert_eq!(
        (
            sequence.video_items().count(),
            sequence.video_occurrences().count()
        ),
        (1, 1)
    );
    let omitted: Vec<_> = omissions
        .iter()
        .map(|omission| {
            (
                omission.scope,
                omission.record.as_str(),
                omission.reason.as_str(),
            )
        })
        .collect();
    assert_eq!(
        omitted,
        [(
            OmissionScope::Occurrence,
            "20",
            "unsupported conversion: ArbVideoComponentParam:82: legacy JSON Appearance: an enabled shadow is unsupported"
        )]
    );
}

#[test]
fn static_graphic_source_span_is_not_its_placement_duration() {
    let original = graphic_xml(BEFORE);
    let expected = graphic(&original);
    for out in [914415264000000_i64, 914923296000000] {
        let xml = original.replace(
            "<OutPoint>914669280000000</OutPoint>",
            &format!("<OutPoint>{out}</OutPoint>"),
        );
        let actual = graphic(&xml);
        assert_eq!(actual.timeline_ticks(), expected.timeline_ticks());
        assert_eq!(actual.in_ticks, expected.in_ticks);
        assert_eq!(actual.objects, expected.objects);
        assert_eq!(actual.opacity, expected.opacity);
        assert_eq!(actual.vector_motion, expected.vector_motion);
    }
}

#[test]
fn unequal_graphic_spans_reject_each_animation_clock() {
    let xml = graphic_xml(BEFORE);
    let source_text = xml.replace(
        "</StartKeyframeValue>",
        &format!("</StartKeyframeValue><Keyframes>0,{BEFORE};</Keyframes>"),
    );
    let opacity = with_clip_opacity(
        r#"<Component Index="0" ObjectRef="70"/><Component Index="1" ObjectRef="40"/>"#,
        "100.",
        &case_b_opacity_keys(0),
        (18, 0),
    );
    for keyed in [
        keyed(&xml, 32, TWO_SCALAR_KEYS),
        keyed(&xml, 44, TWO_SCALAR_KEYS),
        source_text,
        opacity,
    ] {
        // The controls are accepted at a one-to-one clock before mutation.
        graphic(&keyed);
        let changed = keyed.replace(
            "<OutPoint>914669280000000</OutPoint>",
            "<OutPoint>914923296000000</OutPoint>",
        );
        let (project, omissions) = inspect_project_with_omissions(&changed, None).unwrap();
        assert_eq!(project.single_sequence().unwrap().video_items().count(), 1);
        assert!(
            omissions.iter().any(|omission| {
                omission.scope == OmissionScope::Occurrence
                    && omission.reason.contains("graphic retiming is unsupported")
            }),
            "{omissions:?}"
        );
    }
}

#[test]
fn unsupported_graphics_are_omitted_without_losing_other_occurrences() {
    let legacy = "AgAAAAAAAAB7AH0A";
    // Leading below -0.4 em spaces lines closer than the FX renderer can render.
    let mut tight = decode(&STANDARD.decode(BEFORE).unwrap()).unwrap().document;
    tight.leading = -0.5 * tight.size;
    let tight_leading = STANDARD.encode(encode(&tight).unwrap());
    let mut control = decode(&STANDARD.decode(BEFORE).unwrap()).unwrap().document;
    control.text = "bad\u{3}break".into();
    let control_text = STANDARD.encode(encode(&control).unwrap());
    // FX font keys join family and style with '/'.
    let mut slash = decode(&STANDARD.decode(BEFORE).unwrap()).unwrap().document;
    slash.font = "OpenSans/Bold".into();
    let slash_font = STANDARD.encode(encode(&slash).unwrap());
    let cases = [
        (graphic_xml(legacy), "legacy UTF-16 JSON"),
        (
            keyed(&graphic_xml(BEFORE), 49, TWO_POINT_KEYS),
            "animated graphic \"Anchor Point\" is unsupported",
        ),
        // Keys where the model has no counterpart: Text Horizontal Scale and
        // Parent Width, Vector Motion Scale Width and Anchor Point.
        (
            keyed(&graphic_xml(BEFORE), 45, TWO_SCALAR_KEYS),
            "animated graphic \"Horizontal Scale\" is unsupported",
        ),
        (
            keyed(&graphic_xml(BEFORE), 59, TWO_SCALAR_KEYS),
            "animated graphic \"Parent Width\" is unsupported",
        ),
        (
            keyed(&graphic_xml(BEFORE), 33, TWO_SCALAR_KEYS),
            "animated graphic \"Scale Width\" is unsupported",
        ),
        (
            keyed(&graphic_xml(BEFORE), 36, TWO_POINT_KEYS),
            "animated graphic \"Anchor Point\" is unsupported",
        ),
        (
            // Uniform scale off.
            graphic_xml(BEFORE).replace(
                "<Name> </Name><ParameterID>4</ParameterID><StartKeyframe>-91445760000000000,true,",
                "<Name> </Name><ParameterID>4</ParameterID><StartKeyframe>-91445760000000000,false,",
            ),
            "nondefault graphic parameter \" \" is unsupported",
        ),
        (
            // A Source Text key is decoded like the static value.
            graphic_xml(BEFORE).replacen(
                "</StartKeyframeValue></ArbVideoComponentParam>",
                "</StartKeyframeValue><Keyframes>0,AA==;</Keyframes></ArbVideoComponentParam>",
                1,
            ),
            "ArbVideoComponentParam:41: truncated Source Text payload",
        ),
        (
            // Pre-26 text is omitted for its encoding, keys or not.
            keyed(&graphic_xml(legacy), 48, TWO_SCALAR_KEYS),
            "legacy UTF-16 JSON",
        ),
        (
            // Shape Path keys omit the graphic.
            graphic_xml(BEFORE)
                .replace(
                    "<Intrinsic>true</Intrinsic><DisplayName>Vector Motion</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Graphic Group</MatchName>",
                    "<DisplayName>Shape</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Shape</MatchName>",
                )
                .replace(
                    r#"<PointComponentParam ObjectID="31" ClassID="ca81d347-309b-44d2-acc7-1c572efb973c" Version="4"><Name>Position</Name><ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,0.25:0.25,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe></PointComponentParam>"#,
                    r#"<ArbVideoComponentParam ObjectID="31" ClassID="313e54d4-6903-49ad-b0bf-8262cdd10f4e" Version="3"><Name>Path</Name><ParameterControlType>22</ParameterControlType><ParameterID>1</ParameterID><StartKeyframePosition>-91445760000000000</StartKeyframePosition><Keyframes>0,AA==;</Keyframes></ArbVideoComponentParam>"#,
                ),
            "animated or unknown Path is unsupported",
        ),
        (
            graphic_xml(BEFORE).replace(
                "<Name>Parent Width</Name><ParameterID>19</ParameterID><StartKeyframe>-91445760000000000,0.,",
                "<Name>Parent Width</Name><ParameterID>19</ParameterID><StartKeyframe>-91445760000000000,20001.,",
            ),
            "graphic controller value outside native bounds",
        ),
        (
            // A Vector Motion comes first.
            graphic_xml(BEFORE).replace(
                TWO_COMPONENTS,
                r#"<Component Index="0" ObjectRef="40"/><Component Index="1" ObjectRef="30"/>"#,
            ),
            "VideoFilterComponent:30: unsupported graphic component",
        ),
        (
            graphic_xml(BEFORE).replace(
                "<ComponentOwner><Components ObjectRef=\"21\"/></ComponentOwner>",
                "<ComponentOwner><Components ObjectRef=\"4\"/></ComponentOwner>",
            ),
            "a graphic without text or shape objects is unsupported",
        ),
        (
            graphic_xml(BEFORE).replace(
                "<DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><DefaultMotionComponentID>",
                "<DefaultMotion>false</DefaultMotion><DefaultOpacity>true</DefaultOpacity><DefaultMotionComponentID>",
            ),
            "Motion and Opacity must keep their defaults",
        ),
        (
            graphic_xml(BEFORE).replace(
                "<OutPoint>914669280000000</OutPoint>",
                "<OutPoint>914161248000000</OutPoint>",
            ),
            "graphic retiming",
        ),
        (
            graphic_xml(BEFORE).replace(
                "<ClipID>placed-graphic</ClipID>",
                "<PlayBackwards>true</PlayBackwards><ClipID>placed-graphic</ClipID>",
            ),
            "graphic retiming",
        ),
        (
            graphic_xml(BEFORE).replace(
                "<ClipID>placed-graphic</ClipID>",
                "<PlaybackSpeed>2</PlaybackSpeed><ClipID>placed-graphic</ClipID>",
            ),
            "graphic retiming",
        ),
        (
            // Any existing record: the reader must not follow the remap.
            graphic_xml(BEFORE).replace(
                "<ClipID>placed-graphic</ClipID>",
                r#"<TimeRemapping ObjectRef="25"/><ClipID>placed-graphic</ClipID>"#,
            ),
            "graphic retiming",
        ),
        (
            graphic_xml(BEFORE).replace(
                "<OutPoint>914669280000000</OutPoint></Clip></VideoClip>",
                "<OutPoint>914669280000000</OutPoint></Clip><ScaleToFramePolicy>1</ScaleToFramePolicy></VideoClip>",
            ),
            "VideoClip:23: Scale to Frame Size on a graphic is not converted",
        ),
        (
            graphic_xml(BEFORE).replace(
                "<FrameRate>8467200000</FrameRate><FrameRect>0,0,1920,1080</FrameRect><AlphaType>",
                "<FrameRate>8467200000</FrameRate><FrameRect>0,0,1080,1080</FrameRect><AlphaType>",
            ),
            "frame size or rate",
        ),
        (
            // A second VideoClip on the master clip is ambiguous, as on a
            // media master clip.
            graphic_xml(BEFORE).replace(
                r#"<Clip Index="0" ObjectRef="26"/></Clips>"#,
                r#"<Clip Index="0" ObjectRef="26"/><Clip Index="1" ObjectRef="23"/></Clips>"#,
            ),
            "MasterClip:graphic-master: multiple source clips unsupported",
        ),
        (
            graphic_xml(BEFORE).replace(
                "<Name>Horizontal Scale</Name><ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,100.,",
                "<Name>Horizontal Scale</Name><ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,80.,",
            ),
            "Horizontal Scale under Uniform Scale is unverified",
        ),
        (
            graphic_xml(BEFORE).replace(
                "<Name>Scale Width</Name><ParameterID>3</ParameterID><UpperUIBound>200</UpperUIBound><StartKeyframe>-91445760000000000,100.,",
                "<Name>Scale Width</Name><ParameterID>3</ParameterID><UpperUIBound>200</UpperUIBound><StartKeyframe>-91445760000000000,80.,",
            ),
            "nondefault graphic parameter",
        ),
        (graphic_xml(&tight_leading), "line spacing below 0.8 em"),
        (graphic_xml(&control_text), "control characters"),
        (
            graphic_xml(&slash_font),
            "\"OpenSans/Bold\" contains '/', so it is not a PostScript name",
        ),
        (
            graphic_xml("").replace(
                "BinaryHash=\"5ed9ebfb-98e1-6486-992d-0b0600000160\">",
                "BinaryHash=\"no-such-binary\">",
            ),
            "names missing binary",
        ),
        // Graphic Shapes: a bypassed object, a SubGroup, an effect between
        // objects, keys on a Shape, an unverified
        // join, Horizontal Scale under Uniform Scale and a Path of another
        // version.
        (
            shape_xml(TEXT_THEN_SHAPE, FILL).replace(
                "<InstanceName>Box</InstanceName>",
                "<InstanceName>Box</InstanceName><Bypass>true</Bypass>",
            ),
            "VideoFilterComponent:80: a bypassed graphic object is unsupported",
        ),
        (
            shape_xml(TEXT_THEN_SHAPE, FILL).replace(
                "<MatchName>AE.ADBE Shape</MatchName>",
                "<MatchName>AE.ADBE Graphic SubGroup</MatchName>",
            ),
            "unsupported graphic SubGroup component",
        ),
        (
            shape_xml(
                r#"<Component Index="0" ObjectRef="40"/><Component Index="1" ObjectRef="80"/><Component Index="2" ObjectRef="40"/>"#,
                FILL,
            )
            .replace(
                "<MatchName>AE.ADBE Shape</MatchName>",
                "<MatchName>AE.ADBE Gaussian Blur 2</MatchName>",
            ),
            "effect \"AE.ADBE Gaussian Blur 2\" in a graphic is unsupported",
        ),
        (
            keyed(&shape_xml(TEXT_THEN_SHAPE, FILL), 84, TWO_POINT_KEYS),
            "keyed graphic Shape parameters are unsupported",
        ),
        (
            // An equilateral triangle's 60° corners lie between the corners
            // that calibration-2 saw Premiere miter and bevel.
            shape_xml(TEXT_THEN_SHAPE, CENTRED).replace(RECTANGLE, &equilateral_triangle()),
            "VideoFilterComponent:80: stroke joins are unverified",
        ),
        (
            shape_xml(TEXT_THEN_SHAPE, FILL).replace(
                "<Name>Horizontal Scale</Name><ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,100.,",
                "<Name>Horizontal Scale</Name><ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,150.,",
            ),
            "Horizontal Scale under Uniform Scale is unverified",
        ),
        (
            shape_xml(TEXT_THEN_SHAPE, FILL).replace(RECTANGLE, &format!("Aw{}", &RECTANGLE[2..])),
            "ArbVideoComponentParam:81: Path version 3 is unsupported",
        ),
        // Graphic masks: a `SubComponents` mask reference on a Text
        // object or on the Vector Motion omits the graphic before the
        // mask is read (`visualizer_slideshow` masks its ten Texts), so the
        // reference need only resolve. The earlier native shape model
        // rejected the element. The clip Opacity's mask is read as a media
        // clip's, and one outside the converted form omits the graphic
        // rather than show it unmasked.
        (
            graphic_xml(BEFORE).replace(
                "<VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Text</MatchName>",
                "<SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"23\"/></SubComponents><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Text</MatchName>",
            ),
            "VideoFilterComponent:40: a mask on a graphic object is not converted",
        ),
        (
            graphic_xml(BEFORE).replace(
                "<DisplayName>Vector Motion</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Graphic Group</MatchName>",
                "<DisplayName>Vector Motion</DisplayName></Component><SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"23\"/></SubComponents><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Graphic Group</MatchName>",
            ),
            "VideoFilterComponent:30: a mask on a graphic Vector Motion is not converted",
        ),
        (
            with_clip_opacity(OPACITY_AND_TEXT, "100.", "", (18, 0))
                .replace(
                    "<MatchName>AE.ADBE Opacity</MatchName>",
                    "<SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"300\"/></SubComponents><MatchName>AE.ADBE Opacity</MatchName>",
                )
                .replace(
                    "</PremiereData>",
                    &format!(
                        "{}</PremiereData>",
                        super::mask::mask(300, true).replace(
                            "<Name>Mask Expansion</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>8</ParameterControlType><StartKeyframe>-91445760000000000,0.,",
                            "<Name>Mask Expansion</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>8</ParameterControlType><StartKeyframe>-91445760000000000,1001.,",
                        )
                    ),
                ),
            "VideoComponentParam:309: Mask Expansion must be finite and within -1000..=1000",
        ),
    ];
    for (xml, reason) in cases {
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        let sequence = project.single_sequence().unwrap();
        assert_eq!(sequence.video_items().count(), 1, "{reason}");
        assert_eq!(sequence.video_occurrences().count(), 1, "{reason}");
        assert!(
            omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Occurrence
                    && omission.record == "20"
                    && omission.reason.contains(reason)),
            "{reason}: {omissions:?}"
        );
    }
}

#[test]
fn a_graphic_with_mask_path_keys_is_omitted_beside_its_sibling_without_a_guide() {
    // A graphic's clip Opacity converts a static mask only, so Mask Path keys
    // omit the graphic rather than freeze its outline, and the clip below it
    // converts with no guide left behind.
    let xml = with_clip_opacity(OPACITY_AND_TEXT, "100.", "", (18, 0))
        .replace(
            "<MatchName>AE.ADBE Opacity</MatchName>",
            "<SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"300\"/></SubComponents><MatchName>AE.ADBE Opacity</MatchName>",
        )
        .replace(
            "</PremiereData>",
            &format!(
                "{}</PremiereData>",
                super::mask::keyed_mask(300, &super::mask::two_path_keys())
            ),
        );
    let (project, mut omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.video_items().count(), 1);
    assert_eq!(sequence.video_occurrences().count(), 1);
    let occurrences: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.scope == OmissionScope::Occurrence)
        .collect();
    let [omission] = occurrences.as_slice() else {
        panic!("{omissions:?}");
    };
    assert_eq!(omission.record, "20");
    assert!(
        omission.reason.contains("VideoComponentChain:21: Mask Path keys on a graphic clip Opacity are not converted; only a video clip's Opacity mask converts keyed"),
        "{}",
        omission.reason
    );
    let document = crate::convert::premiere_to_tesseract(
        sequence,
        &project.media,
        &crate::tesseract_output::asset_ids_in_order(sequence, &project.media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    let types: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| layer["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["Video", "Rect"]);
}

#[test]
fn deduplicated_binary_values_resolve_by_hash() {
    // Premiere writes each distinct binary value once. Later copies are empty
    // elements that name it by BinaryHash, as in a Premiere 26.5.1 re-save.
    let shared = r#"<ArbVideoComponentParam ObjectID="99"><StartKeyframeValue Encoding="base64" BinaryHash="5ed9ebfb-98e1-6486-992d-0b0600000160">PAYLOAD</StartKeyframeValue></ArbVideoComponentParam>
</PremiereData>"#
        .replace("PAYLOAD", BEFORE);
    let xml = graphic_xml("")
        .replace(
            "<Title>Graphic</Title></Media>",
            r#"<Title>Graphic</Title><ModificationState Encoding="base64" BinaryHash="2aa7b0a6-32ba-a031-96f4-305d0000001c"/></Media>"#,
        )
        .replace("</PremiereData>", &shared);
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(project.single_sequence().unwrap().video_items().count(), 2);
    assert_eq!(graphic(&xml).text().document.text, "Before");
}

#[test]
fn a_binary_hash_defined_with_different_values_omits_its_graphic() {
    // The graphic's own Source Text is empty, so it resolves through this hash.
    let hash = "5ed9ebfb-98e1-6486-992d-0b0600000160";
    let stored = |object: u32, payload: &str| {
        format!(
            r#"<ArbVideoComponentParam ObjectID="{object}"><StartKeyframeValue Encoding="base64" BinaryHash="{hash}">{payload}</StartKeyframeValue></ArbVideoComponentParam>"#
        )
    };
    let with = |definitions: String| {
        graphic_xml("").replace(
            "</PremiereData>",
            &format!("{definitions}\n</PremiereData>"),
        )
    };
    // Premiere repeats a definition only with the same data; line breaks are not data.
    let wrapped = BEFORE
        .as_bytes()
        .chunks(76)
        .map(|line| std::str::from_utf8(line).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let xml = with(stored(98, BEFORE) + &stored(99, &wrapped));
    let (_, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(graphic(&xml).text().document.text, "Before");

    // Different data under one hash is ambiguous: the graphic must not take the first.
    let mut edited = decode(&STANDARD.decode(BEFORE).unwrap()).unwrap().document;
    edited.text = "After".into();
    let other = STANDARD.encode(encode(&edited).unwrap());
    let xml = with(stored(98, BEFORE) + &stored(99, &other));
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert_eq!(project.single_sequence().unwrap().video_items().count(), 1);
    assert!(
        omissions
            .iter()
            .any(|omission| omission.scope == OmissionScope::Occurrence
                && omission.record == "20"
                && omission.reason.contains(&format!(
                    "BinaryHash {hash} is defined with different values"
                ))),
        "{omissions:?}"
    );
}

#[test]
fn native_graphic_rejects_a_nonzero_subclip_time_offset() {
    // Synthetic offset on the pinned text case, not Adobe graphic-subclip proof.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_text_point.prproj");
    let xml = crate::format::reader::read_xml(&path).unwrap();
    assert_eq!(xml.matches("</ClipTrackItem>").count(), 1);
    let edited = xml.replace(
        "</ClipTrackItem>",
        "<OriginalSubClipTimeOffset>1</OriginalSubClipTimeOffset></ClipTrackItem>",
    );
    let error = inspect_project_with_omissions(&edited, None)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("nonzero OriginalSubClipTimeOffset"),
        "{error}"
    );
}

#[test]
fn native_graphic_rejects_unknown_animation_flags() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_text_point.prproj");
    let xml = crate::format::reader::read_xml(&path).unwrap();
    assert!(inspect_project_with_omissions(&xml, None)
        .unwrap()
        .1
        .is_empty());
    for name in ["Source Text", "Position"] {
        let marker = format!("<Name>{name}</Name>");
        assert!(xml.contains(&marker));
        for flag in ["true", "unknown"] {
            let edited = xml.replace(
                &marker,
                &format!("{marker}<IsTimeVarying>{flag}</IsTimeVarying>"),
            );
            let error = inspect_project_with_omissions(&edited, None)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("animated or unknown"),
                "{name}/{flag}: {error}"
            );
        }
    }
}

#[test]
fn disabled_graphic_and_output_off_track_import_as_hidden_text() {
    let visible = graphic_xml(BEFORE);
    let clip_off = visible.replace(
        r#"<SubClip ObjectRef="22"/></ClipTrackItem>"#,
        r#"<SubClip ObjectRef="22"/><IsMuted>true</IsMuted></ClipTrackItem>"#,
    );
    let track_off = visible.replace(
        "<Track><ID>2</ID><Index>1</Index></Track>",
        "<Track><ID>2</ID><Index>1</Index><IsMuted>true</IsMuted></Track>",
    );
    assert!(clip_off != visible && track_off != visible);
    for (xml, hidden) in [(&visible, false), (&clip_off, true), (&track_off, true)] {
        let (project, omissions) = inspect_project_with_omissions(xml, None).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let sequence = project.single_sequence().unwrap();
        let document = crate::tests::support::project_document_with_media(sequence, &project.media);
        let layers = document["composition"]["layers"].as_array().unwrap();
        let hidden_by_type: Vec<_> = layers
            .iter()
            .map(|layer| (layer["type"].as_str().unwrap(), layer["isHidden"] == true))
            .collect();
        assert_eq!(
            hidden_by_type,
            [("Text", hidden), ("Video", false), ("Rect", false)]
        );
    }
}

/// `document` as base64 Source Text.
fn payload(document: &crate::schema::text::PrTextDocument) -> String {
    STANDARD.encode(encode(document).unwrap())
}

/// `BEFORE`'s document with `edit` applied.
fn before_with(
    edit: impl FnOnce(&mut crate::schema::text::PrTextDocument),
) -> crate::schema::text::PrTextDocument {
    let mut document = decode(&STANDARD.decode(BEFORE).unwrap()).unwrap().document;
    edit(&mut document);
    document
}

/// `text_only_xml` whose Source Text has `keys` (`ticks,base64;` each) in
/// the corpus record order, with `IsTimeVarying` after its name when `flag`
/// is given (Premiere 14 wrote keys without the flag).
fn with_source_text_keys(flag: Option<&str>, keys: &str) -> String {
    let flag = flag.map_or_else(String::new, |flag| {
        format!("<IsTimeVarying>{flag}</IsTimeVarying>")
    });
    text_only_xml()
        .replacen(
            "<Name>Source Text</Name>",
            &format!("<Name>Source Text</Name>{flag}"),
            1,
        )
        .replacen(
            r#"<StartKeyframeValue Encoding="base64" BinaryHash="5ed9ebfb-98e1-6486-992d-0b0600000160">"#,
            &format!(
                r#"<Keyframes>{keys}</Keyframes><StartKeyframeValue Encoding="base64" BinaryHash="5ed9ebfb-98e1-6486-992d-0b0600000160">"#
            ),
            1,
        )
}

#[test]
fn source_text_keys_read_as_held_documents_on_the_generator_clock() {
    let two = before_with(|document| document.text = "Two".into());
    let three = before_with(|document| {
        document.text = "Three".into();
        document.size = 140.0;
    });
    // Keys before the In and after the Out are kept, like transform keys.
    let keys = format!(
        "{},{};{},{};",
        IN - TICKS / 2,
        payload(&two),
        IN + 5 * TICKS / 2,
        payload(&three)
    );
    for flag in [None, Some("true")] {
        let xml = with_source_text_keys(flag, &keys);
        let (_, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        assert!(omissions.is_empty(), "{flag:?}: {omissions:?}");
        let text = graphic(&xml).text().clone();
        assert_eq!(
            text.source_text_keys
                .iter()
                .map(|key| (
                    key.source_ticks,
                    key.document.text.as_str(),
                    key.document.size
                ))
                .collect::<Vec<_>>(),
            [
                (IN - TICKS / 2, "Two", two.size),
                (IN + 5 * TICKS / 2, "Three", 140.0)
            ],
            "{flag:?}"
        );
        // The stored static value ("Before") is not the text shown before
        // the first key; the first key's document is.
        assert_eq!(text.document, two, "{flag:?}");
        assert!(text.animations.is_empty(), "{flag:?}");
    }
    // A key document's omitted feature is reported once.
    let xml = with_source_text_keys(
        Some("true"),
        &format!(
            "{IN},{CAPTION_STYLE_EFFECTS};{},{CAPTION_STYLE_EFFECTS};",
            IN + TICKS
        ),
    );
    let (_, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    let reasons: Vec<_> = omissions
        .iter()
        .map(|omission| (omission.scope, omission.reason.as_str()))
        .collect();
    assert_eq!(
        reasons,
        [(OmissionScope::Feature, "text background not converted")]
    );
}

#[test]
fn source_text_keys_beyond_the_former_count_are_preserved() {
    let two = payload(&before_with(|document| document.text = "Two".into()));
    let many: String = (0..4097)
        .map(|index| format!("{},{two};", IN + index as i64 * TICKS / 100))
        .collect();
    let xml = with_source_text_keys(None, &many);
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let graphic = project
        .single_sequence()
        .unwrap()
        .video_items()
        .find_map(crate::schema::PrVideoItem::graphic)
        .expect("graphic survives");
    assert_eq!(graphic.text().source_text_keys.len(), 4097);
    assert_eq!(graphic.text().source_text_keys[4096].document.text, "Two");
}

#[test]
fn source_text_key_forms_that_do_not_convert_omit_the_graphic() {
    let two = payload(&before_with(|document| document.text = "Two".into()));
    let other_font = payload(&before_with(|document| {
        document.font = "Arial-BoldMT".into()
    }));
    let stroked = |color, width| {
        payload(&before_with(|document| {
            document.stroke = Some(crate::schema::text::PrTextStroke { color, width })
        }))
    };
    let stroke = |color| stroked(color, 3.0);
    let unstroked = payload(&before_with(|document| document.stroke = None));
    let stroke_change = "Source Text keys change the stroke color, which no FX text track animates";
    let corpus_legacy = "AgAAAAAAAAB7AH0A";
    let cases = [
        (
            // The Premiere 14 corpus form keys legacy UTF-16 JSON documents.
            with_source_text_keys(None, &format!("{IN},{corpus_legacy};")),
            "legacy UTF-16 JSON text from Premiere before 26 converts only as a static graphic Source Text value",
        ),
        (
            with_source_text_keys(Some("true"), ""),
            "animated or unknown Source Text is unsupported",
        ),
        (
            with_source_text_keys(Some("false"), &format!("{IN},{two};")),
            "animated or unknown Source Text is unsupported",
        ),
        (
            with_source_text_keys(None, &format!("{IN},{two}")),
            "unterminated Source Text key list",
        ),
        (
            with_source_text_keys(None, &format!("{IN};")),
            "unexpected Source Text key shape",
        ),
        (
            with_source_text_keys(None, &format!("{IN},{two},0;")),
            "invalid Source Text base64",
        ),
        (
            with_source_text_keys(None, &format!("{IN},{two};{IN},{two};")),
            "Source Text keys must have strictly increasing source times",
        ),
        (
            with_source_text_keys(None, &format!("{IN},{two};{},{other_font};", IN + TICKS)),
            "Source Text keys change the font, which no FX text track animates",
        ),
        (
            with_source_text_keys(
                None,
                &format!(
                    "{IN},{};{},{};",
                    stroke(PrRgb([0, 0, 0])),
                    IN + TICKS,
                    stroke(PrRgb([0, 0, 255]))
                ),
            ),
            stroke_change,
        ),
        // Enabled stroke colors must agree even after an unstroked key.
        (
            with_source_text_keys(
                None,
                &format!(
                    "{IN},{unstroked};{},{};{},{};",
                    IN + TICKS,
                    stroke(PrRgb([0, 0, 0])),
                    IN + 2 * TICKS,
                    stroke(PrRgb([0, 0, 255]))
                ),
            ),
            stroke_change,
        ),
    ];
    for (xml, reason) in cases {
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        assert_eq!(
            project.single_sequence().unwrap().video_items().count(),
            1,
            "{reason}"
        );
        assert!(
            omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Occurrence
                    && omission.record == "20"
                    && omission.reason.contains(reason)),
            "{reason}: {omissions:?}"
        );
    }
}

/// Our own legacy Source Text, independently chosen, as a Premiere 26
/// document would hold it once read: centred point text in two paragraphs.
fn legacy_document() -> crate::schema::text::PrTextDocument {
    crate::schema::text::PrTextDocument {
        text: "Night\nMarket \u{2713} \u{1f525}".into(),
        font: "Inter-SemiBold".into(),
        size: 64.5,
        fill: Some(PrRgb([128, 128, 128])),
        stroke: None,
        shadow: None,
        all_caps: false,
        tracking: 25.0,
        leading: 0.0,
        justification: PrJustification::Center,
        frame: PrTextFrame::Point {
            vertical: PrVerticalAlign::Top,
        },
        background: None,
    }
}

/// Independently authored static legacy Text: the complete IDs 1–21 layout,
/// with no later ID 22 switch, and equal Scale/Horizontal Scale under Uniform.
fn legacy_static_text_xml() -> String {
    use crate::tests::support::{legacy_source_text, legacy_source_text_payload};
    legacy_static_text_layout(
        &graphic_xml(&STANDARD.encode(legacy_source_text_payload(
            &legacy_source_text().to_string(),
        )))
        .replace(TWO_COMPONENTS, r#"<Component Index="0" ObjectRef="40"/>"#),
    )
}

fn legacy_static_text_layout(xml: &str) -> String {
    xml.replace(r#"<Param Index="21" ObjectRef="62"/>"#, "")
        .replace(
            "<ParameterID>4</ParameterID><StartKeyframe>-91445760000000000,100.,",
            "<ParameterID>4</ParameterID><StartKeyframe>-91445760000000000,37.5,",
        )
        .replace(
            "<ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,100.,",
            "<ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,37.5,",
        )
}

fn with_graphic_ramp(xml: &str, attached: &str) -> String {
    let parsed = roxmltree::Document::parse(xml).unwrap();
    let components = parsed
        .root_element()
        .children()
        .find(|node| node.attribute("ObjectID") == Some("21"))
        .unwrap()
        .descendants()
        .find(|node| node.has_tag_name("Components"))
        .unwrap();
    let mut references = Vec::new();
    for component in components.children().filter(|node| node.is_element()) {
        let reference = component.attribute("ObjectRef").unwrap();
        if reference == "40" {
            references.push("100");
        }
        references.push(reference);
    }
    let references: String = references
        .iter()
        .enumerate()
        .map(|(index, reference)| {
            format!(r#"<Component Index="{index}" ObjectRef="{reference}"/>"#)
        })
        .collect();
    let mut xml = xml.to_owned();
    xml.replace_range(
        components.range(),
        &format!(r#"<Components Version="1">{references}</Components>"#),
    );
    xml.replace(
        "</PremiereData>",
        &format!(r#"<VideoFilterComponent ObjectID="100" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="8"><Component Version="5"><ID>10</ID><DisplayName>Ramp</DisplayName><Bypass>false</Bypass><Intrinsic>false</Intrinsic></Component>{attached}<MatchName>AE.ADBE Ramp</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent></PremiereData>"#),
    )
}

#[test]
fn active_graphic_ramp_omits_only_the_effect_and_keeps_editable_text() {
    let xml = keyed(&text_only_xml(), 44, TWO_SCALAR_KEYS);
    let input = with_graphic_ramp(&xml, "");
    let (project, omissions) = inspect_project_with_omissions(&input, None).unwrap();
    let actual = graphic(&input);
    let expected = graphic(&xml);
    assert_eq!(actual.text(), expected.text());
    assert_eq!(
        (
            actual.start_ticks,
            actual.end_ticks,
            actual.in_ticks,
            actual.opacity
        ),
        (
            expected.start_ticks,
            expected.end_ticks,
            expected.in_ticks,
            expected.opacity
        )
    );
    assert_eq!(project.single_sequence().unwrap().video_items().count(), 2);
    let losses: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.reason.contains("VideoFilterComponent:100"))
        .collect();
    assert_eq!(losses.len(), 1, "{omissions:?}");
    assert_eq!(losses[0].scope, OmissionScope::Feature);
    assert!(losses[0].reason.contains("Ramp"));
    assert!(losses[0].reason.contains("saved base paints"));
    assert!(graphic(&input)
        .text()
        .animations
        .iter()
        .any(|animation| animation.property() == crate::schema::PrAnimatedProperty::UniformScale));
}

#[test]
fn graphic_ramp_keeps_shape_values_and_the_vector_motion_order_guard() {
    let xml = shape_xml(TEXT_THEN_SHAPE, FILL);
    let actual = graphic(&with_graphic_ramp(&xml, ""));
    assert_eq!(actual.objects, graphic(&xml).objects);
    assert!(actual.effect_loss.is_some());
    // A Ramp outside Vector Motion's admitted position must not reorder that
    // transform merely to recover the objects.
    let ordered = with_graphic_ramp(&graphic_xml(BEFORE), "");
    assert_eq!(
        graphic(&ordered).text(),
        graphic(&graphic_xml(BEFORE)).text()
    );
    let reordered = ordered.replace(
        r#"<Component Index="0" ObjectRef="30"/><Component Index="1" ObjectRef="100"/>"#,
        r#"<Component Index="0" ObjectRef="100"/><Component Index="1" ObjectRef="30"/>"#,
    );
    assert_ne!(ordered, reordered);
    let (project, omissions) = inspect_project_with_omissions(&reordered, None).unwrap();
    assert_eq!(project.single_sequence().unwrap().video_items().count(), 1);
    assert!(
        omissions
            .iter()
            .any(|omission| omission.scope == OmissionScope::Occurrence
                && omission.reason.contains("unsupported graphic component")),
        "{omissions:?}"
    );
}

#[test]
fn graphic_ramp_loss_rejects_only_dependent_track_matte_consumers() {
    use super::{
        animation::animation_fixture::track_matte_key_xml,
        effects::{with_chain, with_second_clip, DEFAULT_FLAGS},
    };
    let source = with_second_clip(&with_graphic_ramp(&text_only_xml(), "").replace(
        "<Start>254016000000</Start><End>762048000000</End>",
        "<Start>0</Start><End>1270080000000</End>",
    ))
    .replace(
        r#"<TrackItem ObjectRef="20"/>"#,
        r#"<TrackItem ObjectRef="20"/><TrackItem ObjectRef="220"/>"#,
    )
    .replace(
        "</PremiereData>",
        &format!("{}</PremiereData>", graphic_item_record(220, 5 * TICKS)),
    );
    // Active, missing and invalid bypass flags are never evidence of unchanged
    // alpha. Only an explicitly bypassed effect can remain an alpha/luma source.
    for flag in ["false", "true", "missing", "invalid"] {
        for (channel, inverted) in [(0, false), (0, true), (1, false)] {
            let source = if flag == "missing" {
                source.replace("<Bypass>false</Bypass>", "")
            } else {
                source.replace(
                    "<Bypass>false</Bypass>",
                    &format!("<Bypass>{flag}</Bypass>"),
                )
            };
            let xml = with_chain(
                &source,
                DEFAULT_FLAGS,
                &[(200, track_matte_key_xml(200, 2, channel, inverted))],
            );
            let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
            let sequence = project.single_sequence().unwrap();
            let media: Vec<_> = sequence
                .video_items()
                .filter_map(PrVideoItem::media)
                .map(|clip| clip.id())
                .collect();
            let expected = if flag == "true" {
                vec![Some("VideoClipTrackItem:3"), Some("VideoClipTrackItem:9")]
            } else {
                vec![Some("VideoClipTrackItem:9")]
            };
            assert_eq!(
                media, expected,
                "{flag}/{channel}/{inverted}: {omissions:?}"
            );
            let graphics: Vec<_> = sequence
                .video_items()
                .filter_map(PrVideoItem::graphic)
                .collect();
            let ordinary = graphics
                .iter()
                .find(|graphic| graphic.id() == Some("VideoClipTrackItem:220"))
                .unwrap();
            assert_eq!(
                ordinary.text().document,
                graphic(&text_only_xml()).text().document
            );
            assert_eq!(ordinary.effect_loss.is_none(), flag == "true");
            assert_eq!(
                graphics
                    .iter()
                    .any(|graphic| graphic.id() == Some("VideoClipTrackItem:20")),
                flag == "true",
                "the rejected consumer's originally concealed matte must not become visible"
            );
            if flag != "true" {
                assert!(
                    omissions
                        .iter()
                        .any(|omission| omission.scope == OmissionScope::Occurrence
                            && omission.record == "VideoClipTrackItem:3"
                            && omission
                                .reason
                                .contains("omitted Ramp (VideoFilterComponent:100)")
                            && omission.reason.contains("alpha coverage is unverified")),
                    "{omissions:?}"
                );
            }
        }
    }
}

#[test]
fn graphic_ramp_loss_keeps_siblings_above_native_mask_sources() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_graphic_masks_b_26_5.prproj");
    let xml = crate::format::read_xml(&path).unwrap();
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let baseline = inspect_project(&xml, None).unwrap();
    let native_graphic = baseline
        .video_items()
        .filter_map(PrVideoItem::graphic)
        .find(|graphic| graphic.id() == Some("VideoClipTrackItem:65"))
        .unwrap();
    let native_group = native_graphic
        .objects
        .iter()
        .find_map(|object| match object {
            PrGraphicObject::Group(group) => Some(group),
            _ => None,
        })
        .unwrap();
    let mask_index = native_group
        .objects
        .iter()
        .position(|object| object.mask_source().is_some())
        .unwrap();
    let unaffected_members = &native_group.objects[..mask_index];
    let unaffected_siblings: Vec<_> = native_graphic
        .objects
        .iter()
        .filter_map(|object| match object {
            PrGraphicObject::Group(_) if unaffected_members.is_empty() => None,
            PrGraphicObject::Group(group) => Some(PrGraphicObject::Group(
                crate::schema::text::PrGraphicGroup {
                    objects: unaffected_members.to_vec(),
                    ..group.clone()
                },
            )),
            object => {
                assert!(object.mask_source().is_none());
                Some(object.clone())
            }
        })
        .collect();
    // A native ordinary Shape above the mask's lower composite, flat and in
    // a SubGroup. Copy Shape120 with fresh IDs only for the group case, whose
    // members already use that component ID. Ramp comes after the mask.
    let sibling = parsed
        .root_element()
        .children()
        .find(|node| node.attribute("ObjectID") == Some("120"))
        .unwrap();
    let sibling = xml[sibling.range()]
        .replace(r#"ObjectID="120""#, r#"ObjectID="901""#)
        .replace("<ID>22</ID>", "<ID>98</ID>");
    for (chain_id, item_id, refs, expected_name) in [
        (
            "93",
            "VideoClipTrackItem:68",
            vec!["120", "129", "130", "900"],
            "T1",
        ),
        (
            "87",
            "VideoClipTrackItem:65",
            vec!["901", "118", "119", "120", "121", "900"],
            "T1",
        ),
    ] {
        let chain = parsed
            .root_element()
            .children()
            .find(|node| node.attribute("ObjectID") == Some(chain_id))
            .unwrap();
        let components = chain
            .descendants()
            .find(|node| node.has_tag_name("Components"))
            .unwrap();
        let refs: String = refs
            .iter()
            .enumerate()
            .map(|(index, reference)| {
                format!(r#"<Component Index="{index}" ObjectRef="{reference}"/>"#)
            })
            .collect();
        let mut changed = xml.clone();
        changed.replace_range(
            components.range(),
            &format!(r#"<Components Version="1">{refs}</Components>"#),
        );
        let end = changed.rfind("</").unwrap();
        changed.insert_str(end, &format!(r#"{sibling}<VideoFilterComponent ObjectID="900"><Component><ID>99</ID><DisplayName>Ramp</DisplayName><Bypass>false</Bypass><Intrinsic>false</Intrinsic></Component><MatchName>AE.ADBE Ramp</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>"#));
        let (project, omissions) = inspect_project_with_omissions(&changed, None).unwrap();
        let recovered = project
            .single_sequence()
            .unwrap()
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .find(|graphic| graphic.id() == Some(item_id))
            .unwrap();
        assert!(
            matches!(recovered.objects.first(), Some(PrGraphicObject::Shape(shape)) if shape.name == expected_name),
            "{chain_id}: {omissions:?}"
        );
        if chain_id == "93" {
            assert_eq!(recovered.objects.len(), 1, "{chain_id}: {omissions:?}");
        } else {
            assert_eq!(&recovered.objects[1..], unaffected_siblings, "only the mask and its lower composite may be dropped, not unrelated native siblings");
        }
        assert!(
            omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Feature
                    && omission
                        .reason
                        .contains("unverified alpha coverage for Mask with Shape/Text")
                    && omission.reason.contains("below")),
            "{chain_id}: {omissions:?}"
        );
    }
}

#[test]
fn graphic_ramp_loss_does_not_keep_attached_or_clip_mask_coverage() {
    let records = super::mask::mask(300, false);
    let shape = shape_xml(TEXT_THEN_SHAPE, FILL)
        .replace("<MatchName>AE.ADBE Shape</MatchName>", "<SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"300\"/></SubComponents><MatchName>AE.ADBE Shape</MatchName>")
        .replace("</PremiereData>", &format!("{records}</PremiereData>"));
    assert!(
        matches!(graphic(&shape).objects.as_slice(), [_, PrGraphicObject::Shape(shape)] if shape.mask.is_some())
    );
    let (project, omissions) =
        inspect_project_with_omissions(&with_graphic_ramp(&shape, ""), None).unwrap();
    let recovered = project
        .single_sequence()
        .unwrap()
        .video_items()
        .find_map(PrVideoItem::graphic)
        .unwrap();
    assert!(
        matches!(recovered.objects.as_slice(), [PrGraphicObject::Text(_)]),
        "{omissions:?}"
    );
    let clip = with_clip_opacity(OPACITY_AND_TEXT, "100.", "", (18, 0))
        .replace("<MatchName>AE.ADBE Opacity</MatchName>", "<SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"300\"/></SubComponents><MatchName>AE.ADBE Opacity</MatchName>")
        .replace("</PremiereData>", &format!("{records}</PremiereData>"));
    assert!(graphic(&clip).opacity_mask.is_some());
    let (project, omissions) =
        inspect_project_with_omissions(&with_graphic_ramp(&clip, ""), None).unwrap();
    assert_eq!(project.single_sequence().unwrap().video_items().count(), 1);
    assert!(
        omissions
            .iter()
            .any(|omission| omission.scope == OmissionScope::Occurrence
                && omission
                    .reason
                    .contains("omitted graphic Ramp beside a clip Opacity mask")),
        "{omissions:?}"
    );
}

#[test]
fn graphic_ramp_with_unconverted_attached_mask_keeps_the_mask_guard() {
    let input = with_graphic_ramp(
        &text_only_xml(),
        r#"<SubComponents Version="1"><SubComponent Index="0" ObjectRef="100"/></SubComponents>"#,
    );
    let (project, omissions) = inspect_project_with_omissions(&input, None).unwrap();
    assert_eq!(
        project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .count(),
        1
    );
    assert!(
        omissions
            .iter()
            .any(|omission| omission.scope == OmissionScope::Occurrence
                && omission.reason.contains("graphic Ramp")
                && omission.reason.contains("mask")),
        "{omissions:?}"
    );
}

#[test]
fn legacy_static_text_layout_and_redundant_uniform_scale_read_editable_values() {
    let xml = legacy_static_text_xml();
    // Each adjacent gate alone, and both together. The existing complete
    // modern layout and default width remain covered by the other tests.
    for (input, expected_scale) in [
        (xml.replace("00000,37.5,", "00000,100.,"), 100.0),
        (xml.clone(), 37.5),
    ] {
        let (project, omissions) = inspect_project_with_omissions(&input, None).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        assert_eq!(
            project
                .single_sequence()
                .unwrap()
                .video_occurrences()
                .count(),
            1
        );
        let text = graphic(&input).text().clone();
        assert_eq!(text.document, legacy_document());
        assert_eq!(text.transform.position, [1440.0, 540.0]);
        assert_eq!(text.transform.anchor, [19.2, 21.6]);
        assert_eq!(text.transform.scale, expected_scale);
        assert_eq!(
            (text.transform.rotation, text.transform.opacity),
            (0.0, 60.0)
        );
        assert!(text.animations.is_empty() && text.source_text_keys.is_empty());
    }
}

#[test]
fn legacy_static_text_redundant_uniform_scale_reads_editable_values() {
    // The complete current layout isolates the width gate from the absent tail.
    let xml = legacy_static_text_xml().replace(
        "</Params><ID>4</ID>",
        r#"<Param Index="21" ObjectRef="62"/></Params><ID>4</ID>"#,
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .count(),
        1
    );
    let text = graphic(&xml).text().clone();
    assert_eq!(text.document, legacy_document());
    assert_eq!(text.transform.scale, 37.5);
    assert!(text.animations.is_empty() && text.source_text_keys.is_empty());
}

#[test]
fn legacy_static_text_inactive_width_keeps_uniform_scale() {
    // Saved legacy Text controls: Scale 23.854166030884, Width 10.416666030884,
    // Uniform true. The public native-record scaffold supplies the graph and
    // a single-style document; this is not a new Adobe-rendered fixture.
    let scale = 23.854166030884;
    let xml = legacy_static_text_xml().replace(
        "<ParameterID>4</ParameterID><StartKeyframe>-91445760000000000,37.5,",
        &format!("<ParameterID>4</ParameterID><StartKeyframe>-91445760000000000,{scale},"),
    );
    for width in [0.0, 10.416666030884, 37.5, 100.0, 4000.0] {
        let xml = xml.replace(
            "<ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,37.5,",
            &format!("<ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,{width},"),
        );
        for complete in [false, true] {
            let input = if complete {
                xml.replace(
                    "</Params><ID>4</ID>",
                    r#"<Param Index="21" ObjectRef="62"/></Params><ID>4</ID>"#,
                )
            } else {
                xml.clone()
            };
            let (project, mut omissions) = inspect_project_with_omissions(&input, None).unwrap();
            assert!(omissions.is_empty(), "{omissions:?}");
            let sequence = project.single_sequence().unwrap();
            let text = sequence
                .video_items()
                .find_map(PrVideoItem::graphic)
                .unwrap()
                .text();
            assert_eq!(text.horizontal_scale, None);
            assert_eq!(text.transform.scale, scale);
            assert_eq!(text.document, legacy_document());
            let document = crate::convert::premiere_to_tesseract(
                sequence,
                &project.media,
                &crate::tesseract_output::asset_ids_in_order(sequence, &project.media),
                &mut omissions,
            )
            .unwrap()
            .to_json_value()
            .unwrap();
            let text = document["composition"]["layers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|layer| layer["type"] == "Text")
                .unwrap();
            assert_eq!(
                text["transform"]["scale"],
                serde_json::json!([scale, scale])
            );
            assert_eq!(
                text["transform"]["position"],
                serde_json::json!([1440.0, 540.0])
            );
            assert_eq!(text["transform"]["opacity"], 60.0);
        }
    }
}

#[test]
fn legacy_static_text_admission_keeps_layout_scale_animation_and_fixed_guards() {
    let xml = legacy_static_text_xml();
    let mut rejected = vec![
        (
            "missing parent rotation",
            xml.replace(r#"<Param Index="20" ObjectRef="61"/>"#, ""),
        ),
        (
            "wrong ID",
            xml.replace(
                "<ParameterID>21</ParameterID>",
                "<ParameterID>22</ParameterID>",
            ),
        ),
        (
            "wrong name",
            xml.replace(
                "<Name>Parent Rotation</Name>",
                "<Name>Unknown Rotation</Name>",
            ),
        ),
        (
            "wrong record kind",
            xml.replace(r#"Index="2" ObjectRef="43""#, r#"Index="2" ObjectRef="44""#),
        ),
        (
            "wrong parameter class",
            xml.replace(
                r#"ObjectID="45" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3""#,
                r#"ObjectID="45" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461""#,
            ),
        ),
        (
            "out-of-range inactive width",
            xml.replace(
                "<ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,37.5,",
                "<ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,4000.5,",
            ),
        ),
        (
            "negative inactive width",
            xml.replace(
                "<ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,37.5,",
                "<ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,-1,",
            ),
        ),
        (
            "uniform off",
            xml.replace(
                "<ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,true,",
                "<ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,false,",
            ),
        ),
        (
            "unknown uniform",
            xml.replace(
                "<ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,true,",
                "<ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,1,",
            ),
        ),
        (
            "nonfinite width",
            xml.replace(
                "<ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,37.5,",
                "<ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,NaN,",
            ),
        ),
        ("negative scales", xml.replace("00000,37.5,", "00000,-1,")),
        (
            "modern short layout",
            text_only_xml().replace(r#"<Param Index="21" ObjectRef="62"/>"#, ""),
        ),
        (
            "modern nondefault width",
            text_only_xml().replace(
                "<ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,100.,",
                "<ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,37.5,",
            ),
        ),
    ];
    for id in [2, 10, 11, 12, 15, 16, 17, 18, 19, 20, 21] {
        let old = format!("<ParameterID>{id}</ParameterID><StartKeyframe>-91445760000000000,");
        let start = xml.find(&old).unwrap() + old.len();
        let end = start + xml[start..].find(',').unwrap();
        let active = if xml[start..end].contains("false") {
            "true"
        } else {
            "1."
        };
        let changed = format!("{}{active}{}", &xml[..start], &xml[end..]);
        if matches!(id, 11 | 12 | 19 | 20 | 21) {
            let (project, omissions) = inspect_project_with_omissions(&changed, None).unwrap();
            assert_eq!(project.single_sequence().unwrap().video_items().count(), 2);
            assert!(
                omissions
                    .iter()
                    .any(|o| o.kind == OmissionKind::Approximated
                        && o.reason.contains(&format!("parameter {id} "))),
                "{omissions:?}"
            );
        } else {
            rejected.push(("active fixed control", changed));
        }
    }
    rejected.push((
        "active current tail",
        xml.replace(
            "</Params><ID>4</ID>",
            r#"<Param Index="21" ObjectRef="62"/></Params><ID>4</ID>"#,
        )
        .replace(
            "<ParameterID>22</ParameterID><StartKeyframe>-91445760000000000,false,",
            "<ParameterID>22</ParameterID><StartKeyframe>-91445760000000000,true,",
        ),
    ));
    for object in [43, 44, 45, 46, 47, 48] {
        rejected.push((
            "legacy transform keys",
            keyed(
                &xml,
                object,
                if object == 43 {
                    TWO_POINT_KEYS
                } else {
                    TWO_SCALAR_KEYS
                },
            ),
        ));
    }
    for (case, input) in rejected {
        let (project, omissions) = inspect_project_with_omissions(&input, None).unwrap();
        let sequence = project.single_sequence().unwrap();
        assert_eq!(
            (
                sequence.video_items().count(),
                sequence.video_occurrences().count()
            ),
            (1, 1),
            "{case}: {omissions:?}"
        );
        assert!(
            omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Occurrence
                    && omission.record == "20"),
            "{case}: {omissions:?}"
        );
    }
}

#[test]
fn legacy_json_source_text_reads_as_editable_text_and_an_unsupported_form_omits_only_its_graphic() {
    use crate::tests::support::{legacy_source_text, legacy_source_text_payload};
    // The native Text component, alone in its chain, with our own legacy
    // Source Text in place of its Premiere 26 one.
    let legacy_xml = |text: &serde_json::Value| {
        graphic_xml(&STANDARD.encode(legacy_source_text_payload(&text.to_string())))
            .replace(TWO_COMPONENTS, r#"<Component Index="0" ObjectRef="40"/>"#)
    };
    let xml = legacy_xml(&legacy_source_text());
    let (_, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let read = graphic(&xml);
    let text = read.text();
    assert_eq!(
        (text.name.as_str(), &text.document),
        ("Before label", &legacy_document())
    );
    assert!(text.source_text_keys.is_empty() && text.animations.is_empty());
    // The component's transform reads as beside a Premiere 26 document.
    assert_eq!(text.transform, graphic(&text_only_xml()).text().transform);
    // An unconverted legacy mask, and the same legacy document as a Source
    // Text key, omit only their graphic; the video stays.
    let mut masked = legacy_source_text();
    masked["mTextParam"]["mIsMask"] = serde_json::json!(true);
    let legacy_key = STANDARD.encode(legacy_source_text_payload(
        &legacy_source_text().to_string(),
    ));
    for (xml, reason) in [
        (
            legacy_xml(&masked),
            "unsupported conversion: ArbVideoComponentParam:41: legacy UTF-16 JSON Source Text: active mask is unsupported",
        ),
        (
            with_source_text_keys(None, &format!("{IN},{legacy_key};")),
            "unsupported conversion: ArbVideoComponentParam:41: legacy UTF-16 JSON text from Premiere before 26 converts only as a static graphic Source Text value",
        ),
    ] {
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        let sequence = project.single_sequence().unwrap();
        assert_eq!(
            (
                sequence.video_items().count(),
                sequence.video_occurrences().count()
            ),
            (1, 1),
            "{reason}"
        );
        let omitted: Vec<_> = omissions
            .iter()
            .map(|omission| {
                (
                    omission.scope,
                    omission.record.as_str(),
                    omission.reason.as_str(),
                )
            })
            .collect();
        assert_eq!(omitted, [(OmissionScope::Occurrence, "20", reason)]);
    }
}

#[test]
fn legacy_layout_and_unmapped_styles_keep_editable_text_paint_and_video_sibling() {
    use crate::tests::support::{legacy_run, legacy_source_text, legacy_source_text_payload};
    use serde_json::json;

    // Supplementary authored payload in the public native-record scaffold,
    // not an independent Adobe-native legacy layout/fidelity case.
    let mut text = legacy_source_text();
    text["mTextParam"]["mWidth"] = json!(56);
    text["mTextParam"]["mHeight"] = json!(10);
    text["mTextParam"]["mLeading"] = json!(7.5);
    text["mTextParam"]["mBackFillVisible"] = json!(false);
    let style = &mut text["mTextParam"]["mStyleSheet"];
    style["mStrokeVisible"] = legacy_run(json!(true));
    style["mStrokeColor"] = legacy_run(json!(0xff_ffff));
    style["mStrokeWidth"] = legacy_run(json!(3));
    style["mFillOverStroke"] = legacy_run(json!(true));
    style["mFauxBold"] = legacy_run(json!(true));
    style["mUnderline"] = legacy_run(json!(true));
    let xml = graphic_xml(&STANDARD.encode(legacy_source_text_payload(&text.to_string())))
        .replace(TWO_COMPONENTS, r#"<Component Index="0" ObjectRef="40"/>"#);
    let (project, mut omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    for field in ["mFauxBold", "mUnderline"] {
        assert!(
            omissions
                .iter()
                .any(|omission| omission.reason.contains(field)),
            "{field}"
        );
    }
    assert!(omissions
        .iter()
        .all(|omission| omission.scope == OmissionScope::Feature));
    let sequence = project.single_sequence().unwrap();
    let document = crate::convert::premiere_to_tesseract(
        sequence,
        &project.media,
        &crate::tesseract_output::asset_ids_in_order(sequence, &project.media),
        &mut omissions,
    )
    .unwrap();
    let mut value = document.to_json_value().unwrap();
    let layers = value["composition"]["layers"].as_array_mut().unwrap();
    let video = layers
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap()
        .clone();
    let layer = layers
        .iter_mut()
        .find(|layer| layer["type"] == "Text")
        .unwrap();
    assert_eq!(
        layer["sourceText"]["text"],
        "Night\nMarket \u{2713} \u{1f525}"
    );
    assert_eq!(layer["sourceText"]["fontFamily"], "Inter-SemiBold");
    assert_eq!(layer["sourceText"]["fontSize"], 64.5);
    assert_eq!(layer["sourceText"]["boxSize"], json!([56.0, 10.0]));
    assert_eq!(
        layer["sourceText"]["strokeColor"],
        json!([1.0, 1.0, 1.0, 1.0])
    );
    assert_eq!(layer["sourceText"]["strokeWidth"], 6.0);
    let leading = layer["sourceText"]["leading"].as_f64().unwrap();
    assert_eq!(leading, f64::from(1.2_f32 * 64.5_f32) + 7.5);
    assert_eq!(
        layer["activeRange"],
        json!({"start": 1000, "duration": 2000})
    );
    layer["sourceText"]["text"] = json!("Editable recovered title");
    assert_eq!(
        layers
            .iter()
            .find(|layer| layer["type"] == "Video")
            .unwrap(),
        &video
    );
    let edited = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    assert!(!edited
        .to_json_value()
        .unwrap()
        .to_string()
        .contains("JsScript"));
}

#[test]
fn legacy_json_source_text_placements_edit_apart_and_export_their_current_modern_text() {
    use crate::schema::PrProjectFile;
    use crate::tests::support::{legacy_source_text, legacy_source_text_payload};
    use serde_json::json;
    use std::io::Read;
    // Two placements, from 0 s and 3 s, share one stored legacy Source Text.
    let legacy = legacy_source_text_payload(&legacy_source_text().to_string());
    let records = legacy_static_text_layout(
        &graphic_source_records(&STANDARD.encode(&legacy))
            .replace(TWO_COMPONENTS, r#"<Component Index="0" ObjectRef="40"/>"#),
    );
    let track = r#"<VideoClipTrack ObjectUID="track-2"><ClipTrack><Track><ID>2</ID><Index>1</Index></Track><ClipItems><TrackItems><TrackItem ObjectRef="20"/><TrackItem ObjectRef="27"/></TrackItems><Index>1</Index></ClipItems></ClipTrack></VideoClipTrack>"#;
    let xml = SOURCE
        .replace(
            r#"<Track ObjectURef="track-1"/>"#,
            r#"<Track ObjectURef="track-1"/><Track ObjectURef="track-2" Index="1"/>"#,
        )
        .replace(
            "</PremiereData>",
            &format!(
                "{track}\n{}{}{records}</PremiereData>",
                graphic_item_record(20, 0),
                graphic_item_record(27, 3 * TICKS)
            ),
        );
    let mut document = point_title_document(&xml);
    // A graphic-only export: drop the video layer.
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    layers.retain(|layer| layer["type"] != "Video");
    let text_from = |layers: &[serde_json::Value], start: u64| {
        layers
            .iter()
            .position(|layer| layer["type"] == "Text" && layer["activeRange"]["start"] == start)
            .unwrap_or_else(|| panic!("no text layer from {start} ms: {layers:?}"))
    };
    let (a, b) = (text_from(layers, 0), text_from(layers, 3000));
    assert_ne!(layers[a]["id"], layers[b]["id"]);
    for layer in [a, b] {
        assert_eq!(
            layers[layer]["sourceText"]["text"],
            "Night\nMarket \u{2713} \u{1f525}"
        );
    }
    // Edit only A: its text, fill, tracking and position.
    let b_before = layers[b].clone();
    layers[a]["sourceText"]["text"] = json!("Edited \u{2713}");
    layers[a]["sourceText"]["fillColor"] = json!([1.0, 0.4, 0.0, 1.0]);
    layers[a]["sourceText"]["tracking"] = json!(-10.0);
    layers[a]["transform"]["position"] = json!([480.0, 270.0]);
    layers[a]["transform"]["scale"] = json!([62.5, 62.5]);
    assert_eq!(layers[b], b_before);
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(document).unwrap();
    let mut omissions = Vec::new();
    let project = crate::convert::tesseract_to_premiere(
        &document,
        &std::collections::BTreeMap::new(),
        &std::collections::BTreeMap::new(),
        &std::collections::BTreeMap::new(),
        crate::format::FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    // Write the current content and read it back.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy-source-text.prproj");
    crate::format::PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let mut written = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(&path).unwrap())
        .read_to_string(&mut written)
        .unwrap();
    let (reopened, reopen_omissions) = PrProjectFile::load(&path).unwrap();
    assert!(reopen_omissions.is_empty(), "{reopen_omissions:?}");
    // Each placement keeps its own range, position and document.
    let original = legacy_document();
    let edited = crate::schema::text::PrTextDocument {
        text: "Edited \u{2713}".into(),
        fill: Some(PrRgb([255, 102, 0])),
        tracking: -10.0,
        ..original.clone()
    };
    let texts = |project: &PrProjectFile| {
        let mut texts: Vec<_> = project
            .single_sequence()
            .unwrap()
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .map(|graphic| {
                (
                    graphic.start_ticks,
                    graphic.end_ticks,
                    graphic.text().transform.position,
                    graphic.text().transform.scale,
                    graphic.text().document.clone(),
                )
            })
            .collect();
        texts.sort_by_key(|text| text.0);
        texts
    };
    let expected = vec![
        (0, 2 * TICKS, [480.0, 270.0], 62.5, edited.clone()),
        (
            3 * TICKS,
            5 * TICKS,
            [1440.0, 540.0],
            37.5,
            original.clone(),
        ),
    ];
    assert_eq!(texts(&project), expected);
    assert_eq!(texts(&reopened), expected);
    // The written Source Text values are the Premiere 26 payloads of the
    // current documents; the legacy JSON is never replayed.
    let mut stored: Vec<Vec<u8>> = written
        .split("<Name>Source Text</Name>")
        .skip(1)
        .map(|record| {
            let value = &record[record.find("<StartKeyframeValue").unwrap()..];
            let start = value.find('>').unwrap() + 1;
            let end = value.find("</StartKeyframeValue>").unwrap();
            STANDARD.decode(&value[start..end]).unwrap()
        })
        .collect();
    stored.sort();
    let mut current = vec![encode(&edited).unwrap(), encode(&original).unwrap()];
    current.sort();
    assert_eq!(stored, current);
    assert!(stored.iter().all(|payload| payload != &legacy
        && decode(payload).is_ok_and(|decoded| decoded.omitted.is_empty())));
}

#[test]
fn a_media_chain_still_reports_a_subgroup_map() {
    let (_, omissions) = inspect_project_with_omissions(
        &SOURCE.replacen(
            "<DefaultMotion>",
            "<ComponentGroupMap ObjectRef=\"1\"/><DefaultMotion>",
            1,
        ),
        None,
    )
    .unwrap();
    assert!(
        omissions
            .iter()
            .any(|report| report.reason == "ComponentGroupMap not converted"),
        "{omissions:?}"
    );
}

#[test]
fn adobe_subgroup_map_rejects_a_missing_parent_without_exposing_its_members() {
    use std::io::Read;
    let bytes = include_bytes!("../../../tests/fixtures/feature_graphic_masks_b_26_5.prproj");
    let mut xml = String::new();
    flate2::read::GzDecoder::new(bytes.as_slice())
        .read_to_string(&mut xml)
        .unwrap();
    let start = xml.find("<ParentPinID>").unwrap() + "<ParentPinID>".len();
    let end = start + xml[start..].find("</ParentPinID>").unwrap();
    xml.replace_range(start..end, "999999");
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(
        omissions
            .iter()
            .any(|report| report.reason.contains("pinned to SubGroup 999999")),
        "{omissions:?}"
    );
    assert!(!project
        .single_sequence()
        .unwrap()
        .video_items()
        .filter_map(PrVideoItem::graphic)
        .any(|graphic| graphic.id() == Some("VideoClipTrackItem:65")));
}

#[test]
fn an_unsupported_shape_attachment_omits_only_its_owner_not_other_objects() {
    let xml = shape_xml(TEXT_THEN_SHAPE, FILL).replace(
        "<MatchName>AE.ADBE Shape</MatchName>",
        "<SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"23\"/></SubComponents><MatchName>AE.ADBE Shape</MatchName>",
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.video_items().count(), 2);
    let graphic = sequence
        .video_items()
        .find_map(PrVideoItem::graphic)
        .unwrap();
    assert!(matches!(
        graphic.objects.as_slice(),
        [PrGraphicObject::Text(_)]
    ));
    assert!(
        omissions
            .iter()
            .any(|report| report.reason.contains("its attached mask cannot convert")),
        "{omissions:?}"
    );
}

#[test]
fn attached_numeric_keys_reader_keeps_owner_and_healthy_text() {
    for name in ["Mask Feather", "Mask Expansion", "Mask Opacity"] {
        let records = super::mask::numeric_mask_record_keys(
            super::mask::mask(300, false),
            name,
            "914457600000000,100.,0,0,0,0,0,0;914457854016000,100.,0,0,0,0,0,0;",
        );
        let xml = shape_xml(TEXT_THEN_SHAPE, FILL)
            .replace("<MatchName>AE.ADBE Shape</MatchName>", "<SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"300\"/></SubComponents><MatchName>AE.ADBE Shape</MatchName>")
            .replace("</PremiereData>", &format!("{records}</PremiereData>"));
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        let graphic = project
            .single_sequence()
            .unwrap()
            .video_items()
            .find_map(PrVideoItem::graphic)
            .unwrap();
        let [PrGraphicObject::Text(_), PrGraphicObject::Shape(shape)] = graphic.objects.as_slice()
        else {
            panic!("{name}: {omissions:?}");
        };
        let mask = shape.mask.as_ref().unwrap();
        let keyed: Vec<_> = mask
            .numeric_keys()
            .into_iter()
            .filter(|(_, keys)| !keys.is_empty())
            .collect();
        assert_eq!(keyed.len(), 1);
        assert_eq!(keyed[0].1.len(), 2);
        assert_eq!(keyed[0].1[1].source_ticks, 914457854016000);
        assert_eq!(keyed[0].1[1].value, 100.0);
    }
}

#[test]
fn unequal_graphic_spans_reject_numeric_clip_mask_keys() {
    for name in ["Mask Feather", "Mask Expansion", "Mask Opacity"] {
        let records = super::mask::numeric_mask_record_keys(
            super::mask::mask(300, false),
            name,
            "0,20.,0,0,0,0,0,0;254016000000,40.,0,0,0,0,0,0;",
        );
        let xml = with_clip_opacity(OPACITY_AND_TEXT, "100.", "", (18, 0))
            .replace(
                "<MatchName>AE.ADBE Opacity</MatchName>",
                "<SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"300\"/></SubComponents><MatchName>AE.ADBE Opacity</MatchName>",
            )
            .replace("</PremiereData>", &format!("{records}</PremiereData>"));
        let accepted = graphic(&xml);
        assert!(accepted.opacity_mask.unwrap().has_numeric_keys(), "{name}");
        let changed = xml.replace(
            "<OutPoint>914669280000000</OutPoint>",
            "<OutPoint>914923296000000</OutPoint>",
        );
        let (project, omissions) = inspect_project_with_omissions(&changed, None).unwrap();
        assert_eq!(project.single_sequence().unwrap().video_items().count(), 1);
        assert!(
            omissions.iter().any(|omission| {
                omission.scope == OmissionScope::Occurrence
                    && omission.reason.contains("graphic retiming is unsupported")
            }),
            "{name}: {omissions:?}"
        );
    }
}

#[test]
fn unequal_graphic_spans_check_native_mask_sources_and_subgroups() {
    use std::io::Read;
    let bytes = include_bytes!("../../../tests/fixtures/feature_graphic_masks_b_26_5.prproj");
    let mut xml = String::new();
    flate2::read::GzDecoder::new(bytes.as_slice())
        .read_to_string(&mut xml)
        .unwrap();
    let find = |xml: &str, id: &str| {
        let (project, _) = inspect_project_with_omissions(xml, None).unwrap();
        let graphic = project
            .single_sequence()
            .unwrap()
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .find(|graphic| graphic.id() == Some(id))
            .cloned();
        graphic
    };
    let mask = find(&xml, "VideoClipTrackItem:68").unwrap();
    assert!(mask
        .objects
        .iter()
        .any(|object| object.mask_source().is_some()));
    let group = find(&xml, "VideoClipTrackItem:65").unwrap();
    assert!(group
        .objects
        .iter()
        .any(|object| matches!(object, PrGraphicObject::Group(_))));
    // Native static SubGroup and mask-source records remain accepted. Only
    // their placed source span is changed; the saved object content is intact.
    let unequal = |xml: &str, clip: &str| {
        let mut xml = xml.to_owned();
        let start = xml
            .find(&format!("<VideoClip ObjectID=\"{clip}\""))
            .unwrap();
        let end = start + xml[start..].find("</VideoClip>").unwrap();
        let changed = xml[start..end].replace(
            "<OutPoint>254016000000</OutPoint>",
            "<OutPoint>508032000000</OutPoint>",
        );
        assert_ne!(changed, xml[start..end]);
        xml.replace_range(start..end, &changed);
        xml
    };
    assert_eq!(
        find(&unequal(&xml, "122"), "VideoClipTrackItem:65")
            .unwrap()
            .objects,
        group.objects
    );
    assert_eq!(
        find(&unequal(&xml, "131"), "VideoClipTrackItem:68")
            .unwrap()
            .objects,
        mask.objects
    );
    // Synthetic keys on the native mask-source Text exercise the same reader,
    // not a claim of native-authored animation or a new interpolation proof.
    let keyed = keyed(&xml, 293, TWO_SCALAR_KEYS);
    let accepted = find(&keyed, "VideoClipTrackItem:68").unwrap();
    assert!(accepted.objects.iter().any(|object| matches!(object,
        PrGraphicObject::Text(text) if text.mask_source.is_some() && !text.animations.is_empty()
    )));
    let changed = unequal(&keyed, "131");
    assert!(find(&changed, "VideoClipTrackItem:68").is_none());
    let (_, omissions) = inspect_project_with_omissions(&changed, None).unwrap();
    assert!(
        omissions.iter().any(|omission| omission
            .reason
            .contains("VideoClip:131: graphic retiming is unsupported")),
        "{omissions:?}"
    );
}

#[test]
fn object_mask_sampling_keeps_co_resident_native_clock_graphic() {
    let step = crate::format::object_mask::SAVED_SEQUENCE_FRAME_TICKS;
    let mask = include_str!("../../../tests/fixtures/object_mask/opacity.xml");
    let xml=graphic_xml(BEFORE)
        .replace("<FrameRate>8467200000</FrameRate>",&format!("<FrameRate>{step}</FrameRate>"))
        .replace("<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>",
            "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"665\"/></Components></ComponentChain></VideoComponentChain>")
        .replace("<End>1270080000000</End>",&format!("<End>{}</End>",150*step))
        .replace("<OutPoint>1270080000000</OutPoint>",&format!("<OutPoint>{}</OutPoint>",150*step))
        .replace("<Start>254016000000</Start><End>762048000000</End>",&format!("<Start>{}</Start><End>{}</End>",30*step,90*step))
        .replace("<OutPoint>914669280000000</OutPoint>",&format!("<OutPoint>{}</OutPoint>",914161248000000_i64+60*step))
        .replace("</PremiereData>",&format!("{mask}</PremiereData>"));
    let (project, notes) = inspect_project_with_omissions(&xml, None).unwrap();
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.frame_rate, crate::schema::FrameRate::Fps30);
    assert_eq!(sequence.native_frame_ticks, Some(step));
    assert_eq!(
        sequence
            .video_tracks
            .iter()
            .flat_map(|t| &t.items)
            .filter(|i| matches!(i, PrVideoItem::Graphic(_)))
            .count(),
        1,
        "{notes:?}"
    );
    assert!(!notes
        .iter()
        .any(|n| n.reason.contains("graphic frame size or rate")));
    let editable = crate::tests::support::project_document_with_media(sequence, &project.media);
    assert!(editable.to_string().contains("\"type\":\"Text\""));
    let graphic_owned = xml.replace("<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"665\"/></Components></ComponentChain></VideoComponentChain>",
        "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>")
        .replace("<Component Index=\"1\" ObjectRef=\"40\"/>","<Component Index=\"1\" ObjectRef=\"40\"/><Component Index=\"2\" ObjectRef=\"665\"/>");
    let (project, notes) = inspect_project_with_omissions(&graphic_owned, None).unwrap();
    assert_eq!(
        project
            .single_sequence()
            .unwrap()
            .frame_rate
            .ticks_per_frame(),
        step
    );
    assert!(!notes
        .iter()
        .any(|n| n.reason.contains("Object Mask sequence cadence")));
}
