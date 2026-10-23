//! Structured loss reporting for best-effort Premiere export.
//!
//! This report describes diagnostics observed by one export traversal, with loss
//! classification where assessed. Unclassified preparation summaries can be
//! informational, not losses. It is not
//! a complete capability inventory, a native-project validation result, or a
//! fidelity verdict. In particular, an empty report does not mean that the
//! input is supported or that native content is faithful.

use std::collections::HashSet;

use fx_schema::{LayerId, PropertyTarget};

use crate::{Omission, OmissionKind, OmissionScope};

/// The source object that owned an observed export loss.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportLossSource {
    /// The document as a whole.
    Document,
    /// A layer and the subtree rooted at that layer.
    LayerSubtree(LayerId),
    /// Exactly one layer.
    Layer(LayerId),
    /// Exactly one animated property target.
    Property(PropertyTarget),
}

/// The output domain affected by an observed export loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportLossDomain {
    /// The affected output domain has not been classified.
    Unclassified,
    /// Picture content or editable picture controls.
    Picture,
    /// Audio content or editable audio controls.
    Audio,
    /// Shared context that can affect picture, audio, or their relationship.
    SharedContext,
    /// Descriptive metadata is affected.
    Metadata,
}

/// A field whose source semantics are not fully represented by export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportField {
    /// Layer description text.
    Description,
    /// Unclassified metadata.
    Metadata,
    /// Layer parent/group structure.
    ParentGrouping,
    /// Track-matte behavior.
    TrackMatte,
    /// Layer masks.
    Masks,
    /// Rounded-corner geometry.
    CornerRadius,
    /// Audio-layer playback controls.
    AudioPlaybackSettings,
    /// Pitch preservation during audio retiming.
    AudioPitchPreservation,
    /// Picture media that cannot be emitted by the native writer.
    PictureMedia,
    /// Picture effects.
    Effects,
    /// Spatial placement.
    Placement,
    /// Caption presentation.
    CaptionPresentation,
    /// Motion blur.
    MotionBlur,
    /// Captions.
    Captions,
    /// Skew transforms.
    Skew,
    /// Three-dimensional rotation.
    Rotation3d,
    /// A blend mode not supported by the native output.
    BlendMode,
    /// Media fitting approximated with contain.
    MediaFit,
    /// Time remapping approximated with a constant-speed source range.
    TimeRemap,
    /// Audio enhancement.
    AudioEnhancement,
    /// Input transforms.
    InputTransform,
}

impl ExportField {
    /// Returns the output domain affected by this field.
    #[must_use]
    pub const fn domain(self) -> ExportLossDomain {
        match self {
            Self::Description => ExportLossDomain::Metadata,
            Self::Metadata => ExportLossDomain::Unclassified,
            Self::ParentGrouping | Self::TimeRemap => ExportLossDomain::SharedContext,
            Self::AudioPlaybackSettings | Self::AudioPitchPreservation | Self::AudioEnhancement => {
                ExportLossDomain::Audio
            }
            Self::TrackMatte
            | Self::Masks
            | Self::CornerRadius
            | Self::PictureMedia
            | Self::Effects
            | Self::Placement
            | Self::CaptionPresentation
            | Self::MotionBlur
            | Self::Captions
            | Self::Skew
            | Self::Rotation3d
            | Self::BlendMode
            | Self::MediaFit
            | Self::InputTransform => ExportLossDomain::Picture,
        }
    }
}

impl std::fmt::Display for ExportField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Description => "description",
            Self::Metadata => "metadata",
            Self::ParentGrouping => "parent grouping",
            Self::TrackMatte => "track matte",
            Self::Masks => "masks",
            Self::CornerRadius => "corner radius",
            Self::AudioPlaybackSettings => "playback settings",
            Self::AudioPitchPreservation => "audio pitch preservation",
            Self::PictureMedia => "picture media",
            Self::Effects => "effects",
            Self::Placement => "placement",
            Self::CaptionPresentation => "caption presentation",
            Self::MotionBlur => "motion blur",
            Self::Captions => "captions",
            Self::Skew => "skew",
            Self::Rotation3d => "3D rotation",
            Self::BlendMode => "unsupported blend mode",
            Self::MediaFit => "media fit (using contain)",
            Self::TimeRemap => "time remap (using source range at constant speed)",
            Self::AudioEnhancement => "audio enhancement",
            Self::InputTransform => "input transform",
        })
    }
}

