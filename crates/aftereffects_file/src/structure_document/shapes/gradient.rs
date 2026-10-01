//! AE `GCst/GCky` gradient-stop decoding.

use fx_schema::layer::{ShapeGradientStop, ShapeGradientType, ShapePaint};

use crate::{properties::unique_list, rifx::Chunk};

#[derive(Clone, Copy, Debug)]
struct ColorStop {
    offset: f64,
    midpoint: f64,
    color: [f64; 3],
}

#[derive(Clone, Copy, Debug)]
struct AlphaStop {
    offset: f64,
    midpoint: f64,
    alpha: f64,
}

#[derive(Debug)]
pub(crate) struct DecodedGradient {
    pub(crate) paint: ShapePaint,
    pub(crate) animated: bool,
    pub(crate) warnings: Vec<String>,
}

pub(super) fn decode_or_native_default(
    run: Option<&[Chunk]>,
    gradient_type: i64,
    start: [f64; 2],
    end: [f64; 2],
) -> Result<DecodedGradient, String> {
    run.map_or_else(
        || Ok(ae_default(gradient_type, start, end)),
        |run| decode(run, gradient_type, start, end),
    )
}

pub(crate) fn decode(
    run: &[Chunk],
    gradient_type: i64,
    start: [f64; 2],
    end: [f64; 2],
) -> Result<DecodedGradient, String> {
    let wrapper =
        unique_list(run, *b"GCst").map_err(|error| format!("gradient Colors ignored: {error}"))?;
    let values = unique_list(wrapper, *b"GCky")
        .map_err(|error| format!("gradient Colors ignored: {error}"))?;
    let xml_values: Vec<_> = values
        .iter()
        .filter(|chunk| chunk.id() == *b"Utf8")
        .map(|chunk| {
            chunk
                .data_payload()
                .ok_or_else(|| "gradient Utf8 value is not a data chunk".to_owned())
        })
        .collect::<Result<_, _>>()?;
    let first = xml_values
        .first()
        .ok_or_else(|| "gradient has no GCky Utf8 value".to_owned())?;
    let text =
        std::str::from_utf8(first).map_err(|_| "gradient XML is not valid UTF-8".to_owned())?;
    let (colors, alphas) = parse_stops(text)?;
    let mut warnings = Vec::new();
    if colors.iter().any(|stop| stop.midpoint != 0.5)
        || alphas.iter().any(|stop| stop.midpoint != 0.5)
    {
        warnings.push("AE gradient stop midpoint bias has no persisted FX equivalent; stop positions/colors were retained with linear interpolation".into());
    }
    let stops = merged_stops(&colors, &alphas)?;
    let gradient_type = decode_gradient_type(gradient_type, &mut warnings);
    Ok(DecodedGradient {
        paint: ShapePaint::Gradient {
            gradient_type,
            start,
            end,
            stops,
        },
        animated: xml_values.len() > 1,
        warnings,
    })
}

pub(crate) fn ae_default(gradient_type: i64, start: [f64; 2], end: [f64; 2]) -> DecodedGradient {
    let mut warnings = vec!["AE omitted its default gradient Colors payload; opaque white-to-black stops were reconstructed".into()];
    let gradient_type = decode_gradient_type(gradient_type, &mut warnings);
    DecodedGradient {
        paint: ShapePaint::Gradient {
            gradient_type,
            start,
            end,
            stops: vec![
                ShapeGradientStop {
                    offset: 0.0,
                    color: [1.0, 1.0, 1.0, 1.0],
                },
                ShapeGradientStop {
                    offset: 1.0,
                    color: [0.0, 0.0, 0.0, 1.0],
                },
            ],
        },
        animated: false,
        warnings,
    }
}

fn decode_gradient_type(value: i64, warnings: &mut Vec<String>) -> ShapeGradientType {
    match value {
        1 => ShapeGradientType::Linear,
        2 => ShapeGradientType::Radial,
        other => {
            warnings.push(format!(
                "AE gradient type {other} is unknown; Linear was used"
            ));
            ShapeGradientType::Linear
        }
    }
}

