//! Decoding for AE alias metadata shared by structural and media import.

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum AliasDecodeError {
    #[error("invalid alias JSON")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AliasMetadata {
    pub(crate) fullpath: Option<String>,
    pub(crate) target_is_folder: bool,
    pub(crate) relative_location: Option<RelativeLocation>,
    pub(crate) relative_hint_malformed: bool,
}

/// AE's hint for relinking a moved project, from its alias `ascendcount_base`
/// and `ascendcount_target`: ascend `ascend` levels from the project file (one
/// reaches its directory), then follow the last `components` components of
/// the authored full path. AE's own writer relies on this reading
/// (`writer/footage.rs`); the native case is an AEP whose footage moved with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelativeLocation {
    ascend: u32,
    components: u32,
}

impl RelativeLocation {
    /// Both counts must be at least one: zero names no directory or no file.
    pub(crate) fn new(ascend: u32, components: u32) -> Option<Self> {
        (ascend > 0 && components > 0).then_some(Self { ascend, components })
    }

    /// Levels above the project file; one is the project's directory.
    pub(crate) fn ascend(self) -> u32 {
        self.ascend
    }

    /// Trailing components of the authored path to follow.
    pub(crate) fn components(self) -> u32 {
        self.components
    }

    /// The hint for a file inside the aliased folder, one component deeper.
    pub(crate) fn inside_folder(self) -> Option<Self> {
        Some(Self {
            components: self.components.checked_add(1)?,
            ..self
        })
    }
}

#[derive(Deserialize)]
struct RawAlias {
    fullpath: Option<String>,
    #[serde(default)]
    target_is_folder: bool,
    #[serde(default)]
    ascendcount_base: RawCount,
    #[serde(default)]
    ascendcount_target: RawCount,
}

/// Keep malformed counts distinct from absent hints without rejecting the alias.
#[derive(Default, Deserialize)]
#[serde(untagged)]
enum RawCount {
    #[default]
    #[serde(skip)]
    Absent,
    Count(u32),
    Other(serde::de::IgnoredAny),
}

pub(crate) fn decode(bytes: &[u8]) -> Result<AliasMetadata, AliasDecodeError> {
    let alias: RawAlias = serde_json::from_slice(bytes)?;
    let (relative_location, relative_hint_malformed) =
        match (alias.ascendcount_base, alias.ascendcount_target) {
            (RawCount::Count(ascend), RawCount::Count(components)) => {
                (RelativeLocation::new(ascend, components), false)
            }
            (RawCount::Absent, RawCount::Absent) => (None, false),
            _ => (None, true),
        };
    Ok(AliasMetadata {
        fullpath: alias.fullpath,
        target_is_folder: alias.target_is_folder,
        relative_location,
        relative_hint_malformed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_relative_counts_decode_only_as_positive_integers() {
        let native = br#"{"ascendcount_base":1,"ascendcount_target":3,"fullpath":"/Volumes/a/b/c.mp4","platform":2,"server_name":"","server_volume_name":"","target_is_folder":false}"#;
        let alias = decode(native).unwrap();
        assert_eq!(alias.fullpath.as_deref(), Some("/Volumes/a/b/c.mp4"));
        assert_eq!(alias.relative_location, RelativeLocation::new(1, 3));
        assert!(!alias.relative_hint_malformed);
        for counts in [
            "",
            r#""ascendcount_base":0,"ascendcount_target":0,"#,
            r#""ascendcount_base":0,"ascendcount_target":2,"#,
            r#""ascendcount_base":1,"ascendcount_target":0,"#,
        ] {
            let alias = decode(format!(r#"{{{counts}"fullpath":"/a/b.mp4"}}"#).as_bytes()).unwrap();
            assert_eq!(alias.relative_location, None, "{counts}");
            assert!(!alias.relative_hint_malformed, "{counts}");
        }
        for counts in [
            r#""ascendcount_base":-1,"ascendcount_target":3"#,
            r#""ascendcount_base":"1","ascendcount_target":3"#,
            r#""ascendcount_base":1.5,"ascendcount_target":3"#,
            r#""ascendcount_base":4294967296,"ascendcount_target":3"#,
            r#""ascendcount_target":3"#,
            r#""ascendcount_base":1"#,
            r#""ascendcount_base":1,"ascendcount_target":null"#,
            r#""ascendcount_base":null,"ascendcount_target":null"#,
        ] {
            let alias =
                decode(format!(r#"{{{counts},"fullpath":"/a/b.mp4"}}"#).as_bytes()).unwrap();
            assert_eq!(alias.fullpath.as_deref(), Some("/a/b.mp4"), "{counts}");
            assert_eq!(alias.relative_location, None, "{counts}");
            assert!(alias.relative_hint_malformed, "{counts}");
        }
        let folder = RelativeLocation::new(2, 2)
            .unwrap()
            .inside_folder()
            .unwrap();
        assert_eq!((folder.ascend(), folder.components()), (2, 3));
        assert!(
            RelativeLocation::new(1, u32::MAX)
                .unwrap()
                .inside_folder()
                .is_none()
        );
    }
}
