//! Portable native Motion fixture shared with the internal runtime conformance tests.

pub(crate) const SOURCE: &str = include_str!("../fixtures/one-clip.xml");

// Synthesized around the real Premiere Motion/Rotation record shape. The public
// Adobe lesson samples contain Bezier/position effects and unsupported media, so
// they cannot themselves serve as compatible end-to-end fixtures.
pub(crate) fn animated_xml(keys: &str) -> String {
    let mut params = String::new();
    for (id, name, value, point) in [
        (1, "Position", "0.5:0.5", true),
        (2, "Scale", "100.", false),
        (3, "Scale Width", "100.", false),
        (4, " ", "true", false),
        (5, "Rotation", "0.", false),
        (6, "Anchor Point", "0.5:0.5", true),
        (7, "Anti-flicker Filter", "0.", false),
    ] {
        let tag = if point {
            "PointComponentParam"
        } else {
            "VideoComponentParam"
        };
        let initial = if point {
            format!("-91445760000000000,{value},0,0,0,0,0,0,5,4,0,0,0,0")
        } else {
            format!("-91445760000000000,{value},0,0,0,0,0,0")
        };
        let animation = if id == 5 {
            format!("<Keyframes>{keys}</Keyframes>")
        } else {
            String::new()
        };
        params.push_str(&format!("<{tag} ObjectID=\"{}\"><Name>{name}</Name><ParameterID>{id}</ParameterID><StartKeyframe>{initial}</StartKeyframe>{animation}</{tag}>", id + 10));
    }
    let refs = (1..=7)
        .map(|id| format!("<Param Index=\"{}\" ObjectRef=\"{}\"/>", id - 1, id + 10))
        .collect::<String>();
    let replacement = "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>";
    let chain = "<VideoComponentChain ObjectID=\"4\"><DefaultOpacity>true</DefaultOpacity><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"9\"/></Components></ComponentChain></VideoComponentChain>";
    let xml = SOURCE.replace(replacement, chain);
    assert_ne!(xml, SOURCE);
    xml.replace("</PremiereData>", &format!("<VideoFilterComponent ObjectID=\"9\"><Component><Params>{refs}</Params><DisplayName>Motion</DisplayName><Bypass>false</Bypass><Intrinsic>true</Intrinsic></Component><MatchName>AE.ADBE Motion</MatchName></VideoFilterComponent>{params}</PremiereData>"))
}

