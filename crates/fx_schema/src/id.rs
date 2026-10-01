//! Canonical identity vocabulary for portable FX documents.
//!
//! These branded ID newtypes are shared by the portable schema and internal
//! compatibility facades, so an `asset_id` can never be silently swapped for
//! an arbitrary `String`.
//!
//! Today it exposes:
//!
//! - [`AssetId`] — a Captions asset identifier (video / image / audio / PAG /
//!   model / font reference). Stored as `Arc<str>` because asset ids are
//!   cloned constantly into renderer / codec / audio-renderer cache keys; the
//!   clone is an O(1) refcount bump rather than a heap copy.
//! - [`LayerId`] — a stable `fx_composition` layer identifier. A branded
//!   `u64` (parenting, layer-mask references, animator property addresses);
//!   crosses JSON / ts-rs as a `number`.
//! - [`FxItemId`] — a stable identifier for one stacked `fx_composition`
//!   sub-item (path masks; future stacked layer styles). Same branded-`u64`
//!   discipline as [`LayerId`] / [`EffectId`] (JRB-1373).
//! - [`CompositionId`] — a stable `fx_composition` composition identifier.
//!   The string-backed counterpart to [`LayerId`], same `Arc<str>` /
//!   `#[serde(transparent)]` discipline as [`AssetId`] but without the
//!   path-traversal validation (composition ids are opaque, never used as a
//!   filesystem path).
//!
//! # Serde / FFI contract
//!
//! [`AssetId`] is `#[serde(transparent)]` + `#[ts(type = "string")]`, so it
//! crosses JSON, ts-rs, serde-wasm-bindgen, and uniffi boundaries as a plain
//! string — exactly the discipline `time_types` uses for `Time` (`number`).
//!
//! # Validation policy (read before migrating call sites)
//!
//! `Deserialize` is intentionally **non-failing**: it wraps whatever string
//! is on the wire without validating. This preserves today's behavior, where
//! `resource::validate_asset_id` is checked at the *point of use* (path
//! construction), not at parse time — flipping that to a parse-time hard
//! error could reject projects that currently render fine.
//!
//! Validation is still available, and is the recommended path for any *new*
//! ingestion boundary:
//!
//! - [`AssetId::new`] — checked constructor for a flat id (single path
//!   segment; rejects `/`). Mirrors `resource::validate_asset_id`.
//! - [`AssetId::new_namespaced`] — checked constructor that allows
//!   `namespace/name` segments (e.g. `emojis/foo`). Mirrors
//!   `resource::validate_pag_asset_id`.
//! - [`AssetId::from_trusted`] — explicit no-validation construction for ids
//!   that came from an already-trusted source (proto decode, a prior
//!   `AssetId`, a test fixture).
//!
//! Long-term the `resource` validators should delegate to
//! [`AssetId::validate`] so there is a single source of truth.

use std::borrow::Borrow;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;

/// A Captions asset identifier.
///
/// Opaque and **kind-agnostic** by design: whether the id points at a video,
/// image, audio, or PAG asset is contextual (the `scene::ImageSource` variant,
/// the `fx_composition::AssetKind`, the loader method called). Baking the kind
/// into the type would force lossy conversions every time an id moves between
/// a kind-erased container and a kind-specific one.
///
/// Backed by `Arc<str>` so cloning into cache keys is O(1). See the crate
/// docs for the serde / validation contract.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, TS)]
#[ts(type = "string")]
pub struct AssetId(Arc<str>);

// Manual serde impls: `Arc<str>` only derives `Serialize`/`Deserialize`
// behind serde's `rc` feature, which we don't want to turn on workspace-wide
// (it changes Arc sharing semantics for every type). Hand-rolling keeps the
// `#[serde(transparent)]` string contract without the feature. Deserialize is
// intentionally non-failing — see the crate-level validation policy.
impl Serialize for AssetId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for AssetId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(|s| Self(Arc::from(s)))
    }
}

