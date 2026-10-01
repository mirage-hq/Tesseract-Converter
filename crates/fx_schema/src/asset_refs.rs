//! Structural enumeration of asset references in an FX document.
use crate::LayerData;

use crate::{
    effect::LayerEffect,
    layer::{Layer, MediaSourceKind},
    property::{PropType, PropertyValue},
    FXComposition, LayerId,
};

/// Kind of asset referenced by an fx_composition layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AssetKind {
    /// A video asset decoded through the renderer's frame resolver.
    Video,
    /// A still image asset decoded through the renderer's image resolver.
    Image,
    /// An audio asset: an [`crate::AudioLayer`] source, sampled for live gain
    /// by the `AudioGain{Left,Right,Both}` derived properties.
    Audio,
    /// A PAG asset rendered by an embedded AI Edit PAG sequence.
    Pag,
    /// A 3D `.cube` asset referenced by an Input or Look Transform.
    Lut,
}

/// One asset reference discovered during a composition walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AssetRef<'a> {
    pub kind: AssetKind,
    pub asset_id: &'a str,
}

/// One text-font reference discovered during a composition walk.
///
/// Mirrors the (family, style) split on `TextDocument` so callers can
/// preload bundled font faces ahead of the first frame. `Project::
/// required_resources` consumes these to feed `ResourceProvider::get_font`,
/// which in turn lands the bytes in the resource cache before the renderer
/// asks `cosmic-text` to shape any glyph. Without this, an fx_composition
/// whose root project carries no other font references will hit cosmic-text's
/// `no default font found` panic at the first text frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextFontRef<'a> {
    pub family: &'a str,
    pub style: &'a str,
}

impl FXComposition {
    /// Every asset reference reachable for **any** frame of this editable
    /// composition: each original source asset, each optional generated feature
    /// asset, and every statically enumerable [`PropType::MediaSourceAssetId`]
    /// (media, classified by the target layer's source kind) and
    /// [`PropType::AudioSourceAssetId`] (audio → [`AssetKind::Audio`]) value in
    /// the animation graph.
    ///
    /// `Project::required_resources` consumes this to preload assets ahead of
    /// `ResourceProvider::load_all_required_resources`. Because the graph
    /// gates both asset-id properties to finite, enumerable `String` animator
    /// ranges ([`crate::AnimationGraphError::UnboundedAnimator`] /
    /// [`crate::AnimationGraphError::NonEnumerableValue`]), the override values
    /// are statically known here — no frame can swap in an asset id this walk
    /// did not surface, so the preload set is exhaustive.
    ///
    /// Static audio assets backing an `AudioLayer` (also sampled by the
    /// `AudioGain*` derived properties) are part of the layer tree, so they
    /// are collected by the static walk too.
    ///
    /// Duplicates (a constant override equal to the static source, or two
    /// overrides with the same id) are emitted as-is; callers dedupe through
    /// their own seen-set, matching [`FXComposition::asset_refs`].
    #[must_use]
    pub fn asset_refs(&self) -> Vec<AssetRef<'_>> {
        let mut refs = Vec::new();
        collect_layer_asset_refs(self.layers(), &mut refs);
        collect_graph_asset_overrides(self, &mut refs);
        refs
    }

    /// Returns every statically reachable text font reference. Static
    /// text-layer font refs are unioned with any
    /// constant `FontFamily` / `FontStyle` overrides from the animation graph
    /// so resource preloading sees every reachable font pair.
    #[must_use]
    pub fn text_font_refs(&self) -> Vec<TextFontRef<'_>> {
        let mut refs = Vec::new();
        collect_text_font_refs(self.layers(), &mut refs);
        collect_graph_font_overrides(self, &mut refs);
        refs
    }

    /// Returns static text plus constant `TextContent` overrides.
    /// from the animation graph are included because their glyph coverage is
    /// known ahead of rendering. Unbounded script outputs cannot be enumerated
    /// and therefore retain the static source text as their preload signal.
    #[must_use]
    pub fn text_contents(&self) -> Vec<&str> {
        let mut contents = Vec::new();
        collect_text_contents(self.layers(), &mut contents);
        collect_graph_text_overrides(self, &mut contents);
        contents
    }
}