fn parse_stops(xml: &str) -> Result<(Vec<ColorStop>, Vec<AlphaStop>), String> {
    if !xml.contains("<prop.map") || !xml.contains("Gradient Color Data") {
        return Err("gradient XML has no Gradient Color Data map".into());
    }
    let mut color_stops = Vec::new();
    let mut alpha_stops = Vec::new();
    let mut list_keys: Vec<Option<String>> = Vec::new();
    let mut pending_key = None;
    let mut position = 0;
    while let Some((tag, body_start, body_end, next)) = next_tag(xml, position)? {
        position = next;
        match tag {
            "key" => {
                let close = xml[body_end..]
                    .find("</key>")
                    .map(|offset| body_end + offset)
                    .ok_or_else(|| "gradient XML has an unterminated key".to_owned())?;
                pending_key = Some(xml[body_start..close].trim().to_owned());
                position = close + "</key>".len();
            }
            "prop.list" => list_keys.push(pending_key.take()),
            "/prop.list" => {
                list_keys
                    .pop()
                    .ok_or_else(|| "gradient XML has an unmatched prop.list".to_owned())?;
            }
            "array" => {
                let close = xml[body_end..]
                    .find("</array>")
                    .map(|offset| body_end + offset)
                    .ok_or_else(|| "gradient XML has an unterminated array".to_owned())?;
                let array_key = pending_key.take();
                let stop_index = list_keys
                    .iter()
                    .rev()
                    .flatten()
                    .find_map(|key| key.strip_prefix("Stop-")?.parse::<usize>().ok());
                if stop_index.is_some() {
                    let values = parse_float_array(&xml[body_end..close])?;
                    match array_key.as_deref() {
                        Some("Stops Color") if values.len() >= 5 => color_stops.push(ColorStop {
                            offset: values[0],
                            midpoint: values[1],
                            color: [values[2], values[3], values[4]],
                        }),
                        Some("Stops Alpha") if values.len() >= 3 => alpha_stops.push(AlphaStop {
                            offset: values[0],
                            midpoint: values[1],
                            alpha: values[2],
                        }),
                        Some("Stops Color" | "Stops Alpha") => {
                            return Err("gradient stop array is truncated".into());
                        }
                        _ => {}
                    }
                }
                position = close + "</array>".len();
            }
            _ => {}
        }
    }
    if !list_keys.is_empty() {
        return Err("gradient XML has an unterminated prop.list".into());
    }
    validate_color_stops(&mut color_stops)?;
    validate_alpha_stops(&mut alpha_stops)?;
    Ok((color_stops, alpha_stops))
}

fn next_tag(xml: &str, from: usize) -> Result<Option<(&str, usize, usize, usize)>, String> {
    let Some(relative_start) = xml[from..].find('<') else {
        return Ok(None);
    };
    let start = from + relative_start;
    let end = xml[start + 1..]
        .find('>')
        .map(|offset| start + 1 + offset)
        .ok_or_else(|| "gradient XML has an unterminated tag".to_owned())?;
    let raw = xml[start + 1..end].trim();
    let tag = raw.split_ascii_whitespace().next().unwrap_or_default();
    Ok(Some((tag, end + 1, end + 1, end + 1)))
}

fn parse_float_array(xml: &str) -> Result<Vec<f64>, String> {
    let mut values = Vec::new();
    let mut position = 0;
    while let Some(start_offset) = xml[position..].find("<float>") {
        let start = position + start_offset + "<float>".len();
        let end = xml[start..]
            .find("</float>")
            .map(|offset| start + offset)
            .ok_or_else(|| "gradient XML has an unterminated float".to_owned())?;
        let value = xml[start..end]
            .trim()
            .parse::<f64>()
            .map_err(|_| "gradient XML contains a non-numeric float".to_owned())?;
        if !value.is_finite() {
            return Err("gradient XML contains a non-finite float".into());
        }
        values.push(value);
        position = end + "</float>".len();
    }
    Ok(values)
}