/// Why a string was rejected as an [`AssetId`] by a checked constructor.
///
/// Mirrors the failure modes of `resource::validate_asset_id` so the two can
/// converge on this type. `#[non_exhaustive]` — new rejection reasons may be
/// added without a breaking change.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidAssetId {
    /// The id was the empty string.
    #[error("asset id is empty")]
    Empty,
    /// The id contained a NUL byte (would truncate paths on Linux syscalls).
    #[error("asset id contains a null byte: {id:?}")]
    NullByte { id: String },
    /// The id contained path-traversal characters (`..`, `\\`, or a
    /// disallowed `/`), or an empty / `.` / `..` path segment.
    #[error("asset id contains path traversal characters: {id:?}")]
    PathTraversal { id: String },
}

impl AssetId {
    /// Construct a validated **flat** asset id (a single path segment).
    ///
    /// Rejects empty, NUL, `..`, `\\`, and any `/`. On Windows, drive-prefix
    /// and NTFS alternate-data-stream syntax are also rejected. Use this for
    /// the common case of a bare Captions asset id. Mirrors
    /// `resource::validate_asset_id`.
    ///
    /// # Errors
    /// Returns [`InvalidAssetId`] if the string violates the rules above.
    pub fn new(id: impl Into<Arc<str>>) -> Result<Self, InvalidAssetId> {
        let id = id.into();
        Self::validate(&id)?;
        Ok(Self(id))
    }

    /// Construct a validated **namespaced** asset id, allowing
    /// `namespace/name` segments (e.g. `emojis/foo`, `transitions/bar`).
    ///
    /// Still rejects empty, NUL, `..`, `\\`, a leading/trailing `/`, and any
    /// empty / `.` / `..` segment. On Windows, drive-prefix and NTFS
    /// alternate-data-stream syntax are also rejected. Mirrors
    /// `resource::validate_pag_asset_id`.
    ///
    /// # Errors
    /// Returns [`InvalidAssetId`] if the string violates the rules above.
    pub fn new_namespaced(id: impl Into<Arc<str>>) -> Result<Self, InvalidAssetId> {
        let id = id.into();
        Self::validate_namespaced(&id)?;
        Ok(Self(id))
    }

    /// Wrap a string as an [`AssetId`] **without validation**.
    ///
    /// Use only when the id is already trusted: a proto decode, a value that
    /// was already an `AssetId`, or a test fixture. This is the same
    /// no-validation path [`Deserialize`] uses.
    pub fn from_trusted(id: impl Into<Arc<str>>) -> Self {
        Self(id.into())
    }

    /// Borrow the underlying string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Validate a flat asset id without constructing one. Single source of
    /// truth for [`AssetId::new`]; `resource::validate_asset_id` should
    /// delegate here once this crate is wired in.
    ///
    /// # Errors
    /// Returns [`InvalidAssetId`] if the string is not a valid flat id.
    pub fn validate(id: &str) -> Result<(), InvalidAssetId> {
        validate_impl(id, false)
    }

    /// Validate a namespaced asset id without constructing one. Single source
    /// of truth for [`AssetId::new_namespaced`].
    ///
    /// # Errors
    /// Returns [`InvalidAssetId`] if the string is not a valid namespaced id.
    pub fn validate_namespaced(id: &str) -> Result<(), InvalidAssetId> {
        validate_impl(id, true)
    }
}

fn validate_impl(id: &str, allow_slash: bool) -> Result<(), InvalidAssetId> {
    if id.is_empty() {
        return Err(InvalidAssetId::Empty);
    }
    if id.contains('\0') {
        return Err(InvalidAssetId::NullByte { id: id.to_owned() });
    }
    if id.contains('\\') || id.contains("..") {
        return Err(InvalidAssetId::PathTraversal { id: id.to_owned() });
    }
    #[cfg(windows)]
    if has_windows_drive_prefix_or_ads(id) {
        return Err(InvalidAssetId::PathTraversal { id: id.to_owned() });
    }
    if !allow_slash {
        if id.contains('/') {
            return Err(InvalidAssetId::PathTraversal { id: id.to_owned() });
        }
        return Ok(());
    }
    if id.starts_with('/') || id.ends_with('/') {
        return Err(InvalidAssetId::PathTraversal { id: id.to_owned() });
    }
    for segment in id.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(InvalidAssetId::PathTraversal { id: id.to_owned() });
        }
    }
    Ok(())
}