fn collect_layer_asset_refs<'a>(layers: &'a [Layer], refs: &mut Vec<AssetRef<'a>>) {
    for layer in layers {
        for instance in layer.effects() {
            let effect = match instance.data() {
                crate::EffectData::Identified { effect, .. }
                | crate::EffectData::Legacy(effect) => effect,
            };
            if let crate::EffectPayload::Known(LayerEffect::LookTransform { transform, .. }) =
                effect
            {
                collect_color_transform_asset_ref(transform, refs);
            }
        }
        match layer.data() {
            LayerData::Media(media) => {
                refs.push(AssetRef {
                    kind: media_source_asset_kind(media.source.kind),
                    asset_id: media.source.asset_id.as_str(),
                });
                if let Some(input) = media
                    .source
                    .input_transform
                    .as_ref()
                    .and_then(crate::PersistedInputTransform::as_supported)
                {
                    collect_color_transform_asset_ref(&input.transform, refs);
                }
            }
            LayerData::Video(video) => {
                if let Some(input) = video.source.input_transform() {
                    collect_color_transform_asset_ref(&input.transform, refs);
                }
                refs.push(AssetRef {
                    kind: AssetKind::Video,
                    asset_id: video.source.asset_id.as_str(),
                });
                if let Some(eye_contact) = &video.source.eye_contact {
                    refs.push(AssetRef {
                        kind: AssetKind::Video,
                        asset_id: eye_contact.eye_contact_asset_id.as_str(),
                    });
                }
                // The enhanced-audio companion only ever feeds the audio
                // scene, so it is an audio asset even though it hangs off a
                // video source.
                if let Some(enhancement) = &video.source.audio_enhancement {
                    refs.push(AssetRef {
                        kind: AssetKind::Audio,
                        asset_id: enhancement.enhanced_asset_id.as_str(),
                    });
                }
            }
            LayerData::Image(image) => {
                if let Some(input) = image.source.input_transform() {
                    collect_color_transform_asset_ref(&input.transform, refs);
                }
                let crate::ImageSource::Asset(source) = &image.source;
                refs.push(AssetRef {
                    kind: AssetKind::Image,
                    asset_id: source.asset_id.as_str(),
                });
            }
            LayerData::Audio(audio) => {
                refs.push(AssetRef {
                    kind: AssetKind::Audio,
                    asset_id: audio.source.asset_id.as_str(),
                });
                if let Some(enhancement) = &audio.source.enhancement {
                    refs.push(AssetRef {
                        kind: AssetKind::Audio,
                        asset_id: enhancement.enhanced_asset_id.as_str(),
                    });
                }
            }
            LayerData::Pag(pag) => collect_pag_asset_refs(pag, refs),
            LayerData::Group(group) => collect_layer_asset_refs(&group.layers, refs),
            LayerData::AiEdit(ai_edit) => {
                for sticker in &ai_edit.stickers {
                    if let Some(asset_id) = sticker.pag_asset_id() {
                        refs.push(AssetRef {
                            kind: AssetKind::Pag,
                            asset_id,
                        });
                    }
                }
                collect_layer_asset_refs(&ai_edit.layers, refs);
            }
            LayerData::BooleanOperation(group) => collect_layer_asset_refs(&group.layers, refs),
            LayerData::Text(_)
            | LayerData::Rect(_)
            | LayerData::Shape(_)
            | LayerData::Adjustment(_) => {}
        }
    }
}

fn collect_color_transform_asset_ref<'a>(
    transform: &'a crate::ColorTransform,
    refs: &mut Vec<AssetRef<'a>>,
) {
    let crate::ColorTransform::Lut3d { asset_id, .. } = transform;
    refs.push(AssetRef {
        kind: AssetKind::Lut,
        asset_id: asset_id.as_str(),
    });
}