/// The Premiere 26.3 Opacity of `feature_opacity_screen_strict.prproj`
/// (`VideoFilterComponent:200`, Opacity 50, Normal) with `KEYS` in place of
/// its keyframe list, naming the `cinemagraph` mask `VideoFilterComponent:102`
/// (records 102 to 117 verbatim: Feather 30, Opacity 100, not inverted, the
/// pen path of five vertices in unit-frame fractions).
const MASKED_OPACITY: &str = r#"<VideoFilterComponent ObjectID="200" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="7"><Component Version="5"><Params Version="1"><Param Index="0" ObjectRef="201"/><Param Index="1" ObjectRef="202"/><Param Index="2" ObjectRef="203"/></Params><ID>2</ID><DisplayName>Opacity</DisplayName><Bypass>false</Bypass><Intrinsic>true</Intrinsic></Component><SubComponents Version="1"><SubComponent Index="0" ObjectRef="102"/></SubComponents><MatchName>AE.ADBE Opacity</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>
<VideoComponentParam ObjectID="201" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="9"><Name>Opacity</Name><ParameterControlType>2</ParameterControlType><StartKeyframe>-91445760000000000,50.,0,0,0,0,0,0</StartKeyframe>KEYS<LowerBound>0</LowerBound><UpperBound>100</UpperBound><ParameterID>1</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID="202" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="9"><Name>Blend Mode</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>10</ParameterControlType><StartKeyframe>-91445760000000000,18,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>26</UpperBound><ParameterID>2</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID="203" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="9"><Name>Blend Mode</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>31</UpperBound><ParameterID>3</ParameterID></VideoComponentParam>
<VideoFilterComponent ObjectID="102" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="8"><Component Version="6"><Params Version="1"><Param Index="0" ObjectRef="103"/><Param Index="1" ObjectRef="104"/><Param Index="2" ObjectRef="105"/><Param Index="3" ObjectRef="106"/><Param Index="4" ObjectRef="107"/><Param Index="5" ObjectRef="108"/><Param Index="6" ObjectRef="109"/><Param Index="7" ObjectRef="110"/><Param Index="8" ObjectRef="111"/><Param Index="9" ObjectRef="112"/><Param Index="10" ObjectRef="113"/><Param Index="11" ObjectRef="114"/><Param Index="12" ObjectRef="115"/><Param Index="13" ObjectRef="116"/><Param Index="14" ObjectRef="117"/></Params><ID>0</ID><DisplayName>Mask</DisplayName><InstanceName>1</InstanceName><Bypass>false</Bypass><Intrinsic>false</Intrinsic><ArchivedType>0</ArchivedType></Component><PremiereFilterPrivateData Encoding="base64" BinaryHash="9fbd2923-ddd9-cb82-c26f-f75f00000064">a2NpbgEAAAAAAAAAAAAAAAAAAAAAAAAAq6oIQ6qqkkMAAAhDAICSQ4Dl+f//////AQAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==</PremiereFilterPrivateData><MatchName>AE.ADBE AEMask</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>
<VideoComponentParam ObjectID="103" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="9"><IsTimeVarying>false</IsTimeVarying><ParameterControlType>11</ParameterControlType><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>false</UpperBound><ParameterID>1</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID="104" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="9"><IsTimeVarying>false</IsTimeVarying><ParameterControlType>16</ParameterControlType><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound><ParameterID>2</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID="105" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="9"><IsTimeVarying>false</IsTimeVarying><ParameterControlType>16</ParameterControlType><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound><ParameterID>3</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID="106" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="9"><IsTimeVarying>false</IsTimeVarying><ParameterControlType>16</ParameterControlType><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound><ParameterID>4</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID="107" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="9"><IsTimeVarying>false</IsTimeVarying><ParameterControlType>16</ParameterControlType><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound><ParameterID>14</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID="108" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="9"><IsTimeVarying>false</IsTimeVarying><ParameterControlType>16</ParameterControlType><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound><ParameterID>15</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID="109" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="9"><IsTimeVarying>false</IsTimeVarying><ParameterControlType>12</ParameterControlType><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>false</UpperBound><ParameterID>5</ParameterID></VideoComponentParam>
<ArbVideoComponentParam ObjectID="110" ClassID="313e54d4-6903-49ad-b0bf-8262cdd10f4e" Version="2"><Name>Mask Path</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>22</ParameterControlType><StartKeyframePosition>-91445760000000000</StartKeyframePosition><StartKeyframeValue Encoding="base64" BinaryHash="895a1cc1-7d8d-7f19-e8fd-f8eb000000bc">MmNpbgIAAAAAAAAABQAAAAEAAADNzEw+LtiCP0kXSz7zkYE/UYJOPmoehD8BAAAAAAAAAFVVtT6Y0B4/VVW1PpjQHj9VVbU+mNAePwEAAAAAAAAAVVU3P/qkzz5VVTc/+qTPPlVVNz/6pM8+AQAAAAAAAABVVWc/XXnAPlVVZz9decA+VVVnP115wD4BAAAAAAAAADMzZT8ofYI/MzNlPyh9gj8zM2U/KH2CPwEAAAA=</StartKeyframeValue><ParameterID>6</ParameterID></ArbVideoComponentParam>
<VideoComponentParam ObjectID="111" ClassID="a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542" Version="9"><Name>Mask Feather</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>8</ParameterControlType><StartKeyframe>-91445760000000000,30.,0,0,0,0,0,0</StartKeyframe><CurrentValue>30</CurrentValue><LowerBound>0</LowerBound><UpperBound>5000</UpperBound><ParameterID>7</ParameterID><UpperUIBound>300</UpperUIBound></VideoComponentParam>
<VideoComponentParam ObjectID="112" ClassID="a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542" Version="9"><Name>Mask Opacity</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>8</ParameterControlType><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound><ParameterID>8</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID="113" ClassID="a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542" Version="9"><Name>Mask Expansion</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>8</ParameterControlType><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>-5000</LowerBound><UpperBound>5000</UpperBound><ParameterID>9</ParameterID><LowerUIBound>-300</LowerUIBound><UpperUIBound>300</UpperUIBound></VideoComponentParam>
<VideoComponentParam ObjectID="114" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="9"><IsTimeVarying>false</IsTimeVarying><ParameterControlType>4</ParameterControlType><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound><ParameterID>10</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID="115" ClassID="a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542" Version="9"><IsTimeVarying>false</IsTimeVarying><ParameterControlType>8</ParameterControlType><StartKeyframe>-91445760000000000,2.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>3</UpperBound><ParameterID>11</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID="116" ClassID="a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542" Version="9"><IsTimeVarying>false</IsTimeVarying><ParameterControlType>8</ParameterControlType><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>4294967296</UpperBound><ParameterID>12</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID="117" ClassID="a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542" Version="9"><IsTimeVarying>false</IsTimeVarying><ParameterControlType>8</ParameterControlType><StartKeyframe>-91445760000000000,0.5,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>3.4028234663852886e+38</UpperBound><ParameterID>13</ParameterID></VideoComponentParam>
"#;