/// The typed classification of an observed export loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportLossKind {
    /// The omission has no more specific typed classification.
    Unclassified,
    /// A known source field was omitted or approximated.
    Field(ExportField),
}

/// One observed export diagnostic, with typed loss classification where assessed.
/// Unclassified preparation summaries can be informational rather than losses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportLoss {
    /// The source object that owned the event.
    pub source: ExportLossSource,
    /// The output domain affected by the event.
    pub domain: ExportLossDomain,
    /// The event's typed classification.
    pub kind: ExportLossKind,
    /// The legacy human-readable diagnostic emitted for the event.
    pub omission: Omission,
}

/// Structured routing losses and distinct legacy diagnostics from one traversal.
///
/// This is not a complete capability inventory, native-project validation, or
/// fidelity verdict. Zero recorded losses does not establish support.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportLossReport {
    /// Events observed before the final diagnostic sink's deduplication.
    /// Existing intermediate buffers can already have deduplicated their messages.
    pub losses: Vec<ExportLoss>,
    /// Distinct human-readable diagnostics, in first-report order.
    pub diagnostics: Vec<Omission>,
    /// Compatibility marker for older bounded collectors; current collection is complete.
    pub losses_truncated: bool,
    /// Whether native lowering produced content, not whether publication or Adobe acceptance succeeded.
    pub has_native_content: bool,
}

/// The source and output domain inherited by an unclassified omission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExportContext {
    pub(crate) source: ExportLossSource,
    pub(crate) domain: ExportLossDomain,
}

/// Receives legacy diagnostics and, when supported, typed loss context.
pub(crate) trait OmissionSink {
    /// Emits one human diagnostic.
    fn emit(&mut self, omission: Omission);

    /// Reports proven, root-sequence-sampled direct-still grouping normalization.
    /// Unlike an approximation of visible content, this is not a routing loss.
    fn emit_timed_image_normalization(&mut self, omission: Omission) {
        self.emit(omission);
    }

    /// Reports a deliberately retained native Lens/Alpha Glow/Grain approximation,
    /// or the declared scale mapping of a written Lumetri Color.
    /// The current editable effect was emitted successfully; replacing its
    /// picture with linked AE would discard the chosen native representation.
    /// Missing/failed effects must still use ordinary loss reporting.
    fn emit_retained_native_effect(&mut self, omission: Omission) {
        self.emit(omission);
    }

    /// Returns the current typed context, if this sink tracks one.
    fn context(&self) -> Option<&ExportContext> {
        None
    }

    /// Replaces the current typed context and returns the previous context.
    fn replace_context(&mut self, _context: ExportContext) -> Option<ExportContext> {
        None
    }

    /// Emits a known field loss owned by exactly one layer.
    fn emit_field(&mut self, omission: Omission, _layer: LayerId, _field: ExportField) {
        self.emit(omission);
    }
}

impl OmissionSink for Vec<Omission> {
    fn emit(&mut self, omission: Omission) {
        crate::push_omission(self, omission);
    }
}

/// Runs `f` under `context`, restoring a previous tracked context afterward.
pub(crate) fn with_context<T>(
    sink: &mut dyn OmissionSink,
    context: ExportContext,
    f: impl FnOnce(&mut dyn OmissionSink) -> T,
) -> T {
    let previous = sink.replace_context(context);
    let result = f(sink);
    if let Some(previous) = previous {
        let _ = sink.replace_context(previous);
    }
    result
}

/// Emits one legacy feature omission and one typed field event when supported.
pub(crate) fn omit_field(
    sink: &mut dyn OmissionSink,
    layer: LayerId,
    field: ExportField,
    record: impl Into<String>,
    reason: impl Into<String>,
) {
    sink.emit_field(
        Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: record.into(),
            reason: reason.into(),
        },
        layer,
        field,
    );
}