fn collect_pag_asset_refs<'a>(sequence: &'a crate::PagLayer, refs: &mut Vec<AssetRef<'a>>) {
    refs.extend(sequence.items.iter().map(|item| AssetRef {
        kind: AssetKind::Pag,
        asset_id: item.asset_id.as_str(),
    }));
    for item in &sequence.items {
        for entry in &item.configuration {
            if let Some(image) = &entry.pag_layer_config.image {
                refs.push(AssetRef {
                    kind: AssetKind::Image,
                    asset_id: image.asset_id.as_str(),
                });
            }
            if let Some(video) = &entry.pag_layer_config.video {
                refs.push(AssetRef {
                    kind: AssetKind::Video,
                    asset_id: video.asset_id.as_str(),
                });
            }
        }
    }
    for insert in &sequence.image_inserts {
        if let Some(image) = &insert.image {
            refs.push(AssetRef {
                kind: AssetKind::Image,
                asset_id: image.asset_id.as_str(),
            });
        }
        if let Some(video) = &insert.video {
            refs.push(AssetRef {
                kind: AssetKind::Video,
                asset_id: video.asset_id.as_str(),
            });
        }
    }
}

/// Emit an [`AssetRef`] for every value in each finite asset-id animator range:
/// a [`PropType::MediaSourceAssetId`] override targeting a `MediaLayer` uses
/// that layer's media source kind, while a [`PropType::AudioSourceAssetId`]
/// override targeting an [`crate::AudioLayer`] uses [`AssetKind::Audio`]. The
/// kinds match the classification the static [`collect_layer_asset_refs`] walk
/// uses for each layer type.
///
/// Unbounded or non-`String` animators are unreachable here: the graph rejects
/// them at construction time for both finite-range properties
/// ([`crate::AnimationGraphError::UnboundedAnimator`] /
/// [`crate::AnimationGraphError::NonEnumerableValue`]), which is what makes
/// this walk exhaustive.
fn collect_graph_asset_overrides<'a>(composition: &'a FXComposition, refs: &mut Vec<AssetRef<'a>>) {
    for entry in composition.dynamics().entries() {
        // Only fixed-property targets carry an asset id; effect params never do.
        let Some(property) = entry.target.as_property() else {
            continue;
        };
        let kind = match property.property_type() {
            PropType::MediaSourceAssetId => {
                let Some(media_kind) =
                    find_media_source_kind(composition.layers(), property.layer_id())
                else {
                    continue;
                };
                media_source_asset_kind(media_kind)
            }
            PropType::AudioSourceAssetId => AssetKind::Audio,
            _ => continue,
        };
        if let Some(values) = entry.animator.finite_value_range() {
            refs.extend(values.into_iter().filter_map(|value| {
                let PropertyValue::String(asset_id) = value else {
                    return None;
                };
                Some(AssetRef {
                    kind,
                    asset_id: asset_id.as_str(),
                })
            }));
        }
    }
}

fn collect_graph_font_overrides<'a>(
    composition: &'a FXComposition,
    refs: &mut Vec<TextFontRef<'a>>,
) {
    collect_graph_font_overrides_for_layers(composition, composition.layers(), refs);
}

fn collect_graph_text_overrides<'a>(composition: &'a FXComposition, contents: &mut Vec<&'a str>) {
    collect_graph_text_overrides_for_layers(composition, composition.layers(), contents);
}

fn collect_graph_text_overrides_for_layers<'a>(
    composition: &'a FXComposition,
    layers: &'a [Layer],
    contents: &mut Vec<&'a str>,
) {
    for layer in layers {
        match layer.data() {
            LayerData::Text(text) => {
                if let Some(overrides) =
                    graph_string_overrides(composition, text.id, PropType::TextContent)
                {
                    contents.extend(overrides);
                }
            }
            LayerData::Group(group) => {
                collect_graph_text_overrides_for_layers(composition, &group.layers, contents);
            }
            LayerData::AiEdit(ai_edit) => {
                collect_graph_text_overrides_for_layers(composition, &ai_edit.layers, contents);
            }
            LayerData::BooleanOperation(group) => {
                collect_graph_text_overrides_for_layers(composition, &group.layers, contents);
            }
            LayerData::Media(_)
            | LayerData::Video(_)
            | LayerData::Image(_)
            | LayerData::Rect(_)
            | LayerData::Shape(_)
            | LayerData::Audio(_)
            | LayerData::Pag(_)
            | LayerData::Adjustment(_) => {}
        }
    }
}

