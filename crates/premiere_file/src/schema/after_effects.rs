//! Native Dynamic Link identity, independent of an After Effects file parser.

use uuid::Uuid;

pub(crate) const IMPORTER_ID: &str = "ec341e53-60c2-4d89-abfc-bdb5c0ff2e0b";
pub(crate) const CODEC: &str = "1145854285";

/// Why an audio placement of a linked composition is omitted. Premiere plays a
/// link's sound only through such items, so its video items import muted.
pub(crate) const LINKED_AUDIO_REASON: &str = "the sound of a linked After Effects composition is not converted; linked-audio occurrence import is not implemented, and its video items import the picture muted";

/// The composition GUID stored by Premiere's After Effects importer.
///
/// This is not a project UUID or an AEP numeric item ID. Obtain it from the
/// selected composition's native `dynamicLinkGUID`; no ID-to-GUID mapping is
/// implied. Structural preservation does not establish editable FX import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrAfterEffectsComposition(Uuid);

impl PrAfterEffectsComposition {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        let guid = Uuid::parse_str(value).ok()?;
        (!guid.is_nil() && guid.hyphenated().to_string().eq_ignore_ascii_case(value))
            .then_some(Self(guid))
    }

    /// RFC/network-order bytes of the actual native composition identifier.
    pub fn guid_bytes(self) -> [u8; 16] {
        *self.0.as_bytes()
    }

    /// Returns the canonical native composition identifier, not its display name.
    pub fn dynamic_link_guid(self) -> String {
        self.0.hyphenated().to_string()
    }
}
