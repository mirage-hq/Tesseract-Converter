//! Explicit inverse colour bindings for freshly generated importer DTOs.
//!
//! Callers must admit the native source domain first. This is not a persisted
//! document rewrite: unknown fields, metadata, geometry and scripts stay opaque.

use crate::HdrColorBridge;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn color_fields(kind: Option<&str>) -> &'static [&'static str] {
    match kind {
        Some(
            "dropShadow" | "outerGlow" | "innerGlow" | "innerShadow" | "stroke" | "strokeOutline"
            | "satin" | "colorOverlay" | "fill",
        ) => &["color"],
        Some("bevelEmboss") => &["highlightColor", "shadowColor"],
        Some("tint") => &["blackColor", "whiteColor"],
        Some("tritone") => &["blackColor", "whiteColor", "midColor"],
        Some("ramp") => &["startColor", "endColor"],
        Some("glow") => &["colorA", "colorB"],
        _ => &[],
    }
}

/// A converter-only visitor for declared colour fields of generated FX DTOs.
pub struct GeneratedHdrColorBinding<'a> {
    bridge: HdrColorBridge,
    diagnostics: &'a mut Vec<String>,
    effects: BTreeMap<String, BTreeSet<String>>,
    items: BTreeMap<String, BTreeSet<String>>,
}

impl<'a> GeneratedHdrColorBinding<'a> {
    /// Bind admitted native points into default-white FX meaning.
    pub fn new(transfer: crate::HdrTransfer, diagnostics: &'a mut Vec<String>) -> Self {
        Self {
            bridge: HdrColorBridge::new(transfer, None).expect("default HDR white is valid"),
            diagnostics,
            effects: BTreeMap::new(),
            items: BTreeMap::new(),
        }
    }

    /// Bind admitted native points into FX meaning whose 1.0 is `white` cd/m², the value of
    /// the `hdrGraphicsWhiteNits` the caller writes on the same document, so
    /// a native graphics-white point stays FX graphics white.
    pub fn with_graphics_white(
        transfer: crate::HdrTransfer,
        white: f64,
        diagnostics: &'a mut Vec<String>,
    ) -> Result<Self, crate::HdrColorError> {
        Ok(Self {
            bridge: HdrColorBridge::new(transfer, Some(white))?,
            diagnostics,
            effects: BTreeMap::new(),
            items: BTreeMap::new(),
        })
    }

    /// Also admit native code values above 1.0 (float working-space
    /// super-white); see [`HdrColorBridge::with_super_white_headroom`].
    pub fn with_super_white_headroom(mut self) -> Self {
        self.bridge = self.bridge.with_super_white_headroom();
        self
    }

    fn color(&mut self, value: &mut Value, scope: &str) -> bool {
        let Some(values) = value.as_array() else {
            return false;
        };
        let Some(color) = values
            .iter()
            .map(Value::as_f64)
            .collect::<Option<Vec<_>>>()
            .and_then(|values| <[f64; 4]>::try_from(values).ok())
        else {
            return false;
        };
        let mapped = match self.bridge.to_fx_gamut_mapped(color) {
            Ok((mapped, approximated)) => {
                if color[..3].iter().any(|v| *v > 1.0) {
                    self.diagnostics.push(format!(
                        "{scope}: native super-white code values {:?} lie above the transfer's peak code 1.0, where the curve ends; imported as light continuing linearly past the peak (code c is c times the peak), which re-exports the same codes. FX renders above the peak are unmeasured",
                        &color[..3]
                    ));
                }
                if approximated {
                    self.diagnostics.push(format!(
                        "{scope}: native BT.2020 point outside nonnegative editable BT.709 gamut; chroma reduced toward the achromatic axis preserving linear BT.709 luminance and hue direction; above-white headroom and coverage retained, saturation fidelity approximated"
                    ));
                }
                mapped
            }
            Err(error) => {
                self.diagnostics.push(
                    format!(
                        "{scope}: {error}; unsupported RGB replaced by black, coverage alpha and convertible siblings retained"
                    ),
                );
                [0.0, 0.0, 0.0, color[3]]
            }
        };
        *value = serde_json::json!(mapped);
        true
    }