/// Walk concrete text layers rather than raw graph entries: a font override
/// needs the layer's static counterpart when only family or style is overridden,
/// so orphan font overrides cannot form a complete preload pair and are ignored.
fn collect_graph_font_overrides_for_layers<'a>(
    composition: &'a FXComposition,
    layers: &'a [Layer],
    refs: &mut Vec<TextFontRef<'a>>,
) {
    for layer in layers {
        match layer.data() {
            LayerData::Text(text) => {
                let font_families =
                    graph_string_overrides(composition, text.id, PropType::FontFamily);
                let font_styles = graph_string_overrides(composition, text.id, PropType::FontStyle);
                if font_families.is_none() && font_styles.is_none() {
                    continue;
                }
                let font_families =
                    font_families.unwrap_or_else(|| vec![text.source_text.font_family.as_ref()]);
                let font_styles =
                    font_styles.unwrap_or_else(|| vec![text.source_text.font_style.as_ref()]);
                for &family in &font_families {
                    for &style in &font_styles {
                        refs.push(TextFontRef { family, style });
                    }
                }
            }
            LayerData::Group(group) => {
                collect_graph_font_overrides_for_layers(composition, &group.layers, refs);
            }
            LayerData::AiEdit(ai_edit) => {
                collect_graph_font_overrides_for_layers(composition, &ai_edit.layers, refs);
            }
            LayerData::BooleanOperation(group) => {
                collect_graph_font_overrides_for_layers(composition, &group.layers, refs);
            }
            LayerData::Media(_)
            | LayerData::Video(_)
            | LayerData::Image(_)
            | LayerData::Rect(_)
            | LayerData::Shape(_)
            | LayerData::Audio(_)
            | LayerData::Pag(_)
            | LayerData::Adjustment(_) => {}
        }
    }
}

fn graph_string_overrides(
    composition: &FXComposition,
    layer_id: LayerId,
    property_type: PropType,
) -> Option<Vec<&str>> {
    composition
        .dynamics()
        .entries()
        .iter()
        .find(|entry| {
            entry.target.as_property().is_some_and(|property| {
                property.layer_id() == layer_id && property.property_type() == property_type
            })
        })
        .and_then(|entry| entry.animator.finite_value_range())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| {
                    if let PropertyValue::String(value) = value {
                        Some(value.as_str())
                    } else {
                        None
                    }
                })
                .collect()
        })
}

fn media_source_asset_kind(kind: MediaSourceKind) -> AssetKind {
    match kind {
        MediaSourceKind::Video => AssetKind::Video,
        MediaSourceKind::Image => AssetKind::Image,
    }
}

/// Returns the [`MediaSourceKind`] of the media layer with `layer_id`, if any.
fn find_media_source_kind(layers: &[Layer], layer_id: LayerId) -> Option<MediaSourceKind> {
    for layer in layers {
        match layer.data() {
            LayerData::Media(media) if media.id == layer_id => return Some(media.source.kind),
            LayerData::Video(video) if video.id == layer_id => return Some(MediaSourceKind::Video),
            LayerData::Image(image) if image.id == layer_id => {
                return image.source.asset().map(|_| MediaSourceKind::Image);
            }
            LayerData::Group(group) => {
                if let Some(kind) = find_media_source_kind(&group.layers, layer_id) {
                    return Some(kind);
                }
            }
            LayerData::AiEdit(ai_edit) => {
                if let Some(kind) = find_media_source_kind(&ai_edit.layers, layer_id) {
                    return Some(kind);
                }
            }
            LayerData::BooleanOperation(group) => {
                if let Some(kind) = find_media_source_kind(&group.layers, layer_id) {
                    return Some(kind);
                }
            }
            LayerData::Text(_)
            | LayerData::Media(_)
            | LayerData::Video(_)
            | LayerData::Image(_)
            | LayerData::Rect(_)
            | LayerData::Shape(_)
            | LayerData::Audio(_)
            | LayerData::Pag(_)
            | LayerData::Adjustment(_) => {}
        }
    }
    None
}

