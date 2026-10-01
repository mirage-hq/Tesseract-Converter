//! Identity of this writer's fresh root, never an imported composition lookup.

pub(crate) const GENERATED_ROOT_ITEM_ID: u32 = 1;

/// Native root facts from the same plan that emitted a fresh AEP.
///
/// These facts identify a composition; they do not establish Adobe acceptance of
/// its contents. In particular, a generated Text case failed native opening.
#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedRootComposition {
    pub(crate) name: String,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) frame_rate: f64,
    pub(crate) duration_secs: f64,
}

impl GeneratedRootComposition {
    /// The reserved item identity used by the fresh root writer.
    pub const fn item_id(&self) -> u32 {
        GENERATED_ROOT_ITEM_ID
    }

    /// Observed Dynamic Link identity of this writer's fixed AE26 root profile.
    ///
    /// AE26.5x89 read this value from a freshly generated, nonempty Rect project.
    /// This is not a numeric-ID-to-GUID conversion and must not be used for
    /// imported projects or other compositions. Pair it with the exact AEP path:
    /// different generated files share this root GUID, so it is not a globally
    /// unique project or media identity. Changes to root allocation or project
    /// identity encoding require renewed native evidence.
    pub const fn dynamic_link_guid(&self) -> &'static str {
        "00000001-0000-0000-0000-000000000000"
    }

    /// The emitted composition name; linking uses identity rather than this label.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Native square-pixel canvas dimensions, after writer range validation.
    pub const fn dimensions(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    /// Actual native 16.16-encoded frame rate, not the unrounded request.
    pub const fn frame_rate(&self) -> f64 {
        self.frame_rate
    }

    /// Actual native duration after frame/tick rounding, in seconds.
    pub const fn duration_secs(&self) -> f64 {
        self.duration_secs
    }
}
