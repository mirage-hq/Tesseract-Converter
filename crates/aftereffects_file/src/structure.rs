//! Best-effort structural loading for AEP project items and real timeline layers.
//!
//! This module deliberately does not interpret property trees or validate
//! reference graphs. It retains unsupported layer content for later conversion
//! diagnostics while keeping the strict empty-project reader unchanged.

#[path = "solid.rs"]
mod solid;
pub use crate::media::{
    MediaDecodeError, MediaDescriptor, MediaDuration, MediaFrameRate, MediaKind, PhotoshopSource,
};
pub use solid::{SolidDecodeError, SolidSource};

use std::{collections::HashSet, mem, str::Utf8Error, sync::Arc};

use thiserror::Error;

use crate::{
    aep,
    rifx::{Chunk, RifxError},
    schema::{CompositionRecord, HeadRecord, ItemRecord, RecordError, layer_records::LayerRecord},
};

/// Errors that make the structural project itself unsafe or ambiguous to load.
#[derive(Debug, Error)]
pub enum StructureError {
    /// Malformed or over-limit RIFX framing.
    #[error(transparent)]
    Binary(#[from] RifxError),
    /// A known fixed-layout record is truncated or malformed.
    #[error(transparent)]
    Record(#[from] RecordError),
    /// A required structural chunk is absent, duplicated, or has the wrong shape.
    #[error("invalid After Effects structure: {0}")]
    Invalid(&'static str),
    /// A required name chunk is not UTF-8.
    #[error("invalid UTF-8 in {field}")]
    Utf8 {
        /// Name-bearing field that failed decoding.
        field: &'static str,
        /// Original UTF-8 decoding error.
        #[source]
        source: Utf8Error,
    },
    /// An item ID is duplicated globally or a layer ID within one composition.
    #[error("duplicate After Effects identity {0}")]
    DuplicateIdentity(u32),
    /// Real project items and timeline layers must have nonzero identities.
    #[error("zero After Effects identity in {0}")]
    ZeroIdentity(&'static str),
}

/// A structurally decoded AEP project in file encounter order.
#[derive(Clone, Debug, PartialEq)]
pub struct StructuralProject {
    /// Binary format revision from the `head` record.
    pub format_version: u8,
    /// Project items, recursively flattened in folder/file encounter order.
    pub items: Vec<ProjectItem>,
}

impl StructuralProject {
    /// Looks up a project item by its project-wide identity.
    pub fn item(&self, id: u32) -> Option<&ProjectItem> {
        self.items.iter().find(|item| item.id == id)
    }
}

/// One project-panel item.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectItem {
    /// Project-wide item identity.
    pub id: u32,
    /// Item name from its direct `Utf8` chunk.
    pub name: String,
    /// Containing folder identity, or `None` for a root-folder child.
    pub parent_folder: Option<u32>,
    /// Structurally known item kind.
    pub kind: ItemKind,
    /// Main/proxy source classification for footage items.
    pub footage: Option<FootageClassification>,
    /// Decoded main solid source, or the reason its optional payload could not be used.
    /// `None` means this item does not declare a main solid source.
    pub solid: Option<Result<SolidSource, SolidDecodeError>>,
    /// Decoded main file source, or the reason its optional payload could not be used.
    /// `None` means this item does not declare a main file source.
    pub media: Option<Result<MediaDescriptor, MediaDecodeError>>,
    /// Native source identity and A/V kind, independent of image-specific metadata.
    /// `None` means this item does not declare a main file source.
    pub native_media: Option<Result<MediaDescriptor, MediaDecodeError>>,
}

/// Source classifications evidenced by footage `Pin `/`opti` records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FootageClassification {
    /// Classification of the first (main-source) `Pin ` record.
    pub main_source: FootageSourceKind,
    /// Classification of the optional second (proxy-source) `Pin ` record.
    pub proxy_source: Option<FootageSourceKind>,
}

/// Best-effort footage-source kind, without interpreting its render payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FootageSourceKind {
    /// A file-backed source, including known and unknown importer fourccs.
    File,
    /// An AE solid source (`Soli`).
    Solid,
    /// An AE placeholder source (empty fourcc and discriminator 2).
    Placeholder,
    /// The source discriminator is absent or too short to classify.
    Unknown,
}

/// Project item kinds established by the `idta` type field.
#[derive(Clone, Debug, PartialEq)]
pub enum ItemKind {
    /// A project-panel folder.
    Folder,
    /// A composition and its real timeline layers.
    Composition(Box<Composition>),
    /// A footage item. Supported solid settings are in `ProjectItem::solid`.
    Footage,
    /// An unrecognized item type retained without semantic interpretation.
    Unknown(u16),
}

/// Structural composition metadata and timeline layers.
#[derive(Clone, Debug, PartialEq)]
pub struct Composition {
    /// Canvas width in pixels.
    pub width: u16,
    /// Canvas height in pixels.
    pub height: u16,
    /// Composition duration in seconds.
    pub duration_secs: f64,
    /// Nominal frames per second.
    pub frame_rate: f64,
    /// Exact pixel-aspect numerator and denominator.
    pub pixel_aspect: (u32, u32),
    /// Display-start time in seconds.
    pub display_start_secs: f64,
    /// Real `Layr` entries in their direct file/timeline order.
    pub layers: Vec<Layer>,
    /// Byte-preserving typed composition descriptor for later diagnostics.
    pub record: CompositionRecord,
    /// Source-only Essential Graphics controllers; not persisted FX schema.
    pub(crate) essential_properties: crate::essential::Parsed<crate::essential::Controller>,
}

/// One real timeline layer. Viewer/marker pseudo-layers are not included.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    /// Display name from its direct `Utf8` or source item. Inherited names
    /// share storage so repeated references cannot multiply large allocations.
    pub name: Arc<str>,
    /// Typed, byte-preserving timeline-layer envelope.
    pub record: LayerRecord,
    /// Every direct child chunk, retaining property trees and unknown payloads.
    pub content: Vec<Chunk>,
}