/// What export writes for a layer's `source.inputTransform`, and the report
/// of what it loses (`None` when nothing is lost).
///
/// Premiere 26.5.2 saves a clip Input LUT as a `LUTAsset` with the embedded
/// samples (`color_clip_input_lut.prproj`) and a Color Space Override as
/// `OverriddenColorSpace` (`color_clip_colorspace_override.prproj`), both on
/// the media's `VideoStream`. A `lut3d` into Rec.709 display therefore writes
/// the LUT as the clip Input LUT; its input encoding writes the equal Color
/// Space Override when one exists, and Rec.709 display input needs none.
/// `luts` holds the packaged `.cube` assets that export read. The pinned
/// override grammar is a video stream's, so a still (`video` false) writes
/// no override: its camera interpretation is reported instead. In an ACES
/// document (`aces`) a `cameraInput` writes the equal Color Space Override,
/// which an ACEScct sequence converts from the camera's log curve and gamut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InputTransformExport {
    /// The Premiere Color Space Override label written on the media.
    pub(crate) color_space_override: Option<&'static str>,
    /// The clip Input LUT written on the media.
    pub(crate) input_lut: Option<crate::schema::PrClipInputLut>,
    pub(crate) loss: Option<String>,
}

/// Report the retained approximation of a written clip Input LUT: Premiere
/// interpolates it tetrahedrally ([`crate::cube_lut::PREMIERE_LUT_INTERPOLATION`]).
/// The LUT itself is written, so this is not a routing loss.
pub(crate) fn report_clip_input_lut_interpolation(sink: &mut dyn OmissionSink, record: &str) {
    sink.emit_retained_native_effect(Omission {
        scope: OmissionScope::Feature,
        kind: crate::OmissionKind::Approximated,
        record: record.to_owned(),
        reason: format!(
            "source.inputTransform written as the clip Interpret Footage Input LUT; {}",
            crate::cube_lut::PREMIERE_LUT_INTERPOLATION
        ),
    });
}

pub(crate) fn input_transform_export(
    transform: &fx_schema::PersistedInputTransform,
    luts: &crate::cube_lut::InputLutAssets,
    video: bool,
    aces: bool,
) -> InputTransformExport {
    let prefix = format!("{} was not exported", ExportField::InputTransform);
    let lost = |reason: String| InputTransformExport {
        color_space_override: None,
        input_lut: None,
        loss: Some(reason),
    };
    let camera_input = |encoding: &str| {
        match crate::schema::premiere_space_for_camera_encoding(encoding) {
        _ if !aces => lost(format!("{prefix}: cameraInput {encoding} is an ACES Input Transform, but the document is not an ACES document")),
        Some(space) if video => InputTransformExport {
            color_space_override: Some(space),
            input_lut: None,
            loss: None,
        },
        Some(space) => lost(format!("{prefix}: cameraInput {encoding} on a still: the pinned Color Space Override grammar is a video stream's, so Color Space Override {space:?} is not written and the still plays with its own interpretation")),
        None => lost(format!("{prefix}: cameraInput {encoding} has no equal Premiere Color Space Override, so the clip plays with its own interpretation in the ACEScct sequence")),
    }
    };
    let Some(transform) = transform.as_supported() else {
        // The reader keeps every non-`lut3d` transform opaque, including
        // `cameraInput`, the ACES-only camera declaration.
        let payload = transform.unsupported_payload();
        let camera = payload
            .filter(|payload| payload.pointer("/transform/type") == Some(&"cameraInput".into()))
            .and_then(|payload| payload.pointer("/transform/inputEncoding"))
            .and_then(serde_json::Value::as_str);
        return match camera {
            Some(encoding) => camera_input(encoding),
            None => lost(format!(
                "{prefix}: unsupported future input-transform payload"
            )),
        };
    };
    match &transform.transform {
        fx_schema::InputColorTransform::Lut3d {
            asset_id,
            input_encoding,
            output_encoding,
            ..
        } => {
            let (input, output) = (input_encoding.to_string(), output_encoding.to_string());
            let asset = asset_id.as_str();
            let camera_space = crate::schema::premiere_space_for_camera_encoding(&input);
            let space = camera_space.filter(|_| video);
            if output != fx_schema::color::SDR_REC709_DISPLAY_ENCODING_ID {
                let manual = space.map_or_else(String::new, |space| {
                    format!(" with Color Space Override {space:?}")
                });
                return lost(format!("{prefix}: lut3d {input} -> {output} does not end in Rec.709 display, the values a clip Input LUT hands to a Rec.709 sequence, so no clip Input LUT{} is written and the picture plays uninterpreted; apply Modify > Interpret Footage > Input LUT asset {asset:?}{manual} in Premiere to restore it", if space.is_some() { " or Color Space Override" } else { "" }));
            }
            let packaged = match luts.get(asset) {
                Some(Ok(packaged)) => packaged,
                unread => {
                    let why = match unread {
                        Some(Err(reason)) => reason.clone(),
                        _ => "was not read for export".to_owned(),
                    };
                    // Premiere's own camera conversion still stands in for the LUT.
                    return InputTransformExport {
                        color_space_override: space,
                        input_lut: None,
                        loss: Some(match space {
                            Some(space) => format!("{prefix} as a LUT: the lut3d .cube asset {asset:?} {why}, so no clip Input LUT is written; Color Space Override {space:?} was written instead, so Premiere's own {space} to working-space conversion replaces the LUT and colour specific to that LUT is lost"),
                            None => format!("{prefix}: the lut3d .cube asset {asset:?} {why}, so no clip Input LUT is written and the picture plays without it"),
                        }),
                    };
                }
            };
            let input_lut = Some(packaged.clip_input_lut());
            let written = |loss: Option<String>| InputTransformExport {
                color_space_override: space,
                input_lut: input_lut.clone(),
                loss,
            };
            let file = &packaged.file_name;
            match (space, camera_space) {
                (Some(_), _) => written(None),
                (None, Some(label)) => written(Some(format!("{prefix} input encoding: lut3d {input} -> {output} was written as the still's Input LUT {file:?}, but no Color Space Override {label:?} is written on a still image, so Premiere applies the LUT to the still's own interpretation"))),
                (None, None) if input == fx_schema::color::SDR_REC709_DISPLAY_ENCODING_ID => written(None),
                (None, None) => written(Some(format!("{prefix} input encoding: lut3d {input} -> {output} was written as the clip Input LUT {file:?}, but Premiere offers no Color Space Override equal to {input}, so Premiere applies the LUT to the clip's own interpretation rather than to {input} values"))),
            }
        }
        fx_schema::InputColorTransform::CameraInput { input_encoding } => {
            camera_input(&input_encoding.to_string())
        }
    }
}