#[cfg(windows)]
fn has_windows_drive_prefix_or_ads(id: &str) -> bool {
    id.contains(':')
}

// ─────────────────────────────────────────────────────────────────────
// Conversions & string-like ergonomics.
//
// We expose the string only through explicit, intention-revealing surfaces:
// `as_str()` for an owned-borrow, `AsRef<str>` for generic byte/slice APIs,
// and `Borrow<str>` so a `HashMap<AssetId, _>` can be queried with a `&str`
// key. We deliberately do *not* implement `Deref<Target = str>` (C-DEREF):
// deref is for smart pointers, and letting every `&str` method resolve
// through an `AssetId` would undo the branding. Call sites that need the
// slice spell it `.as_str()`.
// ─────────────────────────────────────────────────────────────────────

impl AsRef<str> for AssetId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for AssetId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Transparent: format exactly like the inner string (`"foo"`), matching
        // `String`/`&str` and the `#[serde(transparent)]` intent. Keeping this
        // identical to `String`'s `Debug` means migrating a field from `String`
        // to `AssetId` does not churn any `insta` scene-tree snapshots.
        fmt::Debug::fmt(&*self.0, f)
    }
}

// No `From<&str>` / `From<String>` / `From<Arc<str>>`: `.into()` is the one
// conversion Rust treats as implicit and obviously-correct, so an inbound
// `From` would let `let id: AssetId = user_string.into();` compile with no
// signal that it skipped the traversal/NUL checks — exactly the silent swap
// this crate exists to prevent. Construct ids through the checked
// `new`/`new_namespaced` or the explicit, audit-greppable `from_trusted`.
// The outbound `From<AssetId> for Arc<str>` below is a lossless extraction,
// not a trust decision, so it stays.

impl From<AssetId> for Arc<str> {
    fn from(id: AssetId) -> Self {
        id.0
    }
}

impl PartialEq<str> for AssetId {
    fn eq(&self, other: &str) -> bool {
        &*self.0 == other
    }
}

impl PartialEq<&str> for AssetId {
    fn eq(&self, other: &&str) -> bool {
        &*self.0 == *other
    }
}

/// A stable layer identifier within an `fx_composition` document.
///
/// Threaded through layer parenting, layer-mask references, and animator
/// property addresses. A branded newtype over `u64`: a raw integer (an index,
/// a count, an asset's numeric field) can never be passed where a layer id is
/// expected, and a layer id can never silently stand in for an arbitrary
/// `u64`.
///
/// `#[serde(transparent)]` + `#[ts(type = "number")]` so it crosses JSON and
/// ts-rs as a plain number — the same discipline `time_types::Time` uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(transparent)]
#[ts(type = "number")]
pub struct LayerId(u64);

impl LayerId {
    /// Wraps a raw `u64` as a layer id.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying `u64`.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl From<u64> for LayerId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<LayerId> for u64 {
    fn from(id: LayerId) -> Self {
        id.0
    }
}

impl fmt::Display for LayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// A stable identifier for one effect in a layer's effect stack, unique within
/// an `fx_composition` document.
///
/// A branded `u64` with the same discipline as [`LayerId`]. It is
/// caller-assigned at `AddFxLayerEffect` time and is the **stable identity** of
/// an effect instance — independent of
/// the effect's position in its layer's stack. Animator addresses key on it, so
/// an effect-param animation survives stack inserts, removals, reorders, and
/// even moving the effect to another layer. Because it is unique across the
/// whole document, the owning layer is derivable from the id alone.
///
/// `#[serde(transparent)]` + `#[ts(type = "number")]`: crosses JSON / ts-rs as
/// a plain number, like [`LayerId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(transparent)]
#[ts(type = "number")]
pub struct EffectId(u64);

impl EffectId {
    /// Wraps a raw `u64` as an effect id.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying `u64`.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl From<u64> for EffectId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<EffectId> for u64 {
    fn from(id: EffectId) -> Self {
        id.0
    }
}