fn validate_color_stops(stops: &mut [ColorStop]) -> Result<(), String> {
    if stops.len() < 2 {
        return Err("gradient has fewer than two color stops".into());
    }
    if stops.iter().any(|stop| {
        !(0.0..=1.0).contains(&stop.offset)
            || !(0.0..=1.0).contains(&stop.midpoint)
            || stop.color.iter().any(|value| !(0.0..=1.0).contains(value))
    }) {
        return Err("gradient color stop is outside the persisted 0..=1 range".into());
    }
    stops.sort_by(|left, right| left.offset.total_cmp(&right.offset));
    Ok(())
}

fn validate_alpha_stops(stops: &mut [AlphaStop]) -> Result<(), String> {
    if stops.is_empty() {
        return Ok(());
    }
    if stops.iter().any(|stop| {
        !(0.0..=1.0).contains(&stop.offset)
            || !(0.0..=1.0).contains(&stop.midpoint)
            || !(0.0..=1.0).contains(&stop.alpha)
    }) {
        return Err("gradient alpha stop is outside the persisted 0..=1 range".into());
    }
    stops.sort_by(|left, right| left.offset.total_cmp(&right.offset));
    Ok(())
}

fn merged_stops(
    colors: &[ColorStop],
    alphas: &[AlphaStop],
) -> Result<Vec<ShapeGradientStop>, String> {
    let mut offsets: Vec<_> = colors
        .iter()
        .map(|stop| stop.offset)
        .chain(alphas.iter().map(|stop| stop.offset))
        .collect();
    offsets.sort_by(f64::total_cmp);
    offsets.dedup_by(|left, right| *left == *right);
    let stops: Vec<_> = offsets
        .into_iter()
        .map(|offset| {
            let color = interpolate_color(colors, offset);
            let alpha = if alphas.is_empty() {
                1.0
            } else {
                interpolate_alpha(alphas, offset)
            };
            ShapeGradientStop {
                offset,
                color: [color[0], color[1], color[2], alpha],
            }
        })
        .collect();
    (stops.len() >= 2)
        .then_some(stops)
        .ok_or_else(|| "gradient has fewer than two merged stops".to_owned())
}

fn interpolate_color(stops: &[ColorStop], offset: f64) -> [f64; 3] {
    interpolate(stops, offset, |stop| stop.offset, |stop| stop.color)
}

fn interpolate_alpha(stops: &[AlphaStop], offset: f64) -> f64 {
    interpolate(stops, offset, |stop| stop.offset, |stop| stop.alpha)
}

fn interpolate<T, V, O, G>(stops: &[T], offset: f64, get_offset: O, get_value: G) -> V
where
    V: InterpolateValue,
    O: Fn(&T) -> f64,
    G: Fn(&T) -> V,
{
    if offset <= get_offset(&stops[0]) {
        return get_value(&stops[0]);
    }
    if offset >= get_offset(&stops[stops.len() - 1]) {
        return get_value(&stops[stops.len() - 1]);
    }
    // First stop at or above the offset preserves the previous interpolation
    // semantics for duplicate offsets without rescanning the entire gradient.
    let upper = stops.partition_point(|stop| get_offset(stop) < offset);
    let pair = &stops[upper - 1..=upper];
    let start = get_offset(&pair[0]);
    let end = get_offset(&pair[1]);
    let amount = if end == start {
        1.0
    } else {
        (offset - start) / (end - start)
    };
    get_value(&pair[0]).interpolate(get_value(&pair[1]), amount)
}

trait InterpolateValue: Copy {
    fn interpolate(self, end: Self, amount: f64) -> Self;
}

impl InterpolateValue for f64 {
    fn interpolate(self, end: Self, amount: f64) -> Self {
        self + (end - self) * amount
    }
}

impl InterpolateValue for [f64; 3] {
    fn interpolate(self, end: Self, amount: f64) -> Self {
        std::array::from_fn(|index| self[index].interpolate(end[index], amount))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        properties::{root_runs, runs, unique_list},
        structure::{ItemKind, read_project},
    };