/// `one-clip.xml` whose clip keeps its Opacity, keyed by `keys` (the native
/// keyframe list; empty for a static clip), with the `cinemagraph` mask on it.
pub(crate) fn masked_opacity_xml(keys: &str) -> String {
    let replacement = "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>";
    let chain = "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"200\"/></Components></ComponentChain></VideoComponentChain>";
    let xml = SOURCE.replace(replacement, chain);
    assert_ne!(xml, SOURCE);
    let keyed = if keys.is_empty() {
        String::new()
    } else {
        format!("<Keyframes>{keys}</Keyframes>")
    };
    xml.replace(
        "</PremiereData>",
        &format!("{}</PremiereData>", MASKED_OPACITY.replace("KEYS", &keyed)),
    )
}

/// The `AE.ADBE Legacy Key Track Matte` of `horror_title` `VideoClipTrackItem:89`
/// (Premiere 14.4, `VideoFilterComponent` 8, `Component` 6) and its three
/// parameters, records `id` to `id + 3`, with `matte`, `composite` and
/// `reverse` in place of its Matte 7 (the `Track/ID` of the track at `Index`
/// 5), Composite Using 0 (Matte Alpha) and Reverse `false`.
pub(crate) fn track_matte_key_xml(id: u32, matte: u32, composite: u32, reverse: bool) -> String {
    let (matte_id, composite_id, reverse_id) = (id + 1, id + 2, id + 3);
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"8\"><Component Version=\"6\"><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"{matte_id}\"/><Param Index=\"1\" ObjectRef=\"{composite_id}\"/><Param Index=\"2\" ObjectRef=\"{reverse_id}\"/></Params><ID>3</ID><DisplayName>Track Matte Key</DisplayName><Bypass>false</Bypass><Intrinsic>false</Intrinsic><ArchivedType>0</ArchivedType></Component><MatchName>AE.ADBE Legacy Key Track Matte</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>\
<VideoComponentParam ObjectID=\"{matte_id}\" ClassID=\"2f2eb0a3-318c-4a93-99fc-f1d319edc864\" Version=\"9\"><Name>Matte:</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>13</ParameterControlType><StartKeyframe>-91445760000000000,{matte},0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>4294967295</UpperBound><ParameterID>1</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{composite_id}\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"9\"><Name>Composite Using:</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,{composite},0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>1</UpperBound><ParameterID>2</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"{reverse_id}\" ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\" Version=\"9\"><Name>Reverse</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>4</ParameterControlType><StartKeyframe>-91445760000000000,{reverse},0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound><ParameterID>3</ParameterID></VideoComponentParam>"
    )
}

/// The matte track of [`with_matte_track`]: `VideoClipTrack` `track-2` with
/// `Track/ID` 7 at `Index` 1, as `horror_title` names its matte track by an
/// ID that is not `Index` + 1, holding `VideoClipTrackItem:93`: the 5 s
/// source of `one-clip.xml` over the same range as its clip, with its own
/// default chain 94.
pub(crate) const MATTE_TRACK: &str = "<VideoClipTrack ObjectUID=\"track-2\"><ClipTrack><Track><ID>7</ID><Index>1</Index></Track><ClipItems><Index>1</Index><TrackItems><TrackItem ObjectRef=\"93\"/></TrackItems></ClipItems></ClipTrack></VideoClipTrack>\
<VideoClipTrackItem ObjectID=\"93\"><ClipTrackItem><ComponentOwner><Components ObjectRef=\"94\"/></ComponentOwner><TrackItem><End>1270080000000</End></TrackItem><SubClip ObjectRef=\"5\"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio></VideoClipTrackItem>\
<VideoComponentChain ObjectID=\"94\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>";

/// `base`, `one-clip.xml` or a variant whose first clip keeps chain
/// `VideoComponentChain:4`, with [`MATTE_TRACK`] above that clip and
/// `components` appended to the chain after its own components, each at the
/// next `Index`. Each entry is the ObjectID of a component and its records.
pub(crate) fn with_matte_track(base: &str, components: &[(u32, String)]) -> String {
    let open = "<VideoComponentChain ObjectID=\"4\">";
    let close = "</VideoComponentChain>";
    let start = base.find(open).expect("the first clip's chain");
    let end = start + base[start..].find(close).expect("the chain end") + close.len();
    let chain = &base[start..end];
    let existing = chain.matches("<Component Index=").count();
    let references: String = components
        .iter()
        .enumerate()
        .map(|(offset, (id, _))| {
            format!(
                "<Component Index=\"{}\" ObjectRef=\"{id}\"/>",
                existing + offset
            )
        })
        .collect();
    let chain = if chain.contains("<ComponentChain/>") {
        chain.replace(
            "<ComponentChain/>",
            &format!("<ComponentChain><Components>{references}</Components></ComponentChain>"),
        )
    } else {
        chain.replace("</Components>", &format!("{references}</Components>"))
    };
    let records: String = components
        .iter()
        .map(|(_, records)| records.as_str())
        .collect();
    format!("{}{chain}{}", &base[..start], &base[end..])
        .replace(
            "</Tracks>",
            "<Track Index=\"1\" ObjectURef=\"track-2\"/></Tracks>",
        )
        .replace(
            "</PremiereData>",
            &format!("{MATTE_TRACK}{records}</PremiereData>"),
        )
}