impl fmt::Display for EffectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// A stable identifier for one addressable **stacked** FX sub-item within an
/// `fx_composition` document (path masks; future stacked layer styles).
///
/// A branded `u64` with the same discipline as [`EffectId`]: caller-minted at
/// creation time (the app/AI assigns it, the same contract as a layer id),
/// unique across the whole composition, and independent of the item's position
/// in its owning stack — so an animator address keyed on it survives stack
/// inserts, removals, reorders, and moving the item to another layer. Because
/// it is document-wide unique, the owning layer and the item kind are
/// derivable from the id alone at apply time; the address carries no layer id
/// and no kind tag (JRB-1373).
///
/// Single-instance sub-items (a track matte, the inline drop shadow) stay
/// positionally addressed and do **not** carry an `FxItemId`.
///
/// [`EffectId`] is planned to converge onto this type (JRB-1372) — both are
/// the same `u64` wire shape, so that is a pure type-name migration.
///
/// `#[serde(transparent)]` + `#[ts(type = "number")]`: crosses JSON / ts-rs as
/// a plain number, like [`LayerId`] and [`EffectId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(transparent)]
#[ts(type = "number")]
pub struct FxItemId(u64);

impl FxItemId {
    /// Wraps a raw `u64` as an FX item id.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying `u64`.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl From<u64> for FxItemId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<FxItemId> for u64 {
    fn from(id: FxItemId) -> Self {
        id.0
    }
}

impl fmt::Display for FxItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// A stable composition identifier within an `fx_composition` document.
///
/// A newtype over `Arc<str>` (same backing and clone discipline as
/// [`AssetId`]) so a composition id can never be transposed with any other
/// string — an asset id, a layer name, a font family. Unlike [`AssetId`] it
/// carries no path-traversal validation: composition ids are opaque tokens,
/// never used to construct a filesystem path.
///
/// `#[serde(transparent)]` + `#[ts(type = "string")]`: crosses JSON and ts-rs
/// as a plain string.
#[derive(Clone, PartialEq, Eq, Hash, TS)]
#[ts(type = "string")]
pub struct CompositionId(Arc<str>);

// Manual serde impls mirror `AssetId`: `Arc<str>` only derives serde behind
// the `rc` feature, which we deliberately keep off workspace-wide.
impl Serialize for CompositionId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CompositionId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(|s| Self(Arc::from(s)))
    }
}

impl CompositionId {
    /// Wraps any string-like value as a composition id.
    pub fn new(id: impl Into<Arc<str>>) -> Self {
        Self(id.into())
    }

    /// Borrow the underlying string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for CompositionId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for CompositionId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CompositionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for CompositionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CompositionId({:?})", self.0)
    }
}

// No inbound `From<&str>` / `From<String>` / `From<Arc<str>>`, matching
// `AssetId`: `.into()` would let `let c: CompositionId = layer_name.into();`
// compile silently — exactly the swap the brand exists to catch. Construct
// through `CompositionId::new`.

impl PartialEq<str> for CompositionId {
    fn eq(&self, other: &str) -> bool {
        &*self.0 == other
    }
}