/// Retains every routing event and every distinct human diagnostic.
#[derive(Debug)]
pub(crate) struct LossCollector {
    context: ExportContext,
    losses: Vec<ExportLoss>,
    diagnostics: Vec<Omission>,
    seen: HashSet<Omission>,
}

impl Default for LossCollector {
    fn default() -> Self {
        Self {
            context: ExportContext {
                source: ExportLossSource::Document,
                domain: ExportLossDomain::Unclassified,
            },
            losses: Vec::new(),
            diagnostics: Vec::new(),
            seen: HashSet::new(),
        }
    }
}

impl LossCollector {
    pub(crate) fn diagnostics(&self) -> &[Omission] {
        &self.diagnostics
    }

    fn push_diagnostic(&mut self, omission: Omission) {
        if !self.seen.contains(&omission) {
            self.seen.insert(omission.clone());
            self.diagnostics.push(omission);
        }
    }

    fn record_loss(
        &mut self,
        source: ExportLossSource,
        domain: ExportLossDomain,
        kind: ExportLossKind,
        omission: &Omission,
    ) {
        self.losses.push(ExportLoss {
            source,
            domain,
            kind,
            omission: omission.clone(),
        });
    }

    /// Finishes collection without inferring support from the observed events.
    pub(crate) fn finish(self, has_native_content: bool) -> ExportLossReport {
        ExportLossReport {
            losses: self.losses,
            diagnostics: self.diagnostics,
            losses_truncated: false,
            has_native_content,
        }
    }
}

impl OmissionSink for LossCollector {
    fn emit_retained_native_effect(&mut self, omission: Omission) {
        self.push_diagnostic(omission);
    }

    fn emit_timed_image_normalization(&mut self, omission: Omission) {
        self.push_diagnostic(omission);
    }

    fn emit(&mut self, omission: Omission) {
        self.record_loss(
            self.context.source.clone(),
            self.context.domain,
            ExportLossKind::Unclassified,
            &omission,
        );
        self.push_diagnostic(omission);
    }

    fn context(&self) -> Option<&ExportContext> {
        Some(&self.context)
    }

    fn replace_context(&mut self, context: ExportContext) -> Option<ExportContext> {
        Some(std::mem::replace(&mut self.context, context))
    }

    fn emit_field(&mut self, omission: Omission, layer: LayerId, field: ExportField) {
        self.record_loss(
            ExportLossSource::Layer(layer),
            field.domain(),
            ExportLossKind::Field(field),
            &omission,
        );
        self.push_diagnostic(omission);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn omission(record: &str, reason: &str) -> Omission {
        Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: record.into(),
            reason: reason.into(),
        }
    }