/// Reads the project/item graph and real timeline layer envelopes.
///
/// Unsupported item kinds, layer kinds, property trees, dangling references,
/// and reference cycles are retained rather than rejected. Existing RIFX
/// bounds still apply; malformed required records and duplicate identities are
/// fatal because they make structural lookup ambiguous.
pub fn read_project(bytes: &[u8]) -> Result<StructuralProject, StructureError> {
    read_project_with_header(bytes).map(|(project, _)| project)
}

// Identity resolution needs the observed producer profile, without reparsing or
// changing the public structural model used by ordinary numeric-ID imports.
pub(crate) fn read_project_with_header(
    bytes: &[u8],
) -> Result<(StructuralProject, HeadRecord), StructureError> {
    let mut project = aep::Project::parse(bytes)?;
    let head = HeadRecord::decode(only_data(&project.chunks, *b"head")?)?;
    let root = only_list_mut(&mut project.chunks, *b"Fold")?;
    let mut state = ReadState::default();
    state.read_items(root, None)?;
    state.resolve_inherited_layer_names();
    Ok((
        StructuralProject {
            format_version: head.format_version(),
            items: state.items,
        },
        head,
    ))
}

#[derive(Default)]
struct ReadState {
    item_ids: HashSet<u32>,
    items: Vec<ProjectItem>,
}

impl ReadState {
    fn read_items(
        &mut self,
        container: &mut [Chunk],
        parent_folder: Option<u32>,
    ) -> Result<(), StructureError> {
        let mut pending = Vec::new();
        queue_items(container, parent_folder, &mut pending)?;
        while let Some((mut children, parent_folder)) = pending.pop() {
            if let Some((folder_id, mut folder)) = self.read_item(&mut children, parent_folder)? {
                queue_items(&mut folder, Some(folder_id), &mut pending)?;
            }
        }
        Ok(())
    }