impl PartialEq<&str> for CompositionId {
    fn eq(&self, other: &&str) -> bool {
        &*self.0 == *other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_constructor_accepts_plain_id() {
        let id = AssetId::new("SunwuOWiSc0mUGcXkfVO").expect("valid flat id");
        assert_eq!(id.as_str(), "SunwuOWiSc0mUGcXkfVO");
    }

    #[test]
    fn flat_constructor_rejects_traversal_and_empty() {
        assert_eq!(AssetId::new(""), Err(InvalidAssetId::Empty));
        assert!(matches!(
            AssetId::new("a/b"),
            Err(InvalidAssetId::PathTraversal { .. })
        ));
        assert!(matches!(
            AssetId::new("../etc/passwd"),
            Err(InvalidAssetId::PathTraversal { .. })
        ));
        assert!(matches!(
            AssetId::new("a\\b"),
            Err(InvalidAssetId::PathTraversal { .. })
        ));
        assert!(matches!(
            AssetId::new("evil\0x"),
            Err(InvalidAssetId::NullByte { .. })
        ));
    }

    #[test]
    fn namespaced_constructor_allows_segments_but_not_traversal() {
        assert!(AssetId::new_namespaced("emojis/foo").is_ok());
        assert!(matches!(
            AssetId::new_namespaced("/leading"),
            Err(InvalidAssetId::PathTraversal { .. })
        ));
        assert!(matches!(
            AssetId::new_namespaced("a//b"),
            Err(InvalidAssetId::PathTraversal { .. })
        ));
        assert!(matches!(
            AssetId::new_namespaced("a/../b"),
            Err(InvalidAssetId::PathTraversal { .. })
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_validation_rejects_drive_prefixed_ids() {
        for id in ["C:payload", "z:payload"] {
            assert!(matches!(
                AssetId::new(id),
                Err(InvalidAssetId::PathTraversal { .. })
            ));
        }
        for id in ["C:/payload", "z:/namespace/payload"] {
            assert!(matches!(
                AssetId::new_namespaced(id),
                Err(InvalidAssetId::PathTraversal { .. })
            ));
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_validation_rejects_ntfs_ads() {
        assert!(matches!(
            AssetId::new("asset:stream"),
            Err(InvalidAssetId::PathTraversal { .. })
        ));
        assert!(matches!(
            AssetId::new_namespaced("emojis/asset:stream"),
            Err(InvalidAssetId::PathTraversal { .. })
        ));
    }

    #[cfg(not(windows))]
    #[test]
    fn colon_ids_remain_legal_off_windows() {
        assert!(AssetId::new("asset:revision").is_ok());
        assert!(AssetId::new_namespaced("emojis/asset:revision").is_ok());
    }

    #[test]
    fn deserialize_is_transparent_and_non_failing() {
        // Even a "namespaced" id round-trips through serde without rejection,
        // matching today's validate-at-use-site behavior.
        let id: AssetId = serde_json::from_str("\"emojis/foo\"").expect("transparent string");
        assert_eq!(id.as_str(), "emojis/foo");
        let json = serde_json::to_string(&id).expect("serialize");
        assert_eq!(json, "\"emojis/foo\"");
    }

    #[test]
    fn borrow_enables_str_keyed_lookup() {
        use std::collections::HashMap;
        let mut map: HashMap<AssetId, u32> = HashMap::new();
        map.insert(AssetId::from_trusted("vid"), 7);
        // `Borrow<str>` lets a `&str` query a map keyed by `AssetId`.
        assert_eq!(map.get("vid"), Some(&7));
    }

    #[test]
    fn clone_shares_backing_allocation() {
        let a = AssetId::from_trusted("shared");
        let b = a.clone();
        assert_eq!(a, b);
        assert!(std::ptr::eq(a.as_str().as_ptr(), b.as_str().as_ptr()));
    }

    #[test]
    fn layer_id_roundtrips_as_transparent_number() {
        let id = LayerId::new(42);
        assert_eq!(id.value(), 42);
        assert_eq!(serde_json::to_string(&id).expect("serialize"), "42");
        let parsed: LayerId = serde_json::from_str("42").expect("transparent number");
        assert_eq!(parsed, id);
        assert_eq!(id.to_string(), "42");
    }

    #[test]
    fn layer_id_round_trips_through_u64_conversions() {
        let id = LayerId::from(7u64);
        assert_eq!(u64::from(id), 7);
    }

    #[test]
    fn composition_id_roundtrips_as_transparent_string() {
        let id = CompositionId::new("fx-1");
        assert_eq!(id.as_str(), "fx-1");
        assert_eq!(serde_json::to_string(&id).expect("serialize"), "\"fx-1\"");
        let parsed: CompositionId = serde_json::from_str("\"fx-1\"").expect("transparent string");
        assert_eq!(parsed, id);
        assert_eq!(id, "fx-1");
    }

    #[test]
    fn composition_id_borrow_enables_str_keyed_lookup() {
        use std::collections::HashMap;
        let mut map: HashMap<CompositionId, u32> = HashMap::new();
        map.insert(CompositionId::new("fx-1"), 9);
        assert_eq!(map.get("fx-1"), Some(&9));
    }

    #[test]
    fn composition_id_clone_shares_backing_allocation() {
        let a = CompositionId::new("shared");
        let b = a.clone();
        assert_eq!(a, b);
        assert!(std::ptr::eq(a.as_str().as_ptr(), b.as_str().as_ptr()));
    }
}