fn collect_text_font_refs<'a>(layers: &'a [Layer], refs: &mut Vec<TextFontRef<'a>>) {
    for layer in layers {
        match layer.data() {
            LayerData::Text(text) => refs.push(TextFontRef {
                family: text.source_text.font_family.as_ref(),
                style: text.source_text.font_style.as_ref(),
            }),
            LayerData::Group(group) => collect_text_font_refs(&group.layers, refs),
            LayerData::Pag(pag) => {
                for item in &pag.items {
                    for entry in &item.configuration {
                        if let Some(text) = &entry.pag_layer_config.text {
                            if let Some(family) = text.font_family.as_deref() {
                                refs.push(TextFontRef {
                                    family,
                                    style: text.font_style.as_deref().unwrap_or_default(),
                                });
                            }
                        }
                    }
                }
                for insert in &pag.text_inserts {
                    if let Some(family) = insert.font_family.as_deref() {
                        refs.push(TextFontRef {
                            family,
                            style: insert.font_style.as_deref().unwrap_or_default(),
                        });
                    }
                }
            }
            LayerData::AiEdit(ai_edit) => collect_text_font_refs(&ai_edit.layers, refs),
            LayerData::BooleanOperation(group) => collect_text_font_refs(&group.layers, refs),
            LayerData::Media(_)
            | LayerData::Video(_)
            | LayerData::Image(_)
            | LayerData::Rect(_)
            | LayerData::Shape(_)
            | LayerData::Audio(_)
            | LayerData::Adjustment(_) => {}
        }
    }
}

