//! Original AE26 empty-composition root/item construction.
//!
//! This grammar opened and resaved two empty projects in AE26.5, without
//! an Adobe-authored template or XMP. That bounded acceptance evidence does
//! not establish compatibility for other AE versions or nonempty timelines.

use crate::{
    aep::Project,
    rifx::Chunk,
    schema::{
        CompositionRecord, HeadRecord, ItemRecord,
        panel_records::{EmptyListHeader, PanelSlots, RenderQueueHeader},
        project_settings::{CompactProjectSettings, ExpandedProjectSettings},
    },
    timing::Duration24,
};

// All call sites are the literal non-LIST tags in this private AE26 recipe;
// neither a file nor caller input controls the tag.
fn raw(kind: [u8; 4], bytes: impl Into<Vec<u8>>) -> Chunk {
    Chunk::data(kind, bytes).expect("writer raw chunk identifiers are never LIST")
}

fn list(kind: [u8; 4], children: Vec<Chunk>) -> Chunk {
    Chunk::list(kind, children)
}

/// Fresh AE26 item-envelope state. Timeline entries carry two envelopes after
/// Ewst; project-folder entries carry one. Omitting the timeline envelopes makes
/// Adobe stop at the first layer even though our structural reader sees more.
/// Values/order are pinned by the independently authored two-layer regression.
pub(super) fn item_envelope_tail() -> [Chunk; 7] {
    [
        raw(*b"fvdv", 3_u32.to_be_bytes()),
        raw(*b"fiop", vec![0]),
        raw(*b"ftts", 0_u32.to_be_bytes()),
        raw(*b"foac", vec![0]),
        raw(*b"fiac", vec![0]),
        raw(*b"fipc", 0_u16.to_be_bytes()),
        raw(*b"fifl", 0_u32.to_be_bytes()),
    ]
}

/// Builds the known ordered root/item subset. `views` must be the canonical
/// eleven pseudo-layers and their own panel metadata; IDs 2..12 are reserved
/// for them, so the next item ID in `head` is 13. This contract has not yet
/// been tested by opening the resulting file in After Effects.
pub(super) fn build_project(
    name: &str,
    width: u16,
    height: u16,
    duration: Duration24,
    views: Vec<Chunk>,
) -> Result<Project, super::AepWriteError> {
    build_project_with_timeline(name, width, height, duration, views, Timeline::default())
}

/// IDs 1..=12 belong to the composition and its working views.
#[derive(Debug)]
pub(super) struct Timeline {
    pub sources: Vec<Chunk>,
    pub layers: Vec<Chunk>,
    pub next_id: u32,
    pub frame_blending_master: bool,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            layers: Vec::new(),
            next_id: 13,
            frame_blending_master: false,
        }
    }
}

/// Builds one fresh composition project item. Root and nested compositions use
/// the same typed grammar; only the allocated item ID, record, views and layer
/// list differ.
pub(super) fn composition_item(
    id: u32,
    name: &str,
    composition: CompositionRecord,
    mut layers: Vec<Chunk>,
    views: Vec<Chunk>,
) -> Result<Chunk, super::AepWriteError> {
    if name.is_empty() || name.len() > 65_535 || name.contains('\0') {
        return Err(super::AepWriteError::Invalid(
            "composition name must be 1..=65535 UTF-8 bytes without NUL",
        ));
    }
    let item = ItemRecord::empty_ae26_composition(id)?;
    // `iide` follows the project-item identity in every fresh source item.
    // `idpc` and the empty stream count are stable AE26 composition defaults.
    let mut children = vec![
        raw(*b"iide", id.to_le_bytes()),
        raw(*b"idpc", 0_u64.to_be_bytes()),
        raw(*b"idta", item.encode()),
        raw(*b"Utf8", name.as_bytes().to_vec()),
        list(*b"dats", vec![raw(*b"numS", 0_u32.to_be_bytes())]),
        raw(*b"cdta", composition.encode()),
        raw(*b"cdrp", vec![0]),
        raw(*b"comr", vec![0]),
        classic_renderer(),
    ];
    children.append(&mut layers);
    children.extend(views);
    Ok(list(*b"Item", children))
}

pub(super) fn build_project_with_timeline(
    name: &str,
    width: u16,
    height: u16,
    duration: Duration24,
    views: Vec<Chunk>,
    timeline: Timeline,
) -> Result<Project, super::AepWriteError> {
    build_project_with_timeline_options(
        name,
        width,
        height,
        duration,
        views,
        timeline,
        None,
        crate::timing::FrameRate::new(24.0)?,
    )
}

