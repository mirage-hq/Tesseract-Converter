//! Alpha-only admission for a single, opaque, undecorated legacy text line.

use super::{child, number, value, Definitions};
use crate::error::{ensure, Result};
use roxmltree::Node;

pub(super) fn plain_alpha(layout: Node<'_, '_>, definitions: &Definitions<'_, '_>) -> bool {
    check(layout, definitions).is_ok()
}

fn zeros(node: Node<'_, '_>, names: &[&str]) -> Result<()> {
    for name in names {
        ensure!(number(node, name)? == 0.0, "nonzero legacy {name}");
    }
    Ok(())
}

fn falses(node: Node<'_, '_>, names: &[&str]) -> Result<()> {
    for name in names {
        ensure!(value(node, name)? == "false", "active legacy {name}");
    }
    Ok(())
}

fn only_children(node: Node<'_, '_>, names: &[&str]) -> Result<()> {
    // Stored controls are singular except the indexed colour palette.
    let mut seen = std::collections::HashSet::new();
    for child in node.children().filter(Node::is_element) {
        let tag = child.tag_name().name();
        ensure!(names.contains(&tag), "unknown legacy title content");
        ensure!(
            tag == "ColorSpec" || seen.insert(tag),
            "duplicate legacy title field"
        );
    }
    Ok(())
}