fn collect_text_contents<'a>(layers: &'a [Layer], contents: &mut Vec<&'a str>) {
    for layer in layers {
        match layer.data() {
            LayerData::Text(text) => contents.push(&text.source_text.text),
            LayerData::Group(group) => collect_text_contents(&group.layers, contents),
            LayerData::Pag(pag) => {
                for item in &pag.items {
                    for entry in &item.configuration {
                        if let Some(text) = &entry.pag_layer_config.text {
                            contents.push(&text.text);
                        }
                    }
                }
                contents.extend(pag.text_inserts.iter().map(|insert| insert.text.as_str()));
            }
            LayerData::AiEdit(ai_edit) => collect_text_contents(&ai_edit.layers, contents),
            LayerData::BooleanOperation(group) => collect_text_contents(&group.layers, contents),
            LayerData::Media(_)
            | LayerData::Video(_)
            | LayerData::Image(_)
            | LayerData::Rect(_)
            | LayerData::Shape(_)
            | LayerData::Audio(_)
            | LayerData::Adjustment(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn range_json() -> Value {
        json!({ "start": 0, "duration": 1000 })
    }

    fn audio_playback() -> Value {
        json!({
            "type": "windowed", "inputRange": range_json(),
            "mapping": {"type": "linear", "input": range_json(), "output": range_json()},
            "inputOffsetMs": 0
        })
    }

    fn add_fixture_active_ranges(value: &mut Value) {
        let Some(layers) = value.as_array_mut() else {
            return;
        };

        for layer in layers {
            add_fixture_timing_to_layer(layer);
        }
    }

    fn add_fixture_timing_to_layer(layer: &mut Value) {
        let Some(layer_obj) = layer.as_object_mut() else {
            return;
        };

        match layer_obj.get("type").and_then(Value::as_str) {
            Some("Audio") => add_source_timing(layer_obj),
            Some("Media") if is_video_media_layer(layer_obj) => {
                layer_obj.entry("activeRange").or_insert_with(range_json);
                add_source_timing(layer_obj);
            }
            _ => {
                layer_obj.entry("activeRange").or_insert_with(range_json);
            }
        }

        if let Some(children) = layer_obj.get_mut("layers").and_then(Value::as_array_mut) {
            for child in children {
                add_fixture_timing_to_layer(child);
            }
        }
    }

    fn add_source_timing(layer_obj: &mut serde_json::Map<String, Value>) {
        layer_obj.entry("sourceRange").or_insert_with(range_json);
        layer_obj
            .entry("sourceIntrinsicDuration")
            .or_insert_with(|| json!(1000));
    }

    /// True for asset-video media layers — the only kind that carries source
    /// timing (image sources have no source timeline).
    fn is_video_media_layer(layer_obj: &serde_json::Map<String, Value>) -> bool {
        layer_obj
            .get("source")
            .and_then(|source| source.get("kind"))
            .and_then(Value::as_str)
            == Some("video")
    }

    fn fx_composition_from(mut value: serde_json::Value) -> FXComposition {
        if let Some(layers) = value.get_mut("layers") {
            add_fixture_active_ranges(layers);
        }
        serde_json::from_value(value).expect("fx composition fixture should parse")
    }

    fn media_layer(id: u64, asset_id: &str, with_person_matte: bool) -> serde_json::Value {
        let mut layer = json!({
            "id": id,
            "name": format!("m{id}"),
            "type": "Media",
            "activeRange": range_json(),
            "sourceRange": range_json(),
            "sourceIntrinsicDuration": 1000,
            "transform": {
                "anchorPoint": [0, 0],
                "position": [0, 0],
                "scale": [100, 100],
                "rotation": 0,
                "opacity": 100
            },
            "source": { "assetId": asset_id, "kind": "video", "fit": "cover" }
        });
        if with_person_matte {
            layer["effects"] = json!([{ "id": id, "effect": { "type": "personMatte" } }]);
        }
        layer
    }

    fn media_layer_with_kind(
        id: u64,
        asset_id: &str,
        kind: &str,
        with_person_matte: bool,
    ) -> serde_json::Value {
        let mut layer = media_layer(id, asset_id, with_person_matte);
        layer["source"]["kind"] = json!(kind);
        if kind == "image" {
            let layer_obj = layer.as_object_mut().expect("media layer is an object");
            layer_obj.remove("sourceRange");
            layer_obj.remove("sourceIntrinsicDuration");
        }
        layer
    }

    fn text_layer(id: u64, family: &str, style: &str) -> serde_json::Value {
        json!({
            "id": id,
            "name": format!("t{id}"),
            "type": "Text",
            "activeRange": range_json(),
            "transform": {
                "anchorPoint": [0, 0],
                "position": [0, 0],
                "scale": [100, 100],
                "rotation": 0,
                "opacity": 100
            },
            "sourceText": {
                "text": "hi",
                "fontFamily": family,
                "fontStyle": style,
                "fontSize": 24,
                "fillColor": [1, 1, 1, 1]
            }
        })
    }

    #[test]
    fn fx_asset_refs_union_static_sources_with_constant_overrides() {
        // A media layer referencing `static-asset`, plus a constant
        // `mediaSourceAssetId` override binding `override-asset` to it. Both
        // must surface so `Project::required_resources` preloads either id
        // the playhead could land on.
        let composition = fx_composition_from(json!({
            "id": "c",
            "name": "c",
            "layers": [media_layer(1, "static-asset", false)],
            "dynamics": {
                "entries": [{
                    "target": {
                        "kind": "layer",
                        "layerId": 1,
                        "propertyType": "mediaSourceAssetId"
                    },
                    "animator": {
                        "type": "constant",
                        "value": { "type": "string", "value": "override-asset" }
                    }
                }]
            }
        }));
        let refs = composition.asset_refs();
        let ids: Vec<&str> = refs.iter().map(|r| r.asset_id).collect();
        assert!(
            refs.iter().all(|r| r.kind == AssetKind::Video),
            "media-source overrides classify as Video"
        );
        assert!(ids.contains(&"static-asset"), "static source must surface");
        assert!(
            ids.contains(&"override-asset"),
            "constant override id must surface so it is preloaded"
        );
    }

    #[test]
    fn fx_asset_refs_union_keyframed_media_and_audio_overrides() {
        let composition = fx_composition_from(json!({
            "id": "c",
            "name": "c",
            "layers": [
                media_layer(1, "static-video", false),
                {
                    "id": 2,
                    "name": "Music",
                    "type": "Audio",
                    "playback": audio_playback(),
                    "sourceRange": range_json(),
                    "sourceIntrinsicDuration": 1000,
                    "volume": 1.0,
                    "source": { "assetId": "static-audio" }
                }
            ],
            "dynamics": {
                "entries": [
                    {
                        "target": {
                            "kind": "layer",
                            "layerId": 1,
                            "propertyType": "mediaSourceAssetId"
                        },
                        "animator": {
                            "type": "keyframes",
                            "enabled": true,
                            "keyframes": [
                                {
                                    "id": "video-a",
                                    "layerTime": 0,
                                    "value": { "type": "string", "value": "video-a" },
                                    "easing": { "type": "hold" }
                                },
                                {
                                    "id": "video-b",
                                    "layerTime": 500,
                                    "value": { "type": "string", "value": "video-b" },
                                    "easing": { "type": "hold" }
                                }
                            ]
                        }
                    },
                    {
                        "target": {
                            "kind": "layer",
                            "layerId": 2,
                            "propertyType": "audioSourceAssetId"
                        },
                        "animator": {
                            "type": "keyframes",
                            "enabled": true,
                            "keyframes": [
                                {
                                    "id": "audio-a",
                                    "layerTime": 0,
                                    "value": { "type": "string", "value": "audio-a" },
                                    "easing": { "type": "hold" }
                                },
                                {
                                    "id": "audio-b",
                                    "layerTime": 500,
                                    "value": { "type": "string", "value": "audio-b" },
                                    "easing": { "type": "hold" }
                                }
                            ]
                        }
                    }
                ]
            }
        }));

        let refs = composition.asset_refs();
        let actual: Vec<(AssetKind, &str)> = refs
            .iter()
            .map(|asset_ref| (asset_ref.kind, asset_ref.asset_id))
            .collect();
        assert_eq!(
            actual,
            vec![
                (AssetKind::Video, "static-video"),
                (AssetKind::Audio, "static-audio"),
                (AssetKind::Video, "video-a"),
                (AssetKind::Video, "video-b"),
                (AssetKind::Audio, "audio-a"),
                (AssetKind::Audio, "audio-b"),
            ],
            "every reachable keyframed asset id must be available to resource preloading"
        );
    }

    #[test]
    fn fx_asset_refs_union_image_sources_with_constant_overrides() {
        let composition = fx_composition_from(json!({
            "id": "c",
            "name": "c",
            "layers": [media_layer_with_kind(1, "static-image", "image", false)],
            "dynamics": {
                "entries": [{
                    "target": {
                        "kind": "layer",
                        "layerId": 1,
                        "propertyType": "mediaSourceAssetId"
                    },
                    "animator": {
                        "type": "constant",
                        "value": { "type": "string", "value": "override-image" }
                    }
                }]
            }
        }));
        let refs = composition.asset_refs();
        let ids: Vec<&str> = refs.iter().map(|r| r.asset_id).collect();
        assert!(
            refs.iter().all(|r| r.kind == AssetKind::Image),
            "image media-source overrides classify as Image"
        );
        assert!(
            ids.contains(&"static-image"),
            "static image source must surface"
        );
        assert!(
            ids.contains(&"override-image"),
            "constant image override id must surface so it is preloaded"
        );
    }

    #[test]
    fn fx_asset_refs_skip_media_overrides_without_a_matching_layer() {
        // Media override asset kind comes from the target layer's source kind.
        // If the target layer is missing, there is no video/image source kind
        // to report.
        let composition = fx_composition_from(json!({
            "id": "c",
            "name": "c",
            "layers": [],
            "dynamics": {
                "entries": [{
                    "target": {
                        "kind": "layer",
                        "layerId": 7,
                        "propertyType": "mediaSourceAssetId"
                    },
                    "animator": {
                        "type": "constant",
                        "value": { "type": "string", "value": "lonely-asset" }
                    }
                }]
            }
        }));
        let ids: Vec<&str> = composition
            .asset_refs()
            .iter()
            .map(|r| r.asset_id)
            .collect();
        assert!(ids.is_empty());
    }

    #[test]
    fn fx_asset_refs_union_audio_source_with_constant_audio_override() {
        // An audio layer referencing `static-song`, plus a constant
        // `audioSourceAssetId` override binding `override-song`. Both must
        // surface as `AssetKind::Audio` so the audio asset the playhead lands
        // on is preloaded.
        let composition = fx_composition_from(json!({
            "id": "c",
            "name": "c",
            "layers": [{
                "id": 1,
                "name": "Music",
                "type": "Audio",
                "playback": audio_playback(),
                "sourceRange": range_json(),
                "sourceIntrinsicDuration": 1000,
                "volume": 1.0,
                "source": { "assetId": "static-song" }
            }],
            "dynamics": {
                "entries": [{
                    "target": {
                        "kind": "layer",
                        "layerId": 1,
                        "propertyType": "audioSourceAssetId"
                    },
                    "animator": {
                        "type": "constant",
                        "value": { "type": "string", "value": "override-song" }
                    }
                }]
            }
        }));
        let refs = composition.asset_refs();
        let ids: Vec<&str> = refs.iter().map(|r| r.asset_id).collect();
        assert!(
            refs.iter().all(|r| r.kind == AssetKind::Audio),
            "audio-source overrides classify as Audio"
        );
        assert!(ids.contains(&"static-song"), "static source must surface");
        assert!(
            ids.contains(&"override-song"),
            "constant audio override id must surface so it is preloaded"
        );
    }

    #[test]
    fn fx_asset_refs_without_dynamics_match_static_walk() {
        let composition = fx_composition_from(json!({
            "id": "c",
            "name": "c",
            "layers": [media_layer(1, "only-asset", false)],
        }));
        let ids: Vec<&str> = composition
            .asset_refs()
            .iter()
            .map(|r| r.asset_id)
            .collect();
        assert_eq!(ids, vec!["only-asset"]);
    }

    #[test]
    fn fx_text_font_refs_union_constant_font_overrides() {
        let composition = fx_composition_from(json!({
            "id": "c",
            "name": "c",
            "layers": [text_layer(1, "Inter", "Regular")],
            "dynamics": {
                "entries": [
                    {
                        "target": {
                            "kind": "layer",
                            "layerId": 1,
                            "propertyType": "fontFamily"
                        },
                        "animator": {
                            "type": "constant",
                            "value": { "type": "string", "value": "Avenir Next" }
                        }
                    },
                    {
                        "target": {
                            "kind": "layer",
                            "layerId": 1,
                            "propertyType": "fontStyle"
                        },
                        "animator": {
                            "type": "constant",
                            "value": { "type": "string", "value": "Bold" }
                        }
                    }
                ]
            }
        }));
        let refs = composition.text_font_refs();
        let pairs: Vec<(&str, &str)> = refs.iter().map(|r| (r.family, r.style)).collect();
        assert!(
            pairs.contains(&("Inter", "Regular")),
            "static font pair must still surface"
        );
        assert!(
            pairs.contains(&("Avenir Next", "Bold")),
            "constant font overrides must surface so the font pair is preloaded"
        );
    }

    #[test]
    fn fx_text_contents_union_constant_text_overrides() {
        let mut layer = text_layer(1, "Inter", "Regular");
        layer["sourceText"]["text"] = json!("static ♡");
        let composition = fx_composition_from(json!({
            "id": "c",
            "name": "c",
            "layers": [layer],
            "dynamics": {
                "entries": [{
                    "target": {
                        "kind": "layer",
                        "layerId": 1,
                        "propertyType": "textContent"
                    },
                    "animator": {
                        "type": "constant",
                        "value": { "type": "string", "value": "animated ✦" }
                    }
                }]
            }
        }));

        assert_eq!(composition.text_contents(), vec!["static ♡", "animated ✦"]);
    }

    #[test]
    fn fx_composition_collects_audio_layer_asset() {
        // An `AudioLayer` source (which the `AudioGain*` derived properties
        // sample) must be discovered for preloading — otherwise `get_audio`
        // returns `Unloaded` and the gain silently reads 0.0.
        let fx: FXComposition = serde_json::from_value(json!({
            "id": "c",
            "name": "c",
            "layers": [{
                "id": 1,
                "name": "music",
                "type": "Audio",
                "playback": audio_playback(),
                "sourceRange": range_json(),
                "sourceIntrinsicDuration": 1000,
                "source": { "assetId": "song-asset" }
            }]
        }))
        .expect("fx composition fixture should parse");
        let refs = fx.asset_refs();
        assert!(
            refs.iter()
                .any(|r| r.kind == AssetKind::Audio && r.asset_id == "song-asset"),
            "audio layer asset must be collected: {refs:?}"
        );
    }
}
