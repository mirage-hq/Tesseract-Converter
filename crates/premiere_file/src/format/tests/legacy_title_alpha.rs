//! Single-face title coverage controls built in code, not an Adobe oracle.

use super::*;

fn plain_title(edit: impl FnOnce(&str) -> String, luma: bool) -> String {
    let xml = legacy_title_xml_with(|title| {
        let palette = (1..=4).map(|index| format!(r#"<ColorSpec index="{index}"><red>12</red><green>34</green><blue>56</blue><xpar>0</xpar></ColorSpec>"#)).collect::<String>();
        let title = title
            .replace("<Layout>", "<Layout><LayoutEffectInfo><EffectType>0</EffectType><Indic>false</Indic><Ligatures>false</Ligatures><HindiDigits>false</HindiDigits></LayoutEffectInfo><Background><On>false</On></Background>")
            .replace("</LayoutDimension>", "<pSCREENAR>1</pSCREENAR></LayoutDimension>")
            .replace("</TypeSpec>", "<txKern>0</txKern><baselineShift>0</baselineShift><txSlant>0</txSlant><fiBold>0</fiBold><fiItalic>0</fiItalic><txSCapsOn>false</txSCapsOn><txUnderline>false</txUnderline></TypeSpec>")
            .replace("<ShaderList>", "<StyleBase><type>50000</type><personality>0</personality><faceDistortX>0</faceDistortX><faceDistortY>0</faceDistortY></StyleBase><FragmentList><Fragment><fragmentOff>false</fragmentOff><size>0</size><offset>0</offset><angle>0</angle><eFragmentType>0</eFragmentType><ghost>false</ghost><isExtendedShadowFragment>false</isExtendedShadowFragment><placeHolder>false</placeHolder><painterMix>15 15 15 15 15 15 15 15 15 15 15 15 15 15 15 15</painterMix></Fragment></FragmentList><ShaderList>")
            .replace("<Shader>", "<Shader><colorOption>4</colorOption><shaderOn>true</shaderOn><textureRef>0</textureRef><bevelSize>0</bevelSize><sheenOn>false</sheenOn>")
            .replace("</Shader>", &format!("{palette}</Shader>"))
            .replace("<Layer><TextPage><TextChain>", "<Layer><DrawPage/><TextPage><TextChain><ChainProperty><wordWrap>false</wordWrap></ChainProperty>")
            .replace("<TextLine>", "<TextLine objectID=\"1\">")
            .replace("<Alignment>", "<EnclosingObjectType>block</EnclosingObjectType><RTL>false</RTL><Alignment>")
            .replace("<CharacterAttributes TextRef=", "<CharacterAttributes RunCount=\"17\" TXKerning=\"0\" TXPostKerning=\"0\" BaselineShifting=\"0\" TextRef=")
            .replace("</TextPage></Layer>", "</TextPage><MergeGroups><Group><punchThru>false</punchThru><opacity>1</opacity><ObjectID value=\"1\"/></Group></MergeGroups></Layer>");
        edit(&title)
    });
    // Composite Using 0/Reverse false is the Alpha control in the pinned G1
    // native Track Matte fixture; 1 selects Luma and must not use this profile.
    let key = super::super::animation::animation_fixture::track_matte_key_xml(
        3000,
        2,
        u32::from(luma),
        false,
    );
    let sibling = format!(
        r#"<VideoClipTrackItem ObjectID="900"><ClipTrackItem><ComponentOwner><Components ObjectRef="901"/></ComponentOwner><TrackItem><Start>{}</Start><End>{}</End></TrackItem><SubClip ObjectRef="5"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio></VideoClipTrackItem><VideoComponentChain ObjectID="901"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>"#,
        5 * TICKS,
        10 * TICKS
    );
    xml.replace(r#"<TrackItem ObjectRef="3"/>"#, r#"<TrackItem ObjectRef="3"/><TrackItem ObjectRef="900"/>"#)
    .replace(
        "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>",
        "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"3000\"/></Components></ComponentChain></VideoComponentChain>",
    ).replace("</PremiereData>", &format!("{key}{sibling}</PremiereData>"))
}

fn convert(xml: &str) -> (serde_json::Value, Vec<crate::Omission>) {
    let (project, mut omissions) = inspect_project_with_omissions(xml, None).unwrap();
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
}

fn layers(value: &serde_json::Value) -> Vec<&serde_json::Value> {
    fn visit<'a>(value: &'a serde_json::Value, result: &mut Vec<&'a serde_json::Value>) {
        if value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .is_some()
        {
            result.push(value);
        }
        match value {
            serde_json::Value::Array(items) => items.iter().for_each(|item| visit(item, result)),
            serde_json::Value::Object(fields) => {
                fields.values().for_each(|item| visit(item, result))
            }
            _ => {}
        }
    }
    let mut result = Vec::new();
    visit(value, &mut result);
    result
}

#[test]
fn opaque_single_face_legacy_title_retains_alpha_consumer_and_editable_text() {
    let (document, omissions) = convert(&plain_title(str::to_owned, false));
    let layers = layers(&document);
    let video = layers
        .iter()
        .find(|layer| layer["type"] == "Video" && !layer["trackMatte"].is_null())
        .expect("keyed picture retained");
    let text = layers
        .iter()
        .find(|layer| layer["type"] == "Text")
        .expect("editable title retained");
    assert_eq!(video["trackMatte"]["mode"], "alpha", "{omissions:?}");
    assert_eq!(video["trackMatte"]["layer"], text["id"]);
    assert_eq!(text["sourceText"]["text"], "Editable & local");
    assert_eq!(text["sourceText"]["fontSize"], 80.0);
    assert_eq!(
        text["transform"]["position"],
        serde_json::json!([300.0, 400.0])
    );
    assert_eq!(
        text["transform"]["scale"],
        serde_json::json!([150.0, 100.0])
    );
}

#[test]
fn incomplete_legacy_title_coverage_keeps_consumers_concealed() {
    for (before, after) in [
        ("<offset>0</offset>", "<offset>10</offset>"),
        ("<txSlant>0</txSlant>", "<txSlant>10</txSlant>"),
        ("<wordWrap>false</wordWrap>", "<wordWrap>true</wordWrap>"),
        (
            "<verticalText>false</verticalText>",
            "<verticalText>true</verticalText>",
        ),
        ("<xpar>0</xpar>", "<xpar>127</xpar>"),
        (
            "<punchThru>false</punchThru>",
            "<punchThru>true</punchThru>",
        ),
        (
            "<txUnderline>false</txUnderline>",
            "<txUnderline>true</txUnderline>",
        ),
        (
            "<txSlant>0</txSlant>",
            "<txSlant>0</txSlant><txSlant>10</txSlant>",
        ),
        (
            "</TypeSpec>",
            "<UnmappedAlphaControl>1</UnmappedAlphaControl></TypeSpec>",
        ),
        ("<textureRef>0</textureRef>", "<textureRef>12</textureRef>"),
        ("<ObjectID value=\"1\"/>", "<ObjectID value=\"2\"/>"),
        ("RunCount=\"17\"", "RunCount=\"1\""),
        (
            "Editable &amp; local</TRString>",
            "Editable &amp; local&#10;Next</TRString>",
        ),
        (
            "<colorOption>4</colorOption>",
            "<colorOption>99</colorOption>",
        ),
        (
            r#"<ColorSpec index="4"><red>12</red><green>34</green><blue>56</blue><xpar>0</xpar></ColorSpec>"#,
            "",
        ),
        (
            r#"<ColorSpec index="4"><red>12</red><green>34</green><blue>56</blue><xpar>0</xpar></ColorSpec>"#,
            r#"<ColorSpec index="4"><red>12</red><green>34</green><blue>56</blue><xpar>127</xpar></ColorSpec>"#,
        ),
    ] {
        let xml = plain_title(
            |title| {
                assert!(title.contains(before));
                title.replace(before, after)
            },
            false,
        );
        let (document, _) = convert(&xml);
        assert!(
            !layers(&document)
                .iter()
                .any(|layer| layer["type"] == "Text" || !layer["trackMatte"].is_null()),
            "{before}"
        );
        assert_eq!(
            layers(&document)
                .iter()
                .filter(|layer| layer["type"] == "Video")
                .count(),
            1
        );
    }
    let (document, _) = convert(&plain_title(str::to_owned, true));
    assert!(!layers(&document)
        .iter()
        .any(|layer| layer["type"] == "Text" || !layer["trackMatte"].is_null()));
    assert_eq!(
        layers(&document)
            .iter()
            .filter(|layer| layer["type"] == "Video")
            .count(),
        1
    );
}