    #[test]
    fn timed_image_normalization_retains_diagnostic_but_not_picture_loss() {
        let mut collector = LossCollector::default();
        let event = omission("layer 7", "sampled grouping normalization");
        collector.emit_timed_image_normalization(event.clone());
        omit_field(
            &mut collector,
            LayerId::new(7),
            ExportField::Placement,
            "layer 7",
            "visible off-grid picture change",
        );
        let report = collector.finish(true);
        assert_eq!(report.diagnostics.len(), 2);
        assert_eq!(report.diagnostics[0], event);
        assert_eq!(report.losses.len(), 1);
        assert_eq!(
            report.losses[0].kind,
            ExportLossKind::Field(ExportField::Placement)
        );
        assert_eq!(report.losses[0].domain, ExportLossDomain::Picture);
    }

    #[test]
    fn alpha_glow_retained_native_diagnostic_preserves_other_picture_losses() {
        let mut collector = LossCollector::default();
        let event = omission(
            "layer 7",
            "retained native Alpha Glow falloff approximation",
        );
        collector.emit_retained_native_effect(event.clone());
        omit_field(
            &mut collector,
            LayerId::new(7),
            ExportField::Effects,
            "layer 7",
            "another picture effect has no native mapping",
        );
        let report = collector.finish(true);
        assert_eq!(report.diagnostics.len(), 2);
        assert_eq!(report.diagnostics[0], event);
        assert_eq!(report.losses.len(), 1);
        assert_eq!(report.losses[0].domain, ExportLossDomain::Picture);
        assert_eq!(
            report.losses[0].kind,
            ExportLossKind::Field(ExportField::Effects)
        );
    }

    #[test]
    fn vec_sink_preserves_legacy_deduplication() {
        let mut diagnostics = Vec::new();
        omit_field(
            &mut diagnostics,
            LayerId::new(7),
            ExportField::Effects,
            "layer 7",
            "not representable",
        );
        omit_field(
            &mut diagnostics,
            LayerId::new(7),
            ExportField::Effects,
            "layer 7",
            "not representable",
        );

        assert_eq!(diagnostics, vec![omission("layer 7", "not representable")]);
    }

    #[test]
    fn duplicate_human_diagnostics_keep_distinct_typed_sources() {
        let mut collector = LossCollector::default();
        let diagnostic = omission("shared record", "same reason");
        for layer in [LayerId::new(1), LayerId::new(2)] {
            with_context(
                &mut collector,
                ExportContext {
                    source: ExportLossSource::LayerSubtree(layer),
                    domain: ExportLossDomain::Picture,
                },
                |sink| sink.emit(diagnostic.clone()),
            );
        }

        let report = collector.finish(false);
        assert_eq!(report.losses.len(), 2);
        assert_eq!(report.diagnostics, vec![diagnostic]);
        assert_eq!(
            report.losses[0].source,
            ExportLossSource::LayerSubtree(LayerId::new(1))
        );
        assert_eq!(
            report.losses[1].source,
            ExportLossSource::LayerSubtree(LayerId::new(2))
        );
    }

    #[test]
    fn distinct_diagnostics_keep_first_report_order_and_exact_identity() {
        let mut collector = LossCollector::default();
        let first = omission("same record", "same reason");
        let mut other = first.clone();
        other.kind = OmissionKind::Approximated;
        for entry in [&first, &other, &first, &other] {
            collector.emit(entry.clone());
        }
        let report = collector.finish(true);
        assert_eq!(report.diagnostics, [first, other]);
        assert_eq!(report.losses.len(), 4);
    }

    #[test]
    fn routing_events_are_not_truncated_at_the_former_count_limit() {
        let mut collector = LossCollector::default();
        let diagnostic = omission("one record", "one reason");
        for _ in 0..2049 {
            collector.emit(diagnostic.clone());
        }

        let report = collector.finish(true);
        assert_eq!(report.losses.len(), 2049);
        assert!(!report.losses_truncated);
        assert_eq!(report.diagnostics, vec![diagnostic]);
        assert!(report.has_native_content);
    }

    #[test]
    fn large_diagnostic_text_is_preserved_by_both_collectors() {
        let large = omission("record", &"x".repeat(1 << 20));
        let mut legacy = Vec::new();
        legacy.emit(large.clone());
        let mut collector = LossCollector::default();
        collector.emit(large);
        let report = collector.finish(true);
        assert!(!report.losses_truncated);
        assert_eq!(report.losses.len(), 1);
        assert_eq!(report.diagnostics, legacy);
        assert_eq!(report.diagnostics[0].reason.len(), 1 << 20);
    }