    fn read_item(
        &mut self,
        children: &mut [Chunk],
        parent_folder: Option<u32>,
    ) -> Result<Option<(u32, Vec<Chunk>)>, StructureError> {
        let record = ItemRecord::decode(only_data(children, *b"idta")?)?;
        let id = record.id();
        self.register_item_identity(id)?;
        let mut name = required_utf8(children, "item name")?.to_owned();
        let item_type = record.item_type();
        if item_type == 7 {
            name = footage_name(children, &name)?;
        }
        let kind = match item_type {
            1 => ItemKind::Folder,
            4 => ItemKind::Composition(Box::new(self.read_composition(children)?)),
            7 => ItemKind::Footage,
            unknown => ItemKind::Unknown(unknown),
        };
        let footage = (item_type == 7).then(|| classify_footage(children));
        let main_source = children
            .iter()
            .find(|chunk| chunk.list_kind() == Some(*b"Pin "));
        let solid = if footage.is_some_and(|source| source.main_source == FootageSourceKind::Solid)
        {
            main_source.map(solid::decode)
        } else {
            None
        };
        let media = if footage.is_some_and(|source| source.main_source == FootageSourceKind::File) {
            main_source.map(crate::media::decode)
        } else {
            None
        };
        let native_media =
            if footage.is_some_and(|source| source.main_source == FootageSourceKind::File) {
                main_source.map(crate::media::decode_native)
            } else {
                None
            };
        self.items.push(ProjectItem {
            id,
            name,
            parent_folder,
            kind,
            footage,
            solid,
            media,
            native_media,
        });

        if item_type == 1 {
            let folder = mem::take(only_list_children_mut(children, *b"Sfdr")?);
            return Ok(Some((id, folder)));
        }
        Ok(None)
    }

    fn read_composition(&mut self, children: &mut [Chunk]) -> Result<Composition, StructureError> {
        let record = CompositionRecord::decode(only_data(children, *b"cdta")?)?;
        let (duration_numerator, duration_denominator) = record.duration_fraction()?;
        let (display_numerator, display_denominator) = record.display_start_fraction();
        if display_denominator == 0 {
            return Err(StructureError::Invalid(
                "zero composition display-start divisor",
            ));
        }
        let essential_properties = crate::essential::controllers(children);
        let mut layer_ids = HashSet::new();
        let mut layers = Vec::new();
        for chunk in children
            .iter_mut()
            .filter(|chunk| chunk.list_kind() == Some(*b"Layr"))
        {
            let layer = Self::read_layer(chunk)?;
            let id = layer.record.id();
            if id == 0 {
                return Err(StructureError::ZeroIdentity("timeline layer"));
            }
            if !layer_ids.insert(id) {
                return Err(StructureError::DuplicateIdentity(id));
            }
            layers.push(layer);
        }
        let (width, height) = record.dimensions();
        Ok(Composition {
            width,
            height,
            duration_secs: f64::from(duration_numerator) / f64::from(duration_denominator),
            frame_rate: record.frame_rate(),
            pixel_aspect: record.pixel_aspect_fraction(),
            display_start_secs: f64::from(display_numerator) / f64::from(display_denominator),
            layers,
            record,
            essential_properties,
        })
    }

    fn read_layer(chunk: &mut Chunk) -> Result<Layer, StructureError> {
        let children = chunk
            .children_mut()
            .ok_or(StructureError::Invalid("Layr LIST is opaque"))?;
        let record = LayerRecord::decode(only_data(children, *b"ldta")?)?;
        let name = Arc::from(required_utf8(children, "layer name")?);
        Ok(Layer {
            name,
            record,
            content: mem::take(children),
        })
    }

    fn register_item_identity(&mut self, id: u32) -> Result<(), StructureError> {
        if id == 0 {
            return Err(StructureError::ZeroIdentity("project item"));
        }
        if !self.item_ids.insert(id) {
            return Err(StructureError::DuplicateIdentity(id));
        }
        Ok(())
    }

    fn resolve_inherited_layer_names(&mut self) {
        let item_names = self
            .items
            .iter()
            .map(|item| (item.id, Arc::<str>::from(item.name.as_str())))
            .collect::<std::collections::HashMap<_, _>>();
        for item in &mut self.items {
            let ItemKind::Composition(composition) = &mut item.kind else {
                continue;
            };
            for layer in &mut composition.layers {
                if layer.name.is_empty() {
                    layer.name = item_names
                        .get(&layer.record.source_id())
                        .cloned()
                        .unwrap_or_default();
                }
            }
        }
    }
}

fn only_data(chunks: &[Chunk], id: [u8; 4]) -> Result<&[u8], StructureError> {
    let mut matches = chunks.iter().filter(|chunk| chunk.id() == id);
    let chunk = matches
        .next()
        .ok_or(StructureError::Invalid("required data chunk is missing"))?;
    if matches.next().is_some() {
        return Err(StructureError::Invalid("duplicate required data chunk"));
    }
    chunk
        .data_payload()
        .ok_or(StructureError::Invalid("required chunk is not data"))
}