// Root sampling rate and motion settings are independent native composition fields.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_project_with_timeline_options(
    name: &str,
    width: u16,
    height: u16,
    duration: Duration24,
    views: Vec<Chunk>,
    timeline: Timeline,
    composition_options: Option<super::CompositionOptions>,
    frame_rate: crate::timing::FrameRate,
) -> Result<Project, super::AepWriteError> {
    let Timeline {
        sources,
        layers,
        next_id,
        frame_blending_master,
    } = timeline;
    let mut composition = CompositionRecord::empty_ae26(width, height, duration)?;
    composition.set_frame_rate(frame_rate);
    if let Some(options) = composition_options {
        super::composition_options::apply(&mut composition, options)?;
    }
    composition.set_frame_blending(frame_blending_master);
    let item = composition_item(
        crate::export_identity::GENERATED_ROOT_ITEM_ID,
        name,
        composition,
        layers,
        views,
    )?;
    let head = HeadRecord::empty_ae26(next_id);

    // An unparented item lives directly in the one root folder. The folder
    // metadata is a zero/default declaration, not a copied fixture subtree.
    let mut folder_data = [0; 14];
    folder_data[12] = 1; // Stable AE26 root-folder default, not an item ID.
    let mut folder_children = vec![raw(*b"fdta", folder_data)];
    for item in std::iter::once(item).chain(sources) {
        let item_record = item
            .children()
            .and_then(|children| children.iter().find(|child| child.id() == *b"idta"))
            .and_then(Chunk::data_payload)
            .ok_or(super::AepWriteError::Invalid("fresh item has no idta"))?;
        let is_composition = ItemRecord::decode(item_record)?.item_type() == 4;
        folder_children.push(item);
        if is_composition {
            // Native compositions have panel metadata; footage items do not.
            // ppSn is a big-endian f64 panel value, zero in fresh projects.
            folder_children.push(list(*b"FEE ", vec![raw(*b"ppSn", 0.0_f64.to_be_bytes())]));
        }
        // These records terminate each item, not the entire folder. Without
        // them AE skips subsequent source items and substitutes placeholders.
        folder_children.extend(item_envelope_tail());
    }
    let folder = list(*b"Fold", folder_children);
    let mut project = Project {
        chunks: vec![
            raw(*b"svap", head.producer_version_word().to_be_bytes()),
            raw(*b"head", head.encode()),
            raw(*b"nhed", CompactProjectSettings::ae26_default().encode()),
            raw(*b"nnhd", ExpandedProjectSettings::ae26_default().encode()),
            raw(*b"adfr", 48_000.0_f64.to_be_bytes()),
            list(*b"Pefl", vec![]),
            raw(*b"qtlg", vec![0]),
            // Stable AE backend identifier, not a per-project GUID. Software
            // avoids requiring the authoring host's Metal backend at runtime.
            list(
                *b"gpuG",
                vec![raw(
                    *b"Utf8",
                    b"f33089e2-1ede-47c1-8a9e-b232bb1cc1a4".to_vec(),
                )],
            ),
            list(
                *b"sfnm",
                vec![
                    raw(*b"Utf8", b"Solids".to_vec()),
                    raw(*b"sfid", 0_u32.to_be_bytes()),
                ],
            ),
            raw(*b"mrid", 0_u32.to_be_bytes()),
            raw(*b"acer", vec![1]),
            list(*b"CPPl", vec![]),
            raw(*b"cpid", [255; 16]), // No assigned color profile ID.
            raw(*b"dwga", 0x01000000_u32.to_be_bytes()),
            raw(*b"pcms", vec![1]),
            raw(*b"Utf8", br#"{"lutInterpolationMethod":1}"#.to_vec()),
            raw(*b"PwCs", vec![1]),
            raw(*b"Utf8", b"{}".to_vec()),
            raw(*b"pdvc", vec![1]),
            raw(*b"Utf8", b"{}".to_vec()),
            list(*b"ExEn", vec![raw(*b"Utf8", b"javascript-1.0".to_vec())]),
            folder,
            // Workspace label only, not the composition's user-visible name.
            raw(*b"wsns", 14_u16.to_be_bytes()),
            raw(
                *b"wsnm",
                "Default"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            ),
            raw(*b"Utf8", b"Default".to_vec()),
            raw(*b"fcid", 0_u32.to_be_bytes()),
            raw(*b"oacc", 0_u16.to_be_bytes()),
            list(
                *b"LSIf",
                vec![raw(*b"AFsi", PanelSlots::project_default().encode())],
            ),
            list(
                *b"LRdr",
                vec![
                    raw(*b"Rhed", RenderQueueHeader::empty_ae26().encode()),
                    raw(*b"Rout", 0_u32.to_be_bytes()),
                    list(
                        *b"list",
                        vec![raw(*b"lhd3", EmptyListHeader::render_queue().encode())],
                    ),
                    list(*b"LItm", vec![]),
                    list(
                        *b"LSIf",
                        vec![raw(*b"ARsi", PanelSlots::render_default().encode())],
                    ),
                ],
            ),
            list(*b"PTRE", vec![raw(*b"ftwd", [0; 56])]),
        ],
        // No specimen XMP/GUID, timestamps, or workspace binary blobs.
        xmp: Vec::new(),
    };
    super::effects::register_definitions(&mut project.chunks)?;
    Ok(project)
}

/// Renderer selection has two fixed-width ASCII names and a revision flag.
/// It is not a captured plugin blob: Classic 3D has no custom options here.
fn classic_renderer() -> Chunk {
    let mut renderer = Vec::new();
    renderer.extend_from_slice(&0_u32.to_be_bytes());
    for name in ["ADBE Escher", "Classic 3D"] {
        let mut field = [0; 48];
        field[..name.len()].copy_from_slice(name.as_bytes());
        renderer.extend_from_slice(&field);
    }
    renderer.extend_from_slice(&1_u32.to_be_bytes());
    let mut options = Vec::new();
    for value in [1_u32, 0, 0] {
        options.extend_from_slice(&value.to_be_bytes());
    }
    list(
        *b"PRin",
        vec![raw(*b"prin", renderer), raw(*b"prda", options)],
    )
}

#[cfg(test)]
mod tests {
    use super::{build_project, composition_item};
    use crate::{
        aep::Project,
        reader::read_empty_composition,
        rifx::Chunk,
        schema::{CompositionRecord, HeadRecord, ItemRecord},
    };

    fn record(chunks: &[Chunk], kind: [u8; 4]) -> &[u8] {
        chunks
            .iter()
            .find(|chunk| chunk.id() == kind)
            .and_then(Chunk::data_payload)
            .unwrap()
    }

    fn children(chunks: &[Chunk], kind: [u8; 4]) -> &[Chunk] {
        chunks
            .iter()
            .find(|chunk| chunk.list_kind() == Some(kind))
            .and_then(Chunk::children)
            .unwrap()
    }

    #[test]
    fn new_root_encodes_independent_names_dimensions_and_durations() {
        for (name, width, height, ticks) in
            [("画面-Ω", 640, 360, 73_728), ("tall", 720, 1280, 49_152)]
        {
            // The view constructor is independently responsible for IDs 2..12;
            // these tests deliberately isolate the root/item codec.
            let project = build_project(
                name,
                width,
                height,
                crate::timing::Duration24::from_frames(ticks / 1024).unwrap(),
                vec![],
            )
            .unwrap();
            let bytes = project.encode().unwrap();
            let decoded = Project::parse(&bytes).unwrap();
            assert_eq!(decoded, project);
            assert!(decoded.xmp.is_empty());
            let head = HeadRecord::decode(record(&decoded.chunks, *b"head")).unwrap();
            assert_eq!((head.format_version(), head.next_item_id()), (97, 13));
            let folder = children(&decoded.chunks, *b"Fold");
            let fee = children(folder, *b"FEE ");
            assert_eq!(record(fee, *b"ppSn"), 0.0_f64.to_be_bytes());
            let item = children(folder, *b"Item");
            let idta = ItemRecord::decode(record(item, *b"idta")).unwrap();
            assert_eq!((idta.item_type(), idta.id()), (4, 1));
            assert_eq!(record(item, *b"Utf8"), name.as_bytes());
            let cdta = CompositionRecord::decode(record(item, *b"cdta")).unwrap();
            assert_eq!(cdta.dimensions(), (width, height));
            assert_eq!(cdta.duration_fraction().unwrap(), (ticks, 24_576));
            assert_eq!(cdta.frame_rate(), 24.0);
            let interpreted = read_empty_composition(&bytes).unwrap();
            assert_eq!(interpreted.name, name);
            assert_eq!((interpreted.width, interpreted.height), (width, height));
        }
    }

    #[test]
    fn nested_composition_item_uses_its_allocated_identity() {
        let duration = crate::timing::Duration24::from_frames(24).unwrap();
        let composition = CompositionRecord::empty_ae26(320, 180, duration).unwrap();
        let item = composition_item(27, "Nested", composition, Vec::new(), Vec::new()).unwrap();
        let children = item.children().unwrap();
        let idta = ItemRecord::decode(record(children, *b"idta")).unwrap();
        assert_eq!((idta.item_type(), idta.id()), (4, 27));
        assert_eq!(record(children, *b"iide"), 27_u32.to_le_bytes());
    }

    #[test]
    fn rejects_invalid_composition_input() {
        assert!(
            build_project(
                "",
                640,
                360,
                crate::timing::Duration24::from_frames(1).unwrap(),
                vec![]
            )
            .is_err()
        );
        assert!(
            build_project(
                "nul\0name",
                640,
                360,
                crate::timing::Duration24::from_frames(1).unwrap(),
                vec![]
            )
            .is_err()
        );
        assert!(
            build_project(
                "ok",
                0,
                360,
                crate::timing::Duration24::from_frames(1).unwrap(),
                vec![]
            )
            .is_err()
        );
    }
}
