//! Bounded import of the old Inscriber `TITL` generator, not a file-backed still.

use super::{required, PrTextTransform};
use crate::error::{ensure, unsupported, Result};
use crate::format::{Graph, Located};
use crate::schema::{
    native::Media,
    text::{PrGraphicObject, PrJustification, PrRgb, PrTextDocument, PrTextFrame, PrVerticalAlign},
    PrText,
};
use crate::{approximate, omit, Omission, OmissionScope};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::io::Read;

mod coverage;

const LIMIT: usize = 16 * 1024 * 1024;

pub(super) struct TitleObjects {
    pub(super) objects: Vec<PrGraphicObject>,
    pub(super) plain_alpha: bool,
}

pub(super) fn is_title(media: &Media) -> bool {
    media.file_path.as_deref() == Some("1414091852") && media.relative_paths.is_empty()
}

pub(super) fn objects(
    graph: &Graph<'_>,
    media: &Located<Media>,
    frame: [u32; 2],
    omissions: &mut Vec<Omission>,
) -> Result<TitleObjects> {
    let prefs = required(
        media.value.importer_prefs.as_ref(),
        &media.identity,
        "ImporterPrefs",
    )?;
    ensure!(
        prefs.encoding == "base64",
        "{}: title preferences must be base64",
        media.identity
    );
    let encoded = if prefs.value.is_empty() {
        graph
            .binary_value(&prefs.binary_hash, &media.identity)?
            .ok_or_else(|| unsupported("missing title BinaryHash definition"))?
    } else {
        &prefs.value
    };
    let xml = decode(encoded)?;
    let document = roxmltree::Document::parse(&xml)
        .map_err(|error| unsupported(format!("invalid legacy title XML: {error}")))?;
    let layout = document
        .descendants()
        .find(|node| node.has_tag_name("Layout"))
        .ok_or_else(|| unsupported("legacy title has no Layout"))?;
    let dimensions = child(layout, "LayoutDimension")?;
    ensure!(
        number(dimensions, "pXPIXELS")? == f64::from(frame[0])
            && number(dimensions, "pYLINES")? == f64::from(frame[1]),
        "legacy title canvas differs from its sequence"
    );
    // Bound repeated reference lookups even for titles with many text lines.
    let definitions = Definitions::new(layout);
    let mut objects = Vec::new();
    for (index, line) in layout
        .descendants()
        .filter(|node| node.has_tag_name("TextLine"))
        .enumerate()
    {
        match text_line(&definitions, line, index) {
            Ok(text) => objects.push(PrGraphicObject::Text(text)),
            Err(error) => omit(
                omissions,
                OmissionScope::Feature,
                &media.identity,
                format!("legacy title line {index} omitted: {error}; independent lines retained"),
            ),
        }
    }
    ensure!(
        !objects.is_empty(),
        "{}: legacy title has no convertible text lines",
        media.identity
    );
    approximate(omissions, &media.identity, "legacy TITL text imported as editable point text: stored line baselines, font, character dimensions, angle and first-run face colour retained; mixed character runs use the first style, tracking/kerning/baseline shifts, legacy fragment switches/painters/strokes/shadows/gradients/textures, background/drawing objects, merge groups, roll/crawl and paragraph layout are not reproduced; native typography and RGB/alpha fidelity unverified");
    let plain_alpha = coverage::plain_alpha(layout, &definitions);
    Ok(TitleObjects {
        objects,
        plain_alpha,
    })
}

fn decode(encoded: &str) -> Result<String> {
    ensure!(
        encoded.len() <= LIMIT * 2,
        "legacy title preferences exceed size limit"
    );
    let encoded: Vec<_> = encoded
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|error| unsupported(format!("invalid title base64: {error}")))?;
    // Native preferences have a 16-byte header followed by this NUL-terminated
    // field name. Do not scan arbitrary binary bytes for a zlib signature.
    let compressed = bytes
        .get(16..)
        .and_then(|bytes| bytes.strip_prefix(b"CompressedTitle\0"))
        .ok_or_else(|| unsupported("missing CompressedTitle field"))?;
    let mut xml = Vec::new();
    flate2::read::ZlibDecoder::new(compressed)
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut xml)
        .map_err(|error| unsupported(format!("invalid compressed title: {error}")))?;
    ensure!(
        xml.len() <= LIMIT && xml.len() % 2 == 0,
        "legacy title XML exceeds limit or has odd UTF-16 length"
    );
    let big_endian = xml.starts_with(&[0xfe, 0xff]);
    let units: Vec<_> = xml
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            if big_endian {
                u16::from_be_bytes([pair[0], pair[1]])
            } else {
                u16::from_le_bytes([pair[0], pair[1]])
            }
        })
        .collect();
    let text = String::from_utf16(&units)
        .map_err(|error| unsupported(format!("invalid title UTF-16: {error}")))?;
    Ok(text.trim_start_matches('\u{feff}').to_owned())
}