fn queue_items(
    container: &mut [Chunk],
    parent_folder: Option<u32>,
    pending: &mut Vec<(Vec<Chunk>, Option<u32>)>,
) -> Result<(), StructureError> {
    for item_chunk in container
        .iter_mut()
        .rev()
        .filter(|chunk| chunk.list_kind() == Some(*b"Item"))
    {
        let children = item_chunk
            .children_mut()
            .ok_or(StructureError::Invalid("Item LIST is opaque"))?;
        pending.push((mem::take(children), parent_folder));
    }
    Ok(())
}

fn only_list_children_mut(
    chunks: &mut [Chunk],
    kind: [u8; 4],
) -> Result<&mut Vec<Chunk>, StructureError> {
    let mut matches = chunks
        .iter_mut()
        .filter(|chunk| chunk.list_kind() == Some(kind));
    let chunk = matches
        .next()
        .ok_or(StructureError::Invalid("required LIST is missing"))?;
    if matches.next().is_some() {
        return Err(StructureError::Invalid("duplicate required LIST"));
    }
    chunk
        .children_mut()
        .ok_or(StructureError::Invalid("required LIST is opaque"))
}

fn only_list_mut(chunks: &mut [Chunk], kind: [u8; 4]) -> Result<&mut [Chunk], StructureError> {
    only_list_children_mut(chunks, kind).map(Vec::as_mut_slice)
}

fn required_utf8<'a>(chunks: &'a [Chunk], field: &'static str) -> Result<&'a str, StructureError> {
    // Ordered Utf8 siblings have different meanings. py-aep selects the first
    // direct name chunk; solid/file items can legally contain more than one.
    let bytes = chunks
        .iter()
        .find(|chunk| chunk.id() == *b"Utf8")
        .and_then(Chunk::data_payload)
        .ok_or(StructureError::Invalid("required name chunk is missing"))?;
    std::str::from_utf8(bytes).map_err(|source| StructureError::Utf8 { field, source })
}

fn footage_name(chunks: &[Chunk], item_name: &str) -> Result<String, StructureError> {
    let Some(pin) = chunks
        .iter()
        .find(|chunk| chunk.list_kind() == Some(*b"Pin "))
    else {
        return Ok(item_name.to_owned());
    };
    let Some(children) = pin.children() else {
        return Ok(item_name.to_owned());
    };
    let kind = classify_footage_source(pin);
    let offset = match kind {
        FootageSourceKind::Solid => Some(26),
        FootageSourceKind::Placeholder => Some(10),
        _ => None,
    };
    if let Some(offset) = offset
        && let Some(bytes) = children
            .iter()
            .find(|chunk| chunk.id() == *b"opti")
            .and_then(Chunk::data_payload)
            .and_then(|bytes| bytes.get(offset..offset + 256))
    {
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        return std::str::from_utf8(&bytes[..end])
            .map(str::to_owned)
            .map_err(|source| StructureError::Utf8 {
                field: "footage source name",
                source,
            });
    }
    if !item_name.is_empty() {
        return Ok(item_name.to_owned());
    }
    // Read alias metadata only. Never open its path or contact remote media.
    let alias = children
        .iter()
        .find(|chunk| chunk.list_kind() == Some(*b"Als2"))
        .and_then(Chunk::children)
        .and_then(|chunks| chunks.iter().find(|chunk| chunk.id() == *b"alas"))
        .and_then(Chunk::data_payload);
    if let Some(alias) = alias
        && let Ok(metadata) = crate::alias::decode(alias)
        && let Some(path) = metadata.fullpath
    {
        return Ok(path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .to_owned());
    }
    Ok(item_name.to_owned())
}

fn classify_footage(chunks: &[Chunk]) -> FootageClassification {
    let sources = chunks
        .iter()
        .filter(|chunk| chunk.list_kind() == Some(*b"Pin "))
        .map(classify_footage_source)
        .collect::<Vec<_>>();
    FootageClassification {
        main_source: sources
            .first()
            .copied()
            .unwrap_or(FootageSourceKind::Unknown),
        proxy_source: sources.get(1).copied(),
    }
}