    fn gradients(bytes: &[u8]) -> Vec<DecodedGradient> {
        let project = read_project(bytes).expect("pinned native gradient fixture parses");
        fn collect(group: &[Chunk], output: &mut Vec<DecodedGradient>) {
            for (name, run) in runs(group).expect("vector entries decode") {
                if name == "ADBE Vector Grad Colors" {
                    output.push(
                        decode(run, 1, [0.0, 0.0], [100.0, 0.0]).expect("native gradient decodes"),
                    );
                } else if matches!(
                    name,
                    "ADBE Vector Group"
                        | "ADBE Vectors Group"
                        | "ADBE Vector Graphic - G-Fill"
                        | "ADBE Vector Graphic - G-Stroke"
                ) {
                    collect(
                        unique_list(run, *b"tdgp").expect("native vector group"),
                        output,
                    );
                }
            }
        }
        let mut output = Vec::new();
        for item in &project.items {
            let ItemKind::Composition(composition) = &item.kind else {
                continue;
            };
            for layer in &composition.layers {
                for (name, run) in root_runs(&layer.content).expect("property root decodes") {
                    if name == "ADBE Root Vectors Group" {
                        collect(
                            unique_list(run, *b"tdgp").expect("vector root decodes"),
                            &mut output,
                        );
                    }
                }
            }
        }
        output
    }

    #[test]
    fn interpolation_search_is_logarithmic_and_preserves_duplicate_offsets() {
        let stops: Vec<_> = (0..4096).map(|index| f64::from(index) / 4095.0).collect();
        let probes = std::cell::Cell::new(0);
        let value = interpolate(
            &stops,
            0.75,
            |stop| {
                probes.set(probes.get() + 1);
                *stop
            },
            |stop| *stop,
        );
        assert_eq!(value, 0.75);
        assert!(probes.get() < 32, "{} offset probes", probes.get());
        let duplicate = [(0.0, 0.0), (0.5, 0.2), (0.5, 0.8), (1.0, 1.0)];
        assert_eq!(
            interpolate(&duplicate, 0.5, |stop| stop.0, |stop| stop.1),
            0.2
        );
        assert_eq!(
            interpolate(&duplicate, 0.75, |stop| stop.0, |stop| stop.1),
            0.9
        );
    }

    #[test]
    fn native_gradient_xml_decodes_color_and_alpha_stops() {
        let mut gradients = gradients(include_bytes!(
            "../../../tests/fixtures/shapes/gradient.aep"
        ));
        assert_eq!(gradients.len(), 2);
        let decoded = gradients.remove(0);
        let ShapePaint::Gradient { stops, .. } = decoded.paint else {
            panic!("native paint must remain a gradient")
        };
        assert!(stops.len() >= 2);
        assert_eq!(stops.first().map(|stop| stop.offset), Some(0.0));
        assert_eq!(stops.last().map(|stop| stop.offset), Some(1.0));
        assert!(stops.iter().all(|stop| stop.color[3] == 1.0));
        assert!(!decoded.animated);
    }

    #[test]
    fn absent_gradient_uses_the_native_default() {
        let decoded = decode_or_native_default(None, 1, [0.0, 0.0], [100.0, 0.0])
            .expect("an absent Colors property has a native default");
        let ShapePaint::Gradient { stops, .. } = decoded.paint else {
            panic!("native default must remain an editable gradient")
        };
        assert_eq!(
            stops,
            [
                ShapeGradientStop {
                    offset: 0.0,
                    color: [1.0, 1.0, 1.0, 1.0],
                },
                ShapeGradientStop {
                    offset: 1.0,
                    color: [0.0, 0.0, 0.0, 1.0],
                },
            ]
        );
        assert!(
            decoded
                .warnings
                .iter()
                .any(|warning| warning.contains("omitted its default gradient Colors payload"))
        );
    }

    #[test]
    fn malformed_present_gradient_does_not_use_the_absent_property_default() {
        let malformed = [Chunk::data(*b"GCst", vec![0]).unwrap()];
        let error = decode_or_native_default(Some(&malformed), 1, [0.0, 0.0], [100.0, 0.0])
            .expect_err("a present malformed Colors property must not use the absent default");
        assert!(error.contains("gradient Colors ignored"), "{error}");
    }

    #[test]
    fn native_animated_gradient_is_detected_without_flattening_claim() {
        let gradients = gradients(include_bytes!(
            "../../../tests/fixtures/shapes/gradient_animated.aep"
        ));
        assert!(!gradients.is_empty());
        assert!(gradients.iter().any(|gradient| gradient.animated));
    }
}