fn child<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    tag: &str,
) -> Result<roxmltree::Node<'a, 'input>> {
    node.children()
        .find(|node| node.has_tag_name(tag))
        .ok_or_else(|| unsupported(format!("legacy title missing {tag}")))
}

fn value<'a>(node: roxmltree::Node<'a, '_>, tag: &str) -> Result<&'a str> {
    child(node, tag)?
        .text()
        .ok_or_else(|| unsupported(format!("legacy title empty {tag}")))
}

fn number(node: roxmltree::Node<'_, '_>, tag: &str) -> Result<f64> {
    let number: f64 = value(node, tag)?
        .parse()
        .map_err(|_| unsupported(format!("legacy title invalid {tag}")))?;
    ensure!(number.is_finite(), "legacy title nonfinite {tag}");
    Ok(number)
}

#[derive(Default)]
struct Definitions<'a, 'input> {
    descriptions: std::collections::HashMap<&'a str, roxmltree::Node<'a, 'input>>,
    styles: std::collections::HashMap<&'a str, roxmltree::Node<'a, 'input>>,
    shaders: std::collections::HashMap<&'a str, roxmltree::Node<'a, 'input>>,
}

impl<'a, 'input> Definitions<'a, 'input> {
    fn new(layout: roxmltree::Node<'a, 'input>) -> Self {
        let mut definitions = Self::default();
        for node in layout.descendants() {
            let entry = if node.has_tag_name("TextDescription") {
                node.attribute("Reference")
                    .map(|key| (&mut definitions.descriptions, key))
            } else if node.has_tag_name("Style") {
                node.attribute("ID")
                    .map(|key| (&mut definitions.styles, key))
            } else if node.has_tag_name("Shader") {
                value(node, "cReference")
                    .ok()
                    .map(|key| (&mut definitions.shaders, key))
            } else {
                None
            };
            if let Some((map, key)) = entry {
                map.entry(key).or_insert(node);
            }
        }
        definitions
    }
}