fn classify_footage_source(pin: &Chunk) -> FootageSourceKind {
    let Some(children) = pin.children() else {
        return FootageSourceKind::Unknown;
    };
    let mut opti = children
        .iter()
        .filter(|chunk| chunk.id() == *b"opti")
        .filter_map(Chunk::data_payload);
    let Some(bytes) = opti.next() else {
        return FootageSourceKind::Unknown;
    };
    if opti.next().is_some() {
        return FootageSourceKind::Unknown;
    }
    // Native model/file importers can emit a present but empty opti. This is
    // py-aep's generic FileSource case, not a truncated solid descriptor.
    if bytes.is_empty() {
        return FootageSourceKind::File;
    }
    if bytes.len() < 6 {
        return FootageSourceKind::Unknown;
    }
    let kind = &bytes[..4];
    if kind == b"Soli" {
        FootageSourceKind::Solid
    } else if kind == [0; 4] && u16::from_be_bytes([bytes[4], bytes[5]]) == 2 {
        FootageSourceKind::Placeholder
    } else {
        FootageSourceKind::File
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FootageSourceKind, ItemKind, ReadState, StructureError, footage_name, read_project,
    };
    use crate::{aep, rifx::Chunk};

    fn list_mut(chunks: &mut [Chunk], kind: [u8; 4]) -> &mut Vec<Chunk> {
        chunks
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(kind))
            .unwrap()
            .children_mut()
            .unwrap()
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }

    fn layer(id: u32, source_id: u32, name: &str, marker: u8) -> Chunk {
        let mut ldta = vec![marker; 164];
        put_u32(&mut ldta, 0, id);
        put_u32(&mut ldta, 16, 1);
        put_u32(&mut ldta, 24, 1);
        put_u32(&mut ldta, 32, 1);
        put_u32(&mut ldta, 40, source_id);
        put_u32(&mut ldta, 108, 1);
        Chunk::list(
            *b"Layr",
            vec![
                Chunk::data(*b"ldta", ldta).unwrap(),
                Chunk::data(*b"Utf8", name.as_bytes()).unwrap(),
                Chunk::list(*b"tdgp", vec![Chunk::data(*b"zzzz", vec![marker]).unwrap()]),
            ],
        )
    }

    #[test]
    fn read_layer_transfers_nested_unknown_payload_without_cloning() {
        let marker = 0x7a;
        let mut chunk = layer(1, 0, "Layer", marker);
        let payload_ptr = chunk
            .children()
            .unwrap()
            .iter()
            .find(|child| child.list_kind() == Some(*b"tdgp"))
            .unwrap()
            .children()
            .unwrap()[0]
            .data_payload()
            .unwrap()
            .as_ptr();

        let layer = ReadState::read_layer(&mut chunk).unwrap();
        let transferred_payload = layer
            .content
            .iter()
            .find(|child| child.list_kind() == Some(*b"tdgp"))
            .unwrap()
            .children()
            .unwrap()[0]
            .data_payload()
            .unwrap();

        assert_eq!(transferred_payload, [marker]);
        assert_eq!(transferred_payload.as_ptr(), payload_ptr);
    }

    #[test]
    fn review_import_large_unrelated_alias_fields_are_skipped() {
        let alias_chunks = |json| {
            vec![Chunk::list(
                *b"Pin ",
                vec![Chunk::list(
                    *b"Als2",
                    vec![Chunk::data(*b"alas", json).unwrap()],
                )],
            )]
        };

        let escaped = serde_json::to_vec(&serde_json::json!({
            "unrelated": "x".repeat(1024 * 1024),
            "fullpath": r"C:\source\clip.mov",
        }))
        .unwrap();
        assert_eq!(
            footage_name(&alias_chunks(escaped), "").unwrap(),
            "clip.mov",
            "skipping a large unknown field must preserve escaped path semantics"
        );
        for (json, expected) in [
            (
                br#"{"fullpath":"C:\\source\\clip.mov"}"#.as_slice(),
                "clip.mov",
            ),
            (
                br#"{"fullpath":"\/tmp\/caf\u00e9.mov"}"#.as_slice(),
                "café.mov",
            ),
        ] {
            assert_eq!(
                footage_name(&alias_chunks(json.to_vec()), "").unwrap(),
                expected,
                "valid escaped aliases must remain supported"
            );
        }

        let oversized_name = "x".repeat(64 * 1024 + 1);
        let oversized = serde_json::to_vec(&serde_json::json!({
            "fullpath": oversized_name,
        }))
        .unwrap();
        assert_eq!(
            footage_name(&alias_chunks(oversized), "").unwrap(),
            oversized_name,
            "an alias name above the former retained-string limit is retained"
        );
    }

    #[test]
    fn reads_independent_composition_without_view_pseudo_layers() {
        let project = read_project(include_bytes!("../tests/fixtures/ae26_one_comp.aep")).unwrap();
        assert_eq!(project.format_version, 97);
        let item = project.item(1).unwrap();
        assert_eq!(
            (item.name.as_str(), item.parent_folder),
            ("classic-3d", None)
        );
        let ItemKind::Composition(comp) = &item.kind else {
            panic!("fixture item must be a composition");
        };
        assert_eq!((comp.width, comp.height), (1920, 1080));
        assert_eq!(comp.duration_secs, 1.0);
        assert_eq!(comp.frame_rate, 24.0);
        assert!(comp.layers.is_empty());
    }

    #[test]
    fn reads_independent_layer_and_source_classification_fixture() {
        let project = read_project(include_bytes!("../tests/fixtures/layers/type.aep")).unwrap();
        let footage = project.item(14).unwrap();
        assert_eq!(footage.name, "Null 1");
        assert_eq!(footage.parent_folder, Some(13));
        assert_eq!(
            footage.footage.unwrap().main_source,
            FootageSourceKind::Solid
        );
        let solid = footage.solid.as_ref().unwrap().as_ref().unwrap();
        assert_eq!((solid.width, solid.height), (100, 100));
        assert_eq!(solid.pixel_aspect, (1, 1));
        assert_eq!(solid.color, [1.0, 1.0, 1.0]);

        let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
            panic!("fixture item 1 must be a composition");
        };
        assert_eq!(comp.layers.len(), 1);
        let layer = &comp.layers[0];
        assert_eq!((layer.record.id(), layer.record.source_id()), (15, 14));
        assert_eq!(layer.name.as_ref(), "Null 1");
        assert!(layer.record.flags().null_layer);

        let ItemKind::Composition(camera_comp) = &project.item(42).unwrap().kind else {
            panic!("fixture item 42 must be a composition");
        };
        assert_eq!(camera_comp.layers[0].record.layer_type(), 2);
        assert_eq!(camera_comp.layers[0].name.as_ref(), "CameraLayer");
    }

    #[test]
    fn reads_pinned_native_file_audio_and_sequence_descriptors() {
        let still = read_project(include_bytes!(
            "../tests/fixtures/media/footage_not_missing.aep"
        ))
        .unwrap();
        let still = still
            .item(1)
            .unwrap()
            .media
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap();
        assert_eq!(still.kind, super::MediaKind::StillImage);
        assert_eq!((still.width, still.height), (200, 200));
        assert_eq!(still.duration.seconds(), 0.0);
        assert!(!still.missing_at_save);
        let missing = read_project(include_bytes!(
            "../tests/fixtures/media/footage_missing.aep"
        ))
        .unwrap();
        assert!(
            missing
                .item(1)
                .unwrap()
                .media
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .missing_at_save
        );
        assert!(
            still
                .authored_path
                .ends_with("sample_motionblur_transparency.exr")
        );

        let audio =
            read_project(include_bytes!("../tests/fixtures/media/audioEnabled.aep")).unwrap();
        let audio = audio
            .item(13)
            .unwrap()
            .media
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap();
        assert_eq!(audio.kind, super::MediaKind::Audio);
        assert_eq!((audio.width, audio.height), (0, 0));
        assert!(audio.audio_sample_rate > 0.0);
        assert!((audio.duration.seconds() - 5.943_174_603_174_6).abs() < 1e-12);

        let sequence =
            read_project(include_bytes!("../tests/fixtures/media/imio_sequence.aep")).unwrap();
        assert!(sequence.items.iter().any(|item| {
            item.media.as_ref().is_some_and(|media| {
                media
                    .as_ref()
                    .is_ok_and(|media| media.kind == super::MediaKind::ImageSequence)
            })
        }));
    }

    #[test]
    fn malformed_solid_settings_do_not_abort_other_items() {
        let mut project =
            aep::Project::parse(include_bytes!("../tests/fixtures/layers/type.aep")).unwrap();
        fn corrupt_solid(chunks: &mut [Chunk]) -> bool {
            for chunk in chunks {
                if chunk.list_kind() == Some(*b"Pin ") {
                    let children = chunk.children_mut().unwrap();
                    if children.iter().any(|child| {
                        child.id() == *b"opti"
                            && child
                                .data_payload()
                                .is_some_and(|data| data.starts_with(b"Soli"))
                    }) {
                        children.retain(|child| child.id() != *b"sspc");
                        return true;
                    }
                }
                if let Some(children) = chunk.children_mut()
                    && corrupt_solid(children)
                {
                    return true;
                }
            }
            false
        }
        assert!(corrupt_solid(&mut project.chunks));
        let loaded = read_project(&project.encode().unwrap()).unwrap();
        assert_eq!(
            loaded.item(14).unwrap().solid,
            Some(Err(super::SolidDecodeError::Chunk("sspc")))
        );
        assert!(matches!(
            loaded.item(1).unwrap().kind,
            ItemKind::Composition(_)
        ));
    }

    #[test]
    fn reads_independent_nested_folder_graph_fixture() {
        let project = read_project(include_bytes!("../tests/fixtures/layers/folder.aep")).unwrap();
        assert_eq!(project.item(10).unwrap().parent_folder, None);
        assert_eq!(project.item(11).unwrap().parent_folder, Some(10));
        assert_eq!(project.item(23).unwrap().parent_folder, Some(10));
        assert_eq!(project.item(35).unwrap().parent_folder, Some(10));
    }

    #[test]
    fn structural_reader_handles_deep_folder_nesting_on_a_small_stack() {
        const DEPTH: u32 = 2_048;
        std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(|| {
                let mut envelope =
                    aep::Project::parse(include_bytes!("../tests/fixtures/ae26_one_comp.aep"))
                        .unwrap();
                let root = list_mut(&mut envelope.chunks, *b"Fold");
                let leaf = root
                    .iter_mut()
                    .find(|chunk| chunk.list_kind() == Some(*b"Item"))
                    .unwrap()
                    .clone();
                let mut nested = vec![leaf];
                for id in (2..DEPTH + 2).rev() {
                    let mut idta = vec![0; 84];
                    idta[0..2].copy_from_slice(&1_u16.to_be_bytes());
                    put_u32(&mut idta, 16, id);
                    nested = vec![Chunk::list(
                        *b"Item",
                        vec![
                            Chunk::data(*b"idta", idta).unwrap(),
                            Chunk::data(*b"Utf8", format!("folder-{id}")).unwrap(),
                            Chunk::list(*b"Sfdr", nested),
                        ],
                    )];
                }
                *root = nested;

                let project = read_project(&envelope.encode().unwrap()).unwrap();
                assert_eq!(project.items.len(), usize::try_from(DEPTH).unwrap() + 1);
                assert_eq!(project.items[0].parent_folder, None);
                assert_eq!(project.items[1].parent_folder, Some(2));
                assert_eq!(project.item(1).unwrap().parent_folder, Some(DEPTH + 1));
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn retains_real_layer_order_forward_references_and_unknown_content() {
        let mut project =
            aep::Project::parse(include_bytes!("../tests/fixtures/ae26_one_comp.aep")).unwrap();
        let root = list_mut(&mut project.chunks, *b"Fold");
        let item = list_mut(root, *b"Item");
        item.push(layer(100, 999, "top", 0xa1));
        item.push(layer(101, 1, "", 0xb2));

        let structure = read_project(&project.encode().unwrap()).unwrap();
        let ItemKind::Composition(comp) = &structure.item(1).unwrap().kind else {
            panic!("fixture item must be a composition");
        };
        assert_eq!(
            comp.layers
                .iter()
                .map(|layer| (
                    layer.name.as_ref(),
                    layer.record.id(),
                    layer.record.source_id()
                ))
                .collect::<Vec<_>>(),
            [("top", 100, 999), ("classic-3d", 101, 1)]
        );
        assert_eq!(
            comp.layers[0].content[2].children().unwrap()[0].data_payload(),
            Some(&[0xa1][..])
        );
    }

    #[test]
    fn rejects_duplicate_real_identities_but_not_dangling_references() {
        let mut project =
            aep::Project::parse(include_bytes!("../tests/fixtures/ae26_one_comp.aep")).unwrap();
        let root = list_mut(&mut project.chunks, *b"Fold");
        let item = list_mut(root, *b"Item");
        item.push(layer(100, 999, "one", 0));
        item.push(layer(100, 998, "two", 0));
        assert!(matches!(
            read_project(&project.encode().unwrap()),
            Err(StructureError::DuplicateIdentity(100))
        ));
    }
}
