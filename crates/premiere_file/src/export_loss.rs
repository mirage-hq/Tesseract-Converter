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