    #[test]
    fn long_property_names_preserve_routing_identity() {
        let target = PropertyTarget::effect_param(fx_schema::EffectId::new(1), "x".repeat(1 << 20));
        let mut collector = LossCollector::default();
        collector.replace_context(ExportContext {
            source: ExportLossSource::Property(target),
            domain: ExportLossDomain::Unclassified,
        });
        let expected = omission("record", "unsupported property");
        collector.emit(expected.clone());
        let report = collector.finish(true);
        assert!(!report.losses_truncated);
        assert_eq!(report.losses.len(), 1);
        assert_eq!(report.diagnostics, [expected]);
    }

    #[test]
    fn context_is_restored_after_error_result() {
        let mut collector = LossCollector::default();
        let original = collector.context().cloned();
        let result: Result<(), &str> = with_context(
            &mut collector,
            ExportContext {
                source: ExportLossSource::Layer(LayerId::new(9)),
                domain: ExportLossDomain::Audio,
            },
            |_| Err("stop"),
        );

        assert_eq!(result, Err("stop"));
        assert_eq!(collector.context(), original.as_ref());
    }

    #[test]
    fn typed_fields_have_declared_domains_and_exact_labels() {
        let cases = [
            (
                ExportField::Description,
                "description",
                ExportLossDomain::Metadata,
            ),
            (
                ExportField::Metadata,
                "metadata",
                ExportLossDomain::Unclassified,
            ),
            (
                ExportField::ParentGrouping,
                "parent grouping",
                ExportLossDomain::SharedContext,
            ),
            (
                ExportField::TrackMatte,
                "track matte",
                ExportLossDomain::Picture,
            ),
            (ExportField::Masks, "masks", ExportLossDomain::Picture),
            (
                ExportField::CornerRadius,
                "corner radius",
                ExportLossDomain::Picture,
            ),
            (
                ExportField::AudioPlaybackSettings,
                "playback settings",
                ExportLossDomain::Audio,
            ),
            (
                ExportField::AudioPitchPreservation,
                "audio pitch preservation",
                ExportLossDomain::Audio,
            ),
            (ExportField::Effects, "effects", ExportLossDomain::Picture),
            (
                ExportField::Placement,
                "placement",
                ExportLossDomain::Picture,
            ),
            (
                ExportField::CaptionPresentation,
                "caption presentation",
                ExportLossDomain::Picture,
            ),
            (
                ExportField::MotionBlur,
                "motion blur",
                ExportLossDomain::Picture,
            ),
            (ExportField::Captions, "captions", ExportLossDomain::Picture),
            (ExportField::Skew, "skew", ExportLossDomain::Picture),
            (
                ExportField::Rotation3d,
                "3D rotation",
                ExportLossDomain::Picture,
            ),
            (
                ExportField::BlendMode,
                "unsupported blend mode",
                ExportLossDomain::Picture,
            ),
            (
                ExportField::MediaFit,
                "media fit (using contain)",
                ExportLossDomain::Picture,
            ),
            (
                ExportField::TimeRemap,
                "time remap (using source range at constant speed)",
                ExportLossDomain::SharedContext,
            ),
            (
                ExportField::AudioEnhancement,
                "audio enhancement",
                ExportLossDomain::Audio,
            ),
            (
                ExportField::InputTransform,
                "input transform",
                ExportLossDomain::Picture,
            ),
        ];

        for (field, label, domain) in cases {
            assert_eq!(field.to_string(), label);
            assert_eq!(field.domain(), domain);
        }
    }

    #[test]
    fn field_event_uses_exact_layer_and_does_not_add_generic_event() {
        let mut collector = LossCollector::default();
        omit_field(
            &mut collector,
            LayerId::new(42),
            ExportField::AudioEnhancement,
            "audio",
            "unsupported",
        );

        let report = collector.finish(false);
        assert_eq!(report.losses.len(), 1);
        assert_eq!(
            report.losses[0].source,
            ExportLossSource::Layer(LayerId::new(42))
        );
        assert_eq!(report.losses[0].domain, ExportLossDomain::Audio);
        assert_eq!(
            report.losses[0].kind,
            ExportLossKind::Field(ExportField::AudioEnhancement)
        );
    }
}