    fn fields(&mut self, value: &mut Value, fields: &[&str], scope: &str) {
        for field in fields {
            if let Some(color) = value.get_mut(*field) {
                self.color(color, &format!("{scope}/{field}"));
            }
        }
    }

    fn paint(&mut self, value: &mut Value, scope: &str) {
        match value["type"].as_str() {
            Some("solid") => self.fields(value, &["color"], scope),
            Some("gradient") => {
                self.stops(value, scope);
                self.diagnostics.push(
                    format!(
                        "{scope}: gradient stops decoded independently; encoded-native and extended-sRGB interpolation differ, so interval fidelity remains an approximation"
                    ),
                );
            }
            _ => {}
        }
    }

    fn stops(&mut self, value: &mut Value, scope: &str) {
        if let Some(stops) = value.get_mut("stops").and_then(Value::as_array_mut) {
            for (index, stop) in stops.iter_mut().enumerate() {
                self.fields(stop, &["color"], &format!("{scope}/stop {index}"));
            }
        }
    }

    fn text(&mut self, value: &mut Value, scope: &str) {
        self.fields(
            value,
            &["fillColor", "strokeColor", "backgroundColor"],
            scope,
        );
    }

    /// Decode declared fields of one newly generated layer and its children.
    pub fn layer(&mut self, value: &mut Value) {
        let scope = format!("editable layer {} ({})", value["id"], value["name"]);
        match value["type"].as_str() {
            Some("Rect") => {
                if let Some(rect) = value.get_mut("rect") {
                    self.fields(rect, &["fillColor", "strokeColor"], &scope);
                    if let Some(paint) = rect.get_mut("fillPaint") {
                        self.paint(paint, &scope);
                    }
                }
            }
            Some("Shape") => {
                if let Some(shape) = value.get_mut("shape") {
                    self.shape(shape, &scope);
                }
            }
            Some("BooleanOperation" | "Group") => self.shape(value, &scope),
            Some("Text") => {
                if let Some(text) = value.get_mut("sourceText") {
                    self.text(text, &scope);
                }
                if let Some(animators) = value.get_mut("animators").and_then(Value::as_array_mut) {
                    for animator in animators {
                        Self::register(
                            &mut self.items,
                            animator,
                            &["fillColor", "strokeColor", "backgroundColor"],
                        );
                        self.text(animator, &scope);
                    }
                }
            }
            _ => {}
        }
        if let Some(shadow) = value.get_mut("dropShadow") {
            self.fields(shadow, &["color"], &scope);
        }
        if let Some(styles) = value.get_mut("layerStyles").and_then(Value::as_array_mut) {
            for style in styles {
                let fields = color_fields(style["style"]["type"].as_str());
                Self::register(&mut self.items, style, fields);
                if let Some(payload) = style.get_mut("style") {
                    self.style(payload, &scope);
                }
            }
        }
        if let Some(effects) = value.get_mut("effects").and_then(Value::as_array_mut) {
            for effect in effects {
                let kind = effect.get("effect").unwrap_or(effect)["type"].as_str();
                Self::register(&mut self.effects, effect, color_fields(kind));
                if let Some(payload) = effect.get_mut("effect") {
                    self.style(payload, &scope);
                } else {
                    self.style(effect, &scope);
                }
            }
        }
        if let Some(layers) = value.get_mut("layers").and_then(Value::as_array_mut) {
            for layer in layers {
                self.layer(layer);
            }
        }
    }