fn text_line(
    definitions: &Definitions<'_, '_>,
    line: roxmltree::Node<'_, '_>,
    index: usize,
) -> Result<PrText> {
    let run = line
        .descendants()
        .find(|node| node.has_tag_name("CharacterAttributes"))
        .ok_or_else(|| unsupported("legacy title has no character attributes"))?;
    let text_ref = run
        .attribute("TextRef")
        .ok_or_else(|| unsupported("missing title TextRef"))?;
    let style_ref = run
        .attribute("StyleRef")
        .ok_or_else(|| unsupported("missing title StyleRef"))?;
    let description = definitions
        .descriptions
        .get(text_ref)
        .copied()
        .ok_or_else(|| unsupported("unresolved title TextRef"))?;
    let spec = child(description, "TypeSpec")?;
    let base = child(line, "BaseProperties")?;
    ensure!(
        value(base, "verticalText")? == "false",
        "vertical legacy text is not mapped"
    );
    let height = number(spec, "txHeight")?;
    let width = number(spec, "txWidth")?;
    ensure!(
        height > 0.0 && height <= 10000.0 && width > 0.0 && width <= 10000.0,
        "legacy character dimensions out of range"
    );
    let style = definitions
        .styles
        .get(style_ref)
        .ok_or_else(|| unsupported("unresolved title StyleRef"))?;
    let shader_ref = style
        .descendants()
        .find(|node| {
            node.has_tag_name("ShaderRef") && node.attribute("PainterNumber") == Some("15")
        })
        .ok_or_else(|| unsupported("missing title face painter"))?;
    let shader_id = value(shader_ref, "shaderRef")?;
    let shader = definitions
        .shaders
        .get(shader_id)
        .ok_or_else(|| unsupported("unresolved title shader"))?;
    let color = shader
        .children()
        .find(|node| node.has_tag_name("ColorSpec") && node.attribute("index") == Some("0"))
        .ok_or_else(|| unsupported("missing title face colour"))?;
    let channel = |name| -> Result<u8> {
        let value = number(color, name)?;
        ensure!(
            (0.0..=255.0).contains(&value),
            "legacy title colour out of range"
        );
        Ok(value.round() as u8)
    };
    let transparency = number(color, "xpar")?;
    ensure!(
        (0.0..=255.0).contains(&transparency),
        "legacy title transparency out of range"
    );
    let fill = if shader
        .children()
        .any(|node| node.has_tag_name("shaderOn") && node.text() == Some("false"))
    {
        None
    } else {
        Some(PrRgb([
            channel("red")?,
            channel("green")?,
            channel("blue")?,
        ]))
    };
    let justification = match value(line, "Alignment")? {
        "left" => PrJustification::Left,
        "center" => PrJustification::Center,
        "right" => PrJustification::Right,
        _ => return Err(unsupported("legacy title alignment is unsupported")),
    };
    // txHeight/txWidth are the saved character dimensions in layout pixels;
    // TypeSpec/size is Inscriber's internal design size, not the UI font size.
    let text = PrText {
        mask: None,
        name: format!("Legacy title line {}", index + 1),
        document: PrTextDocument {
            text: value(line, "TRString")?.to_owned(),
            font: value(spec, "fifullName")?.to_owned(),
            size: height as f32,
            fill,
            fill_spans: Vec::new(),
            stroke: None,
            shadow: None,
            all_caps: false,
            tracking: 0.0,
            leading: 0.0,
            justification,
            frame: PrTextFrame::Point {
                vertical: PrVerticalAlign::Top,
            },
            background: None,
        },
        transform: PrTextTransform {
            position: [number(base, "XPos")?, number(base, "txBase")?],
            anchor: [0.0; 2],
            scale: 100.0,
            rotation: number(base, "angle")?,
            opacity: (1.0 - transparency / 255.0) * 100.0,
        },
        horizontal_scale: Some(width / height * 100.0),
        animations: Vec::new(),
        source_text_keys: Vec::new(),
        mask_source: None,
    };
    text.validate()
        .map_err(|error| unsupported(format!("invalid legacy text: {error}")))?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn preferences(xml: &[u8]) -> String {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(xml).unwrap();
        let mut bytes = vec![0; 16];
        bytes.extend_from_slice(b"CompressedTitle\0");
        bytes.extend(encoder.finish().unwrap());
        STANDARD.encode(bytes)
    }

    #[test]
    fn title_definition_index_keeps_first_reference_and_skips_missing_keys() {
        let xml = r#"<Layout>
            <TextDescription/><TextDescription Reference="7"/>
            <TextDescription Reference="7"><duplicate/></TextDescription>
            <Style/><Style ID="9"/><Style ID="9"><duplicate/></Style>
            <Shader/><Shader><cReference>11</cReference></Shader>
            <Shader><cReference>11</cReference><duplicate/></Shader>
        </Layout>"#;
        let document = roxmltree::Document::parse(xml).unwrap();
        let definitions = Definitions::new(document.root_element());
        for (map, key) in [
            (&definitions.descriptions, "7"),
            (&definitions.styles, "9"),
            (&definitions.shaders, "11"),
        ] {
            assert_eq!(map.len(), 1);
            assert!(!map[key]
                .descendants()
                .any(|node| node.has_tag_name("duplicate")));
        }
    }

    #[test]
    fn title_decoder_handles_utf16_byte_orders_without_replacing_invalid_units() {
        let text = "<Title>Editable \u{1f680}</Title>";
        for big_endian in [false, true] {
            let mut bytes = if big_endian {
                vec![0xfe, 0xff]
            } else {
                vec![0xff, 0xfe]
            };
            bytes.extend(text.encode_utf16().flat_map(|unit| {
                if big_endian {
                    unit.to_be_bytes()
                } else {
                    unit.to_le_bytes()
                }
            }));
            assert_eq!(decode(&preferences(&bytes)).unwrap(), text);
        }
        assert!(decode(&preferences(&[0, 0xd8])).is_err());
        assert!(decode(&preferences(&[1])).is_err());
    }

    #[test]
    fn title_decoder_bounds_inflation_and_rejects_bad_framing() {
        assert!(decode(&preferences(&vec![0; LIMIT + 2]))
            .unwrap_err()
            .to_string()
            .contains("exceeds limit"));
        assert!(decode("not base64").is_err());
        assert!(decode(&STANDARD.encode(b"CompressedTitle\0")).is_err());
        let mut bytes = vec![0; 16];
        bytes.extend_from_slice(b"CompressedTitle\0invalid zlib");
        assert!(decode(&STANDARD.encode(bytes)).is_err());
    }
}