fn check(layout: Node<'_, '_>, definitions: &Definitions<'_, '_>) -> Result<()> {
    only_children(
        layout,
        &[
            "LayoutEffectInfo",
            "LayoutDimension",
            "LayoutAttributes",
            "Background",
            "DefaultStyle",
            "DefaultTextDescription",
            "GraphicObjectDefaults",
            "TextChainDefaults",
            "TextDescriptions",
            "Styles",
            "Shaders",
            "Textures",
            "Logos",
            "Layers",
            "VLS",
        ],
    )?;
    for (tag, key, count) in [
        (
            "TextDescription",
            "Reference",
            definitions.descriptions.len(),
        ),
        ("Style", "ID", definitions.styles.len()),
    ] {
        ensure!(
            layout
                .descendants()
                .filter(|node| node.has_tag_name(tag) && node.attribute(key).is_some())
                .count()
                == count,
            "duplicate legacy title definitions"
        );
    }
    ensure!(
        layout
            .descendants()
            .filter(|node| node.has_tag_name("Shader") && child(*node, "cReference").is_ok())
            .count()
            == definitions.shaders.len(),
        "duplicate legacy shader definitions"
    );
    ensure!(
        layout
            .descendants()
            .filter(|node| node.has_tag_name("TextLine"))
            .count()
            == 1,
        "legacy title lines"
    );
    let effect = child(layout, "LayoutEffectInfo")?;
    zeros(effect, &["EffectType"])?;
    falses(effect, &["Indic", "Ligatures", "HindiDigits"])?;
    ensure!(
        number(child(layout, "LayoutDimension")?, "pSCREENAR")? == 1.0,
        "nonsquare legacy title pixels"
    );
    falses(child(layout, "Background")?, &["On"])?;
    let layers = child(layout, "Layers")?;
    let layers: Vec<_> = layers.children().filter(Node::is_element).collect();
    ensure!(
        layers.len() == 1 && layers[0].has_tag_name("Layer"),
        "legacy title layers"
    );
    let layer = layers[0];
    only_children(layer, &["DrawPage", "TextPage", "MergeGroups"])?;
    ensure!(
        !child(layer, "DrawPage")?
            .children()
            .any(|node| node.is_element()),
        "legacy title drawings"
    );
    let page = child(layer, "TextPage")?;
    let chains: Vec<_> = page.children().filter(Node::is_element).collect();
    ensure!(
        chains.len() == 1 && chains[0].has_tag_name("TextChain"),
        "legacy title chains"
    );
    let chain = chains[0];
    only_children(chain, &["ChainProperty", "ChainTabs", "TextLine"])?;
    let chain_properties = child(chain, "ChainProperty")?;
    only_children(
        chain_properties,
        &[
            "wordWrap",
            "Position",
            "Size",
            "leading",
            "lockedLinesX",
            "lockedLinesY",
            "boxCanGrow",
            "tabModeStyle",
            "implicitTabSpacing",
            "implicitTabType",
        ],
    )?;
    falses(chain_properties, &["wordWrap"])?;
    let lines: Vec<_> = chain
        .children()
        .filter(|node| node.has_tag_name("TextLine"))
        .collect();
    ensure!(lines.len() == 1, "legacy title lines");
    let line = lines[0];
    only_children(
        line,
        &[
            "BaseProperties",
            "EnclosingObjectType",
            "Alignment",
            "RTL",
            "TRString",
            "RunLengthEncodedCharacterAttributes",
            "tagName",
        ],
    )?;
    ensure!(
        value(line, "EnclosingObjectType")? == "block",
        "legacy paragraph title"
    );
    falses(line, &["RTL"])?;
    let text = value(line, "TRString")?;
    ensure!(!text.chars().any(|character| character.is_control() || matches!(character, '\u{2028}' | '\u{2029}')), "legacy text layout controls");
    only_children(
        child(line, "BaseProperties")?,
        &["txBase", "XPos", "angle", "verticalText", "objectLeading"],
    )?;
    falses(child(line, "BaseProperties")?, &["verticalText"])?;
    let attributes = child(line, "RunLengthEncodedCharacterAttributes")?;
    let runs: Vec<_> = attributes.children().filter(Node::is_element).collect();
    ensure!(
        runs.len() == 1 && runs[0].has_tag_name("CharacterAttributes"),
        "mixed legacy runs"
    );
    let run = runs[0];
    let run_count = run
        .attribute("RunCount")
        .and_then(|value| value.parse::<usize>().ok());
    ensure!(
        run_count.is_some_and(|count| count >= text.encode_utf16().count()),
        "incomplete legacy character coverage"
    );
    ensure!(
        run.attributes().all(|attribute| [
            "RunCount",
            "TextRef",
            "StyleRef",
            "TXKerning",
            "TXPostKerning",
            "BaselineShifting"
        ]
        .contains(&attribute.name())),
        "unknown legacy character attribute"
    );
    for name in ["TXKerning", "TXPostKerning", "BaselineShifting"] {
        ensure!(run.attribute(name) == Some("0"), "legacy run {name}");
    }
    let description = definitions
        .descriptions
        .get(run.attribute("TextRef").unwrap_or(""))
        .ok_or_else(|| crate::error::unsupported("legacy text reference"))?;
    let spec = child(*description, "TypeSpec")?;
    only_children(
        spec,
        &[
            "size",
            "txHeight",
            "txKern",
            "baselineShift",
            "leading",
            "txSCaps",
            "txSCapsOn",
            "txSlant",
            "txUnderline",
            "txWidth",
            "linked",
            "fiBold",
            "fiItalic",
            "fifullName",
            "fifontFamilyName",
            "fifontStyle",
            "fifontType",
            "ficategory",
        ],
    )?;
    zeros(
        spec,
        &["txKern", "baselineShift", "txSlant", "fiBold", "fiItalic"],
    )?;
    falses(spec, &["txSCapsOn", "txUnderline"])?;
    let style = definitions
        .styles
        .get(run.attribute("StyleRef").unwrap_or(""))
        .ok_or_else(|| crate::error::unsupported("legacy style reference"))?;
    only_children(*style, &["StyleBase", "FragmentList", "ShaderList"])?;
    let base = child(*style, "StyleBase")?;
    only_children(
        base,
        &[
            "type",
            "positionDominance",
            "lineGradient",
            "styleRef",
            "faceDistortX",
            "faceDistortY",
            "shadow_softness",
            "personality",
            "linked",
            "EmbellishmentSizeRule",
            "PainterRampType",
        ],
    )?;
    zeros(base, &["faceDistortX", "faceDistortY", "personality"])?;
    ensure!(number(base, "type")? == 50000.0, "legacy style type");
    let fragments = child(*style, "FragmentList")?;
    ensure!(
        fragments
            .children()
            .filter(Node::is_element)
            .all(|node| node.has_tag_name("Fragment")),
        "unknown legacy fragment"
    );
    let active: Vec<_> = fragments
        .children()
        .filter(|node| node.is_element())
        .filter(|node| value(*node, "fragmentOff").ok() != Some("true"))
        .collect();
    ensure!(
        active.len() == 1 && active[0].has_tag_name("Fragment"),
        "legacy fragments"
    );
    let fragment = active[0];
    only_children(
        fragment,
        &[
            "size",
            "offset",
            "angle",
            "ghost",
            "isExtendedShadowFragment",
            "eFragmentType",
            "fragmentOff",
            "placeHolder",
            "annotation",
            "placeHolderShaderIndex",
            "painterMix",
        ],
    )?;
    zeros(fragment, &["size", "offset", "angle", "eFragmentType"])?;
    falses(
        fragment,
        &[
            "fragmentOff",
            "ghost",
            "isExtendedShadowFragment",
            "placeHolder",
        ],
    )?;
    let painters: Vec<_> = value(fragment, "painterMix")?.split_whitespace().collect();
    ensure!(
        painters.len() == 16 && painters.iter().all(|painter| *painter == "15"),
        "legacy fragment painters"
    );
    let shader_refs: Vec<_> = child(*style, "ShaderList")?
        .children()
        .filter(|node| {
            node.has_tag_name("ShaderRef") && node.attribute("PainterNumber") == Some("15")
        })
        .collect();
    ensure!(shader_refs.len() == 1, "ambiguous legacy face painter");
    let shader_ref = shader_refs[0];
    let shader = definitions
        .shaders
        .get(value(shader_ref, "shaderRef")?)
        .ok_or_else(|| crate::error::unsupported("legacy shader reference"))?;
    only_children(
        *shader,
        &[
            "cReference",
            "textureRef",
            "colorOption",
            "shaderOn",
            "glintSize",
            "glintOffset",
            "rampPosTop",
            "rampPosBottom",
            "rampAngle",
            "bevelBalance",
            "rampCycle",
            "classicStyle",
            "rampType",
            "ColorSpec",
            "glintAngle",
            "bevelSize",
            "bevelDirection",
            "bevelPipe",
            "bevelAngle",
            "bevelShape",
            "bevelShining",
            "bevelLight",
            "bevelMerge",
            "sheenOn",
        ],
    )?;
    ensure!(
        value(*shader, "shaderOn")? == "true",
        "disabled legacy face"
    );
    zeros(*shader, &["bevelSize"])?;
    falses(*shader, &["sheenOn"])?;
    ensure!(
        value(*shader, "colorOption")? == "4",
        "unknown legacy face palette"
    );
    let colors: Vec<_> = shader
        .children()
        .filter(|node| node.has_tag_name("ColorSpec"))
        .collect();
    ensure!(colors.len() == 5, "incomplete legacy face palette");
    for (color, index) in colors.iter().zip(["0", "1", "2", "3", "4"]) {
        ensure!(
            color.attribute("index") == Some(index),
            "ambiguous legacy face palette"
        );
        only_children(*color, &["red", "green", "blue", "xpar"])?;
        ensure!(number(*color, "xpar")? == 0.0, "nonopaque legacy ramp");
    }
    let texture_ref = value(*shader, "textureRef")?;
    if texture_ref != "0" {
        let textures: Vec<_> = child(layout, "Textures")?
            .children()
            .filter(|node| {
                node.has_tag_name("Texture") && value(*node, "textureRef").ok() == Some(texture_ref)
            })
            .collect();
        ensure!(textures.len() == 1, "ambiguous legacy texture reference");
        ensure!(
            textures[0]
                .children()
                .filter(|node| node.has_tag_name("textureOn"))
                .count()
                == 1,
            "ambiguous legacy texture enable"
        );
        falses(textures[0], &["textureOn"])?;
    }
    let groups: Vec<_> = child(layer, "MergeGroups")?
        .children()
        .filter(Node::is_element)
        .collect();
    ensure!(
        groups.len() == 1 && groups[0].has_tag_name("Group"),
        "legacy merge groups"
    );
    let group = groups[0];
    only_children(group, &["punchThru", "opacity", "ObjectID"])?;
    falses(group, &["punchThru"])?;
    ensure!(number(group, "opacity")? == 1.0, "legacy group opacity");
    let objects: Vec<_> = group
        .children()
        .filter(|node| node.has_tag_name("ObjectID"))
        .collect();
    ensure!(
        objects.len() == 1
            && line.attribute("objectID").is_some()
            && objects[0].attribute("value") == line.attribute("objectID"),
        "legacy group membership"
    );
    Ok(())
}