    fn register(registry: &mut BTreeMap<String, BTreeSet<String>>, owner: &Value, fields: &[&str]) {
        if let Some(id) = owner.get("id").filter(|id| !id.is_null()) {
            match registry.entry(id.to_string()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(fields.iter().map(|field| (*field).to_owned()).collect());
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().clear();
                } // Ambiguous ownership is never admitted.
            }
        }
    }

    fn shape(&mut self, value: &mut Value, scope: &str) {
        for field in ["fills", "strokes"] {
            if let Some(styles) = value.get_mut(field).and_then(Value::as_array_mut) {
                for style in styles {
                    if let Some(paint) = style.get_mut("paint") {
                        self.paint(paint, scope);
                    }
                }
            }
        }
    }

    fn triple(&mut self, value: &mut Value, fields: [&str; 3], scope: &str) {
        let Some(rgb) = fields
            .iter()
            .map(|field| value.get(*field)?.as_f64())
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };
        let mut color = serde_json::json!([rgb[0], rgb[1], rgb[2], 1.0]);
        self.color(&mut color, &format!("{scope}/{fields:?}"));
        for (index, field) in fields.into_iter().enumerate() {
            value[field] = color[index].clone();
        }
    }

    fn style(&mut self, value: &mut Value, scope: &str) {
        // These are canonical colour-bearing DTO variants constructed by this
        // importer. Unknown effect params/custom shader uniforms stay opaque.
        match value["type"].as_str() {
            Some(
                "dropShadow" | "outerGlow" | "innerGlow" | "innerShadow" | "stroke"
                | "strokeOutline" | "satin" | "bevelEmboss" | "colorOverlay" | "gradientOverlay"
                | "fill" | "tint" | "tritone" | "ramp" | "glow",
            ) => {
                self.fields(
                    value,
                    &[
                        "color",
                        "highlightColor",
                        "shadowColor",
                        "blackColor",
                        "whiteColor",
                        "midColor",
                        "startColor",
                        "endColor",
                        "colorA",
                        "colorB",
                    ],
                    scope,
                );
                self.stops(value, scope);
            }
            Some("tintTritone") => {
                self.triple(value, ["blackR", "blackG", "blackB"], scope);
                self.triple(value, ["whiteR", "whiteG", "whiteB"], scope);
            }
            Some("gradientRamp") => {
                self.triple(value, ["startR", "startG", "startB"], scope);
                self.triple(value, ["endR", "endG", "endB"], scope);
            }
            _ => {
                self.diagnostics.push(
                    format!(
                        "{scope}: effect/style {:?} has no admitted inverse colour-field binding; native effect domain/opaque parameters retained as an explicit approximation",
                        value["type"]
                    ),
                );
            }
        }
    }

    fn tagged(&mut self, value: &mut Value, scope: &str) -> bool {
        if value["type"].as_str() == Some("color") {
            return value
                .get_mut("value")
                .is_some_and(|color| self.color(color, scope));
        }
        false
    }

    /// Bind an importer-generated graph entry, retaining unsupported channel keys.
    pub fn entry(&mut self, entry: &mut Value, scope: &str) {
        let target = &entry["target"];
        let split_rgb = target["kind"].as_str() == Some("effectProperty")
            && matches!(
                target["paramName"].as_str(),
                Some(
                    "blackR"
                        | "blackG"
                        | "blackB"
                        | "whiteR"
                        | "whiteG"
                        | "whiteB"
                        | "startR"
                        | "startG"
                        | "startB"
                        | "endR"
                        | "endG"
                        | "endB"
                )
            );
        let source_text = target["kind"].as_str() == Some("layer")
            && target["propertyType"].as_str() == Some("sourceText");
        if split_rgb {
            self.diagnostics.push(format!("{scope}: split RGB effect-channel animator retained as native numeric approximation; synchronized triple-key inversion is unfinished, no colour endpoints decoded"));
        }
        let owned = match target["kind"].as_str() {
            Some("effectProperty") => {
                Self::owns(&self.effects, &target["effectId"], &target["paramName"])
            }
            Some("fxItemProperty") => {
                Self::owns(&self.items, &target["itemId"], &target["propertyName"])
            }
            Some("layer") => true,
            _ => false,
        };
        if let Some(animator) = entry.get_mut("animator") {
            if source_text {
                self.source_text_animator(animator, scope);
            } else if owned {
                self.animator(animator, scope);
            } else if Self::has_color(animator) {
                self.diagnostics.push(format!("{scope}: colour animator on an unadmitted effect/item owner or field remains opaque; native numeric approximation retained"));
            }
        }
    }

    fn owns(registry: &BTreeMap<String, BTreeSet<String>>, id: &Value, field: &Value) -> bool {
        field.as_str().is_some_and(|field| {
            registry
                .get(&id.to_string())
                .is_some_and(|fields| fields.contains(field))
        })
    }

    fn has_color(animator: &Value) -> bool {
        animator["value"]["type"] == "color"
            || animator["disabledValue"]["type"] == "color"
            || animator["keyframes"]
                .as_array()
                .is_some_and(|keys| keys.iter().any(|key| key["value"]["type"] == "color"))
    }

    fn source_text_value(&mut self, value: &mut Value, scope: &str) {
        if value.is_null() {
            return;
        }
        let text = (value["type"] == "string")
            .then(|| value["value"].as_str())
            .flatten();
        let Some(mut document) = text
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .filter(|document| {
                document.is_object()
                    && document["text"].is_string()
                    && document["fontFamily"].is_string()
                    && document["fontSize"].is_number()
            })
        else {
            self.diagnostics.push(format!("{scope}: unrecognized Source Text string payload retained opaque; embedded colour meaning remains a numeric approximation"));
            return;
        };
        self.text(&mut document, scope);
        value["value"] = Value::String(document.to_string());
    }

    fn source_text_animator(&mut self, animator: &mut Value, scope: &str) {
        match animator["type"].as_str() {
            Some("constant") => {
                if let Some(value) = animator.get_mut("value") {
                    self.source_text_value(value, scope);
                }
            }
            Some("keyframes") => {
                if let Some(keys) = animator.get_mut("keyframes").and_then(Value::as_array_mut) {
                    for key in keys {
                        if let Some(value) = key.get_mut("value") {
                            self.source_text_value(value, scope);
                        }
                    }
                }
                if let Some(value) = animator.get_mut("disabledValue") {
                    self.source_text_value(value, scope);
                }
            }
            _ => {} // Scripts remain opaque, including scripts that return text.
        }
    }

    /// Decode declared point endpoints of one newly generated animator.
    pub fn animator(&mut self, value: &mut Value, scope: &str) {
        match value["type"].as_str() {
            Some("constant") => {
                if let Some(value) = value.get_mut("value") {
                    self.tagged(value, scope);
                }
            }
            Some("keyframes") => {
                let mut decoded = false;
                if let Some(keys) = value.get_mut("keyframes").and_then(Value::as_array_mut) {
                    for key in keys {
                        if let Some(value) = key.get_mut("value") {
                            decoded |= self.tagged(value, scope);
                        }
                    }
                }
                if let Some(value) = value.get_mut("disabledValue") {
                    decoded |= self.tagged(value, scope);
                }
                if decoded {
                    self.diagnostics.push(
                    format!(
                        "{scope}: admitted key endpoints decoded without baking; non-Hold native encoded interpolation between decoded endpoints remains unproved"
                    ),
                );
                }
            }
            _ => {} // JsScript code is neither interpreted nor rewritten.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HdrTransfer;
    use serde_json::json;

    #[test]
    fn hdr_graphics_white_binding_decodes_native_white_to_fx_white() {
        for transfer in [HdrTransfer::Pq, HdrTransfer::Hlg] {
            let native = HdrColorBridge::new(transfer, Some(203.0))
                .unwrap()
                .to_native([1.0, 1.0, 1.0, 1.0])
                .unwrap();
            let mut diagnostics = Vec::new();
            let mut binding =
                GeneratedHdrColorBinding::with_graphics_white(transfer, 203.0, &mut diagnostics)
                    .unwrap();
            let mut layer =
                json!({"type":"Rect","id":1,"name":"white","rect":{"fillColor":native}});
            binding.layer(&mut layer);
            let actual: [f64; 4] =
                serde_json::from_value(layer["rect"]["fillColor"].clone()).unwrap();
            for channel in actual {
                assert!((channel - 1.0).abs() < 1e-10, "{actual:?}");
            }
            // The default 609 binding reads the same native point as dimmer than FX white.
            let mut default =
                json!({"type":"Rect","id":1,"name":"white","rect":{"fillColor":native}});
            GeneratedHdrColorBinding::new(transfer, &mut Vec::new()).layer(&mut default);
            assert!(default["rect"]["fillColor"][0].as_f64().unwrap() < 0.9);
            for invalid in [99.0, 1001.0, f64::NAN] {
                assert!(GeneratedHdrColorBinding::with_graphics_white(
                    transfer,
                    invalid,
                    &mut Vec::new()
                )
                .is_err());
            }
        }
    }

    #[test]
    fn hdr_effect_colour_keys_require_known_owner_and_field() {
        let bridge = HdrColorBridge::new(HdrTransfer::Pq, None).unwrap();
        let native = bridge.to_native([0.8, 0.4, 0.2, 0.7]).unwrap();
        let mut diagnostics = Vec::new();
        let mut binding = GeneratedHdrColorBinding::new(HdrTransfer::Pq, &mut diagnostics);
        binding.layer(&mut json!({"type":"Group","id":1,"effects":[
            {"id":5,"effect":{"type":"fill","color":native}},
            {"id":6,"effect":{"type":"futureEffect","color":native}}
        ]}));
        for (id, param, admitted) in [
            (5, "color", true),
            (6, "color", false),
            (5, "opaqueColor", false),
        ] {
            let mut entry = json!({"target":{"kind":"effectProperty","effectId":id,"paramName":param},
                "animator":{"type":"constant","value":{"type":"color","value":native}}});
            let before = entry.clone();
            binding.entry(&mut entry, "effect");
            if admitted {
                let actual: [f64; 4] =
                    serde_json::from_value(entry["animator"]["value"]["value"].clone()).unwrap();
                for (actual, expected) in actual.into_iter().zip([0.8, 0.4, 0.2, 0.7]) {
                    assert!((actual - expected).abs() < 1e-10);
                }
            } else {
                assert_eq!(entry, before);
            }
        }
        let mut unknown_target = json!({"target":{"kind":"futureTarget"},
            "animator":{"type":"constant","value":{"type":"color","value":native}}});
        let before = unknown_target.clone();
        binding.entry(&mut unknown_target, "unknown target");
        assert_eq!(unknown_target, before);
        assert!(diagnostics
            .iter()
            .any(|warning| warning.contains("unadmitted effect/item owner")));
    }

    #[test]
    fn hdr_source_text_strings_decode_declared_points_without_touching_opaque_fields_or_scripts() {
        let bridge = HdrColorBridge::new(HdrTransfer::Hlg, None).unwrap();
        let native = bridge.to_native([0.8, 0.4, 0.2, 0.7]).unwrap();
        let text = json!({"text":"Caption","fontFamily":"Inter","fontSize":24,"fillColor":native,
            "metadata":{"color":native},"privateCode":"return [1,0,0,1]"});
        let mut entry = json!({"target":{"kind":"layer","layerId":1,"propertyType":"sourceText"},
            "animator":{"type":"keyframes","keyframes":[{"value":{"type":"string","value":text.to_string()}}],
                "disabledValue":{"type":"string","value":text.to_string()}}});
        let mut diagnostics = Vec::new();
        let mut binding = GeneratedHdrColorBinding::new(HdrTransfer::Hlg, &mut diagnostics);
        binding.entry(&mut entry, "source text");
        for value in [
            &entry["animator"]["keyframes"][0]["value"],
            &entry["animator"]["disabledValue"],
        ] {
            let decoded: Value = serde_json::from_str(value["value"].as_str().unwrap()).unwrap();
            assert_eq!(decoded["metadata"], text["metadata"]);
            assert_eq!(decoded["privateCode"], text["privateCode"]);
            let actual: [f64; 4] = serde_json::from_value(decoded["fillColor"].clone()).unwrap();
            for (actual, expected) in actual.into_iter().zip([0.8, 0.4, 0.2, 0.7]) {
                assert!((actual - expected).abs() < 1e-10);
            }
        }
        entry["animator"] = json!({"type":"jsScript","layerTimeJsCode":"return sourceText"});
        let before = entry.clone();
        binding.entry(&mut entry, "script");
        assert_eq!(entry, before);
        entry["animator"] =
            json!({"type":"constant","value":{"type":"string","value":"not canonical text JSON"}});
        let before = entry.clone();
        binding.entry(&mut entry, "opaque");
        assert_eq!(entry, before);
        assert!(diagnostics
            .iter()
            .any(|warning| warning.contains("unrecognized Source Text")));
        assert!(!diagnostics
            .iter()
            .any(|warning| warning.contains("interpolation")));
    }

    #[test]
    fn hdr_numeric_and_split_rgb_keys_do_not_claim_decoded_colour_endpoints() {
        let mut warnings = Vec::new();
        let mut binding = GeneratedHdrColorBinding::new(HdrTransfer::Pq, &mut warnings);
        let mut entry = json!({"target":{"kind":"effectProperty","effectId":5,"paramName":"blackR"},
            "animator":{"type":"keyframes","keyframes":[{"value":{"type":"float","value":0.5}}]}});
        let before = entry.clone();
        binding.entry(&mut entry, "effect channel");
        assert_eq!(entry, before);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("inversion is unfinished"));
        assert!(!warnings[0].contains("admitted key endpoints"));
        let mut warnings = Vec::new();
        let mut binding = GeneratedHdrColorBinding::new(HdrTransfer::Pq, &mut warnings);
        binding.animator(&mut entry["animator"], "ordinary float");
        assert!(warnings.is_empty());
    }

    #[test]
    fn hdr_import_only_visits_declared_points_and_preserves_coverage_siblings_and_scripts() {
        for transfer in [HdrTransfer::Pq, HdrTransfer::Hlg] {
            let bridge = HdrColorBridge::new(transfer, None).unwrap();
            let native = bridge.to_native([0.8, 0.4, 0.2, 0.35]).unwrap();
            let mut diagnostics = Vec::new();
            let mut binding = GeneratedHdrColorBinding::new(transfer, &mut diagnostics);
            let mut layer = json!({"type":"Group", "id":1, "name":"scope", "layers":[
                {"type":"Rect", "id":2, "name":"supported", "rect":{"fillColor":native,"size":[4,3]}},
                {"type":"Rect", "id":3, "name":"out of gamut", "rect":{"fillColor":[1,0,0,0.4]}}
            ], "fills":[{"paint":{"type":"solid","color":native}}], "opaque":{"color":native}, "effects":[{"effect":{"type":"futureEffect","color":native}}]});
            let opaque = layer["opaque"].clone();
            let effects = layer["effects"].clone();
            binding.layer(&mut layer);
            let actual: [f64; 4] =
                serde_json::from_value(layer["layers"][0]["rect"]["fillColor"].clone()).unwrap();
            for (a, b) in actual.into_iter().zip([0.8, 0.4, 0.2, 0.35]) {
                assert!((a - b).abs() < 1e-10);
            }
            let mapped: [f64; 4] =
                serde_json::from_value(layer["layers"][1]["rect"]["fillColor"].clone()).unwrap();
            assert!(mapped[..3].iter().all(|v| *v >= 0.0));
            assert!(mapped[..3].iter().any(|v| *v > 0.0));
            assert_eq!(mapped[3], 0.4);
            assert_eq!(layer["layers"][0]["rect"]["size"], json!([4, 3]));
            let fill: [f64; 4] =
                serde_json::from_value(layer["fills"][0]["paint"]["color"].clone()).unwrap();
            for (actual, expected) in fill.into_iter().zip([0.8, 0.4, 0.2, 0.35]) {
                assert!((actual - expected).abs() < 1e-10);
            }
            assert_eq!(layer["opaque"], opaque);
            assert_eq!(layer["effects"], effects);
            let mut script = json!({"type":"jsScript","layerTimeJsCode":"return [1,0,0,1]"});
            let original = script.clone();
            binding.animator(&mut script, "opaque script");
            assert_eq!(script, original);
            assert!(!layer["layers"][0]
                .as_object()
                .unwrap()
                .contains_key("layerStyles"));
            assert!(diagnostics
                .iter()
                .any(|warning| warning.contains("chroma reduced toward the achromatic axis")));
        }
    }
}
