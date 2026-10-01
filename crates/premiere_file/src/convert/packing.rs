//! Source-boundary picture packing and typed foreign-picture replacement.
//!
//! The lowering traversal records only numeric ownership tokens and packing
//! actions. Native picture objects remain owned by `PrProjectFile`; replacement
//! drains them through the recorded per-track inventories and replays the same
//! overlap packing without re-running source lowering.

use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use fx_schema::LayerId;

use crate::{
    error::{unsupported, BuildError, Result},
    format::{
        FrameRate, MediaId, PrMedia, PrProjectFile, PrSequence, PrVideoItem, PrVideoOccurrence,
    },
    schema::{records::MediaPathField, PrMediaKind, PrNestOccurrence, PrVideoStream, PrVideoTrack},
    PrAfterEffectsComposition,
};

// u32::MAX is the disabled-capture sentinel, not a usable source token.
const BOUNDARY_LIMIT: usize = u32::MAX as usize;

static NEXT_RECIPE_ID: AtomicU64 = AtomicU64::new(1);

/// Identity of one retained preparation's packing recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PicturePackingId(u64);

impl PicturePackingId {
    /// Process-local numeric value. It is not a native Adobe identifier.
    pub fn value(self) -> u64 {
        self.0
    }
}

/// Numeric identity of one root or nested sequence container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PictureContainerToken(u32);

impl PictureContainerToken {
    /// Preparation-local numeric value.
    pub fn value(self) -> u32 {
        self.0
    }
}

/// Numeric identity of one visited source-layer slot, including empty slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceBoundaryToken(u32);

impl SourceBoundaryToken {
    /// Preparation-local numeric value.
    pub fn value(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PlacementToken(u32);

/// One source slot in its original bottom-to-top packing order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PictureSourceBoundary {
    pub token: SourceBoundaryToken,
    pub container: PictureContainerToken,
    pub ordinal: usize,
    pub layer: LayerId,
}

/// Read-only clock/canvas inventory for one packing container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictureContainer {
    pub token: PictureContainerToken,
    pub parent_boundary: Option<SourceBoundaryToken>,
    pub dimensions: [u32; 2],
    pub frame_rate: FrameRate,
    pub timeline_end_ticks: i64,
    pub boundaries: Vec<SourceBoundaryToken>,
}

/// Bounded source-slot recipe produced by the actual native lowerer.
#[derive(Debug)]
pub struct PicturePackingRecipe {
    id: PicturePackingId,
    complete: bool,
    root: PictureContainerToken,
    containers: BTreeMap<PictureContainerToken, ContainerRecipe>,
}

impl PicturePackingRecipe {
    /// Whether all native placements were captured with valid ownership tokens.
    /// Incomplete capture does not prevent ordinary native export.
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// Preparation identity required by every replacement request.
    pub fn id(&self) -> PicturePackingId {
        self.id
    }

    /// Root sequence container.
    pub fn root(&self) -> PictureContainerToken {
        self.root
    }

    /// Bounded container summaries in numeric-token order.
    pub fn containers(&self) -> Vec<PictureContainer> {
        self.containers
            .values()
            .map(|container| PictureContainer {
                token: container.token,
                parent_boundary: container.parent_boundary,
                dimensions: container.header.dimensions,
                frame_rate: container.header.frame_rate,
                timeline_end_ticks: container.header.timeline_end_ticks,
                boundaries: container.boundaries.iter().map(|item| item.token).collect(),
            })
            .collect()
    }

    /// Source slots with actual native picture placements in this container.
    /// Pending empty nests do not count; callers still validate container reachability.
    pub fn retained_picture_boundaries(
        &self,
        container: PictureContainerToken,
    ) -> impl Iterator<Item = SourceBoundaryToken> + '_ {
        self.containers
            .get(&container)
            .into_iter()
            .flat_map(|container| &container.actions)
            .filter(|action| action.kind != PlacementKind::PendingNest)
            .map(|action| action.boundary)
    }

    /// Every visited source slot, including slots that emitted no native picture.
    pub fn boundaries(&self) -> Vec<PictureSourceBoundary> {
        self.containers
            .values()
            .flat_map(|container| container.boundaries.iter().copied())
            .collect()
    }
}

/// One actual Dynamic Link picture supplied by the coordinator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AfterEffectsPicture {
    /// Existing native Dynamic Link composition GUID.
    pub composition_guid: String,
    /// Canonical package-relative AEP path, for example
    /// `media/ae-0001/compositions.aep`.
    pub relative_path: PathBuf,
    pub dimensions: [u32; 2],
    pub frame_rate: FrameRate,
    pub intrinsic_duration_ticks: i64,
    pub timeline_ticks: Range<i64>,
    pub source_ticks: Range<i64>,
    pub enabled: bool,
}

/// Replacement of one nonempty, contiguous source-boundary interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictureReplacement {
    pub packing_id: PicturePackingId,
    pub container: PictureContainerToken,
    /// Exact source slots in original order. A range is not inferred from the
    /// first and last token, so skipped/interleaved slots fail closed.
    pub boundaries: Vec<SourceBoundaryToken>,
    pub picture: AfterEffectsPicture,
}

#[derive(Debug)]
pub(crate) struct PackingOutcome {
    pub project: PrProjectFile,
    pub foreign_paths: Vec<PathBuf>,
    pub foreign_media_ids: BTreeSet<MediaId>,
}

#[derive(Debug)]
pub(crate) struct PicturePacker {
    recipe: PicturePackingRecipe,
    next_boundary: u32,
    next_placement: u32,
}

#[derive(Debug, Clone)]
struct ContainerHeader {
    name: String,
    top_level: bool,
    dimensions: [u32; 2],
    frame_rate: FrameRate,
    timeline_end_ticks: i64,
}

#[derive(Debug)]
struct ContainerRecipe {
    token: PictureContainerToken,
    parent_boundary: Option<SourceBoundaryToken>,
    header: ContainerHeader,
    boundaries: Vec<PictureSourceBoundary>,
    actions: Vec<PlacementAction>,
    tracks: Vec<TrackInventory>,
    mattes: Vec<MatteRelation>,
    pending_nests: BTreeMap<PlacementToken, PrNestOccurrence>,
}

#[derive(Debug, Clone, Copy)]
struct PlacementAction {
    boundary: SourceBoundaryToken,
    placement: PlacementToken,
    kind: PlacementKind,
    child: Option<PictureContainerToken>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlacementKind {
    Item,
    Nest,
    PendingNest,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PlacementLocation {
    Item { track: usize, position: usize },
    Nest { track: usize, position: usize },
}

#[derive(Debug, Default)]
struct TrackInventory {
    items: Vec<PlacementToken>,
    nests: Vec<PlacementToken>,
}

#[derive(Debug, Clone, Copy)]
struct MatteRelation {
    consumer: PlacementToken,
    consumer_boundary: SourceBoundaryToken,
    source: PlacementToken,
    source_boundary: SourceBoundaryToken,
}

impl PicturePacker {
    pub(crate) fn new(
        name: &str,
        dimensions: [u32; 2],
        frame_rate: FrameRate,
        timeline_end_ticks: i64,
    ) -> Self {
        let id = NEXT_RECIPE_ID.fetch_add(1, Ordering::Relaxed);
        let id = if id == 0 {
            NEXT_RECIPE_ID.fetch_add(1, Ordering::Relaxed)
        } else {
            id
        };
        let root = PictureContainerToken(0);
        let mut containers = BTreeMap::new();
        containers.insert(
            root,
            ContainerRecipe::new(
                root,
                None,
                ContainerHeader {
                    name: name.to_owned(),
                    top_level: true,
                    dimensions,
                    frame_rate,
                    timeline_end_ticks,
                },
            ),
        );
        Self {
            recipe: PicturePackingRecipe {
                id: PicturePackingId(id),
                complete: true,
                root,
                containers,
            },
            next_boundary: 0,
            next_placement: 0,
        }
    }

    pub(crate) fn root(&self) -> PictureContainerToken {
        self.recipe.root
    }

    // Capture is advisory for native export, but mandatory for replacements.
    // Once disabled, do not allocate or accept further ownership evidence.
    fn capture<T>(
        &mut self,
        fallback: T,
        action: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        if !self.recipe.complete {
            return Ok(fallback);
        }
        match action(self) {
            Ok(value) => Ok(value),
            Err(_) => {
                self.recipe.complete = false;
                Ok(fallback)
            }
        }
    }

    pub(crate) fn begin_container(
        &mut self,
        parent_boundary: SourceBoundaryToken,
        name: &str,
        dimensions: [u32; 2],
        frame_rate: FrameRate,
        timeline_end_ticks: i64,
    ) -> Result<PictureContainerToken> {
        self.capture(PictureContainerToken(u32::MAX), |this| {
            this.try_begin_container(
                parent_boundary,
                name,
                dimensions,
                frame_rate,
                timeline_end_ticks,
            )
        })
    }

    pub(crate) fn begin_boundary(
        &mut self,
        container: PictureContainerToken,
        layer: LayerId,
    ) -> Result<SourceBoundaryToken> {
        self.capture(SourceBoundaryToken(u32::MAX), |this| {
            this.try_begin_boundary(container, layer)
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &mut self,
        container: PictureContainerToken,
        boundary: SourceBoundaryToken,
        track: usize,
        kind: PlacementKind,
        child: Option<PictureContainerToken>,
        pending: Option<PrNestOccurrence>,
    ) -> Result<PlacementToken> {
        self.capture(PlacementToken(u32::MAX), |this| {
            this.try_record(container, boundary, track, kind, child, pending)
        })
    }

    pub(crate) fn record_matte(
        &mut self,
        container: PictureContainerToken,
        consumer: PlacementLocation,
        source: PlacementLocation,
    ) -> Result<()> {
        self.capture((), |this| {
            this.try_record_matte(container, consumer, source)
        })
    }

    pub(crate) fn finish_container(
        &mut self,
        container: PictureContainerToken,
        tracks: &mut [PrVideoTrack],
        timeline_end_ticks: i64,
    ) -> Result<()> {
        self.capture((), |this| {
            this.try_finish_container(container, tracks, timeline_end_ticks)
        })?;
        // Native ordering must remain unchanged even after capture exhaustion.
        for track in tracks {
            track.items.sort_by_key(|item| item.timeline_ticks().start);
            track.nests.sort_by_key(|nest| nest.start_ticks);
        }
        Ok(())
    }

    fn try_begin_container(
        &mut self,
        parent_boundary: SourceBoundaryToken,
        name: &str,
        dimensions: [u32; 2],
        frame_rate: FrameRate,
        timeline_end_ticks: i64,
    ) -> Result<PictureContainerToken> {
        // Reserve u32::MAX for the disabled-capture sentinel.
        if self.recipe.containers.len() >= u32::MAX as usize {
            return Err(unsupported("picture container token overflow"));
        }
        let token = PictureContainerToken(
            u32::try_from(self.recipe.containers.len())
                .map_err(|_| unsupported("picture container token overflow"))?,
        );
        self.recipe.containers.insert(
            token,
            ContainerRecipe::new(
                token,
                Some(parent_boundary),
                ContainerHeader {
                    name: name.to_owned(),
                    top_level: false,
                    dimensions,
                    frame_rate,
                    timeline_end_ticks,
                },
            ),
        );
        Ok(token)
    }

    fn try_begin_boundary(
        &mut self,
        container: PictureContainerToken,
        layer: LayerId,
    ) -> Result<SourceBoundaryToken> {
        if self.next_boundary as usize == BOUNDARY_LIMIT {
            return Err(unsupported(format!(
                "picture packing exceeds {BOUNDARY_LIMIT} source boundaries"
            )));
        }
        let token = SourceBoundaryToken(self.next_boundary);
        self.next_boundary += 1;
        let recipe = self.container_mut(container)?;
        let ordinal = recipe.boundaries.len();
        recipe.boundaries.push(PictureSourceBoundary {
            token,
            container,
            ordinal,
            layer,
        });
        Ok(token)
    }

    pub(crate) fn record_item(
        &mut self,
        container: PictureContainerToken,
        boundary: SourceBoundaryToken,
        track: usize,
    ) -> Result<PlacementToken> {
        self.record(container, boundary, track, PlacementKind::Item, None, None)
    }

    pub(crate) fn record_nest(
        &mut self,
        container: PictureContainerToken,
        boundary: SourceBoundaryToken,
        track: usize,
        child: PictureContainerToken,
    ) -> Result<PlacementToken> {
        self.record(
            container,
            boundary,
            track,
            PlacementKind::Nest,
            Some(child),
            None,
        )
    }

    pub(crate) fn record_pending_nest(
        &mut self,
        container: PictureContainerToken,
        boundary: SourceBoundaryToken,
        child: PictureContainerToken,
        nest: PrNestOccurrence,
    ) -> Result<PlacementToken> {
        self.record(
            container,
            boundary,
            0,
            PlacementKind::PendingNest,
            Some(child),
            Some(nest),
        )
    }

    fn try_record(
        &mut self,
        container: PictureContainerToken,
        boundary: SourceBoundaryToken,
        track: usize,
        kind: PlacementKind,
        child: Option<PictureContainerToken>,
        pending: Option<PrNestOccurrence>,
    ) -> Result<PlacementToken> {
        let placement = PlacementToken(self.next_placement);
        self.next_placement = self
            .next_placement
            .checked_add(1)
            .ok_or_else(|| unsupported("picture placement token overflow"))?;
        let recipe = self.container_mut(container)?;
        if !recipe.boundaries.iter().any(|item| item.token == boundary) {
            return Err(unsupported(
                "picture placement uses a foreign source boundary",
            ));
        }
        recipe.actions.push(PlacementAction {
            boundary,
            placement,
            kind,
            child,
        });
        if kind == PlacementKind::PendingNest {
            let nest = pending.ok_or_else(|| unsupported("pending nest header is missing"))?;
            recipe.pending_nests.insert(placement, nest);
        } else {
            while recipe.tracks.len() <= track {
                recipe.tracks.push(TrackInventory::default());
            }
            match kind {
                PlacementKind::Item => recipe.tracks[track].items.push(placement),
                PlacementKind::Nest => recipe.tracks[track].nests.push(placement),
                PlacementKind::PendingNest => unreachable!("handled above"),
            }
        }
        Ok(placement)
    }

    fn try_finish_container(
        &mut self,
        container: PictureContainerToken,
        tracks: &mut [PrVideoTrack],
        timeline_end_ticks: i64,
    ) -> Result<()> {
        let recipe = self.container_mut(container)?;
        recipe.header.timeline_end_ticks = timeline_end_ticks;
        if tracks.len() != recipe.tracks.len() {
            return Err(unsupported(
                "picture track inventory does not match native tracks",
            ));
        }
        for (track, inventory) in tracks.iter_mut().zip(&mut recipe.tracks) {
            sort_together(&mut track.items, &mut inventory.items, |item| {
                item.timeline_ticks().start
            })?;
            sort_together(&mut track.nests, &mut inventory.nests, |nest| {
                nest.start_ticks
            })?;
        }
        Ok(())
    }

    fn try_record_matte(
        &mut self,
        container: PictureContainerToken,
        consumer: PlacementLocation,
        source: PlacementLocation,
    ) -> Result<()> {
        let recipe = self.container_mut(container)?;
        let resolve = |location| {
            let token = match location {
                PlacementLocation::Item { track, position } => recipe
                    .tracks
                    .get(track)
                    .and_then(|lane| lane.items.get(position)),
                PlacementLocation::Nest { track, position } => recipe
                    .tracks
                    .get(track)
                    .and_then(|lane| lane.nests.get(position)),
            }
            .ok_or_else(|| unsupported("track matte placement address is missing"))?;
            let action = recipe
                .actions
                .iter()
                .find(|action| action.placement == *token)
                .ok_or_else(|| unsupported("track matte placement action is missing"))?;
            Ok::<_, BuildError>((*token, action.boundary))
        };
        let (consumer, consumer_boundary) = resolve(consumer)?;
        let (source, source_boundary) = resolve(source)?;
        recipe.mattes.push(MatteRelation {
            consumer,
            consumer_boundary,
            source,
            source_boundary,
        });
        Ok(())
    }

    pub(crate) fn finish(self) -> PicturePackingRecipe {
        self.recipe
    }

    fn container_mut(&mut self, token: PictureContainerToken) -> Result<&mut ContainerRecipe> {
        self.recipe
            .containers
            .get_mut(&token)
            .ok_or_else(|| unsupported("unknown picture packing container"))
    }
}

impl ContainerRecipe {
    fn new(
        token: PictureContainerToken,
        parent_boundary: Option<SourceBoundaryToken>,
        header: ContainerHeader,
    ) -> Self {
        Self {
            token,
            parent_boundary,
            header,
            boundaries: Vec::new(),
            actions: Vec::new(),
            tracks: Vec::new(),
            mattes: Vec::new(),
            pending_nests: BTreeMap::new(),
        }
    }

    fn empty_sequence(&self) -> PrSequence {
        PrSequence {
            id: None,
            name: self.header.name.clone(),
            top_level: Some(self.header.top_level),
            video_tracks: Vec::new(),
            audio: Vec::new(),
            frame_rate: self.header.frame_rate,
            width: self.header.dimensions[0],
            height: self.header.dimensions[1],
            timeline_end_ticks: self.header.timeline_end_ticks,
        }
    }
}

fn sort_together<T>(
    objects: &mut Vec<T>,
    tokens: &mut Vec<PlacementToken>,
    key: impl Fn(&T) -> i64,
) -> Result<()> {
    if objects.len() != tokens.len() {
        return Err(unsupported(
            "picture object/token inventory length mismatch",
        ));
    }
    let mut pairs: Vec<_> = objects.drain(..).zip(tokens.drain(..)).collect();
    pairs.sort_by_key(|(object, _)| key(object));
    for (object, token) in pairs {
        objects.push(object);
        tokens.push(token);
    }
    Ok(())
}

pub(crate) fn apply_replacements(
    project: Option<PrProjectFile>,
    mut recipe: PicturePackingRecipe,
    replacements: &[PictureReplacement],
    final_output: &Path,
) -> Result<PackingOutcome> {
    let plans = validate_replacements(&recipe, replacements)?;
    let root = recipe.root;
    let mut media = project
        .as_ref()
        .map(|project| project.media.keys().cloned().collect::<BTreeSet<_>>())
        .unwrap_or_default();
    let mut media_records = BTreeMap::new();
    let mut sequence = match project {
        Some(project) => {
            let (mut sequences, actual_media) = project.into_parts();
            media_records = actual_media;
            if sequences.len() != 1 {
                return Err(unsupported("picture packing requires one root sequence"));
            }
            sequences.remove(0)
        }
        None => recipe
            .containers
            .get(&root)
            .ok_or_else(|| unsupported("root picture packing container is missing"))?
            .empty_sequence(),
    };
    let mut foreign_paths = Vec::with_capacity(replacements.len());
    let mut foreign_media = BTreeMap::new();
    replay_container(
        &mut recipe,
        root,
        &mut sequence,
        &plans,
        replacements,
        final_output,
        &mut media,
        &mut foreign_media,
        &mut foreign_paths,
    )?;
    if foreign_paths.len() != replacements.len() {
        return Err(unsupported("a picture replacement was not replayed"));
    }
    let foreign_media_ids = foreign_media.keys().cloned().collect();
    media_records.extend(foreign_media);
    let referenced: BTreeSet<_> = sequence.media_in_order().into_iter().cloned().collect();
    media_records.retain(|id, _| referenced.contains(id));
    let project = PrProjectFile::from_sequences(vec![sequence], media_records);
    project.validate()?;
    Ok(PackingOutcome {
        project,
        foreign_paths,
        foreign_media_ids,
    })
}

#[derive(Debug)]
struct ReplacementPlan {
    replacement: usize,
    boundaries: BTreeSet<SourceBoundaryToken>,
    first: SourceBoundaryToken,
}

type Plans = BTreeMap<PictureContainerToken, Vec<ReplacementPlan>>;

fn validate_replacements(
    recipe: &PicturePackingRecipe,
    replacements: &[PictureReplacement],
) -> Result<Plans> {
    if !recipe.complete {
        return Err(unsupported("picture packing capture is incomplete"));
    }
    if replacements.is_empty() {
        return Err(unsupported("picture replacement list must not be empty"));
    }
    let mut reachable = BTreeSet::new();
    let mut pending = vec![recipe.root];
    while let Some(token) = pending.pop() {
        if !reachable.insert(token) {
            return Err(unsupported(
                "picture container ownership is cyclic or shared",
            ));
        }
        let container = recipe
            .containers
            .get(&token)
            .ok_or_else(|| unsupported("picture container ownership is missing"))?;
        for action in &container.actions {
            if let Some(child) = action.child {
                let child_container = recipe
                    .containers
                    .get(&child)
                    .ok_or_else(|| unsupported("nested picture container is missing"))?;
                if child_container.parent_boundary != Some(action.boundary) {
                    return Err(unsupported(
                        "nested picture container has a different owner",
                    ));
                }
                pending.push(child);
            }
        }
    }
    let boundary_locations: BTreeMap<_, _> = recipe
        .containers
        .values()
        .flat_map(|container| {
            container
                .boundaries
                .iter()
                .enumerate()
                .map(|(position, boundary)| (boundary.token, (container.token, position)))
        })
        .collect();
    let selected: BTreeSet<_> = replacements
        .iter()
        .flat_map(|replacement| replacement.boundaries.iter().copied())
        .collect();
    let mut plans: Plans = BTreeMap::new();
    let mut paths = BTreeSet::new();
    for (index, replacement) in replacements.iter().enumerate() {
        if replacement.packing_id != recipe.id {
            return Err(unsupported("stale picture packing recipe identity"));
        }
        if !reachable.contains(&replacement.container) {
            return Err(unsupported("picture replacement container is not retained"));
        }
        let mut ancestor = replacement.container;
        for _ in 0..=recipe.containers.len() {
            let parent = recipe
                .containers
                .get(&ancestor)
                .ok_or_else(|| unsupported("picture ancestor container is missing"))?
                .parent_boundary;
            let Some(parent) = parent else {
                break;
            };
            if selected.contains(&parent) {
                return Err(unsupported(
                    "picture replacements overlap through an ancestor",
                ));
            }
            ancestor = boundary_locations
                .get(&parent)
                .ok_or_else(|| unsupported("picture ancestor boundary is missing"))?
                .0;
        }
        let container = recipe
            .containers
            .get(&replacement.container)
            .ok_or_else(|| unsupported("picture replacement names the wrong container"))?;
        if replacement.boundaries.is_empty()
            || replacement.boundaries.len() > container.boundaries.len()
        {
            return Err(unsupported(
                "picture replacement requires a bounded nonempty boundary interval",
            ));
        }
        let mut previous = None;
        let mut selected = BTreeSet::new();
        for token in &replacement.boundaries {
            let (_, position) = *boundary_locations
                .get(token)
                .filter(|(owner, _)| *owner == replacement.container)
                .ok_or_else(|| unsupported("picture boundary belongs to the wrong container"))?;
            if previous.is_some_and(|previous| position != previous + 1) || !selected.insert(*token)
            {
                return Err(unsupported(
                    "picture replacement boundaries must be unique and contiguous in source order",
                ));
            }
            previous = Some(position);
        }
        validate_picture(&replacement.picture, container)?;
        let path = normalized_foreign_path(&replacement.picture.relative_path)?;
        let folded = path.to_string_lossy().to_ascii_lowercase();
        if !paths.insert(folded) {
            return Err(unsupported("foreign AEP package paths collide"));
        }
        plans
            .entry(replacement.container)
            .or_default()
            .push(ReplacementPlan {
                replacement: index,
                first: replacement.boundaries[0],
                boundaries: selected,
            });
    }
    for (container_token, container_plans) in &plans {
        let container = &recipe.containers[container_token];
        let mut owners = BTreeMap::new();
        for plan in container_plans {
            for boundary in &plan.boundaries {
                if owners.insert(*boundary, plan.replacement).is_some() {
                    return Err(unsupported(
                        "picture replacement boundary intervals overlap",
                    ));
                }
            }
        }
        for relation in &container.mattes {
            let consumer = owners.get(&relation.consumer_boundary);
            let source = owners.get(&relation.source_boundary);
            if consumer != source {
                return Err(unsupported(
                    "picture replacement crosses a track-matte ownership boundary",
                ));
            }
        }
    }
    Ok(plans)
}

fn validate_picture(picture: &AfterEffectsPicture, container: &ContainerRecipe) -> Result<()> {
    if picture.composition_guid.len() != 36 {
        return Err(unsupported(
            "replacement requires a canonical composition GUID",
        ));
    }
    let identity = PrAfterEffectsComposition::parse(&picture.composition_guid)
        .ok_or_else(|| unsupported("replacement requires a canonical non-nil composition GUID"))?;
    let _ = identity;
    if picture.dimensions != container.header.dimensions
        || picture.frame_rate != container.header.frame_rate
    {
        return Err(unsupported(
            "replacement canvas and frame rate must match its packing container",
        ));
    }
    let timeline = &picture.timeline_ticks;
    let source = &picture.source_ticks;
    let frame_ticks = picture.frame_rate.ticks_per_frame();
    if picture.intrinsic_duration_ticks <= 0
        || timeline.start < 0
        || timeline.end <= timeline.start
        || timeline.end > container.header.timeline_end_ticks
        || source.start < 0
        || source.end <= source.start
        || source.end > picture.intrinsic_duration_ticks
        || timeline.end - timeline.start != source.end - source.start
        || [timeline.start, timeline.end, source.start, source.end]
            .iter()
            .any(|ticks| ticks % frame_ticks != 0)
    {
        return Err(unsupported(
            "replacement requires positive, frame-aligned, unretimed timeline/source ranges within its container and intrinsic clocks",
        ));
    }
    Ok(())
}

fn normalized_foreign_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() || path.as_os_str().is_empty() {
        return Err(unsupported("foreign AEP path must be package-relative"));
    }
    let scope = path
        .to_str()
        .and_then(|text| text.strip_prefix("media/ae-"))
        .and_then(|text| text.strip_suffix("/compositions.aep"));
    if !scope.is_some_and(|scope| {
        scope.len() == 4 && scope.bytes().all(|byte| byte.is_ascii_digit()) && scope != "0000"
    }) {
        return Err(unsupported(
            "foreign AEP path must use media/ae-NNNN/compositions.aep with scope 0001..9999",
        ));
    }
    Ok(path.to_owned())
}

#[allow(clippy::too_many_arguments)]
fn replay_container(
    recipe: &mut PicturePackingRecipe,
    token: PictureContainerToken,
    sequence: &mut PrSequence,
    plans: &Plans,
    replacements: &[PictureReplacement],
    final_output: &Path,
    used_media_ids: &mut BTreeSet<MediaId>,
    media: &mut BTreeMap<MediaId, PrMedia>,
    foreign_paths: &mut Vec<PathBuf>,
) -> Result<()> {
    let mut container = recipe
        .containers
        .remove(&token)
        .ok_or_else(|| unsupported("stale picture container token"))?;
    if sequence.dimensions() != container.header.dimensions
        || sequence.frame_rate != container.header.frame_rate
        || sequence.timeline_end_ticks != container.header.timeline_end_ticks
    {
        return Err(unsupported("stale picture container clock or canvas"));
    }
    let mut objects = take_objects(sequence, &container.tracks)?;
    let container_plans = plans.get(&token).map(Vec::as_slice).unwrap_or_default();
    let selected_by_boundary: BTreeMap<_, _> = container_plans
        .iter()
        .flat_map(|plan| {
            plan.boundaries
                .iter()
                .map(move |boundary| (*boundary, plan))
        })
        .collect();
    let mut actions_by_boundary: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for action in &container.actions {
        actions_by_boundary
            .entry(action.boundary)
            .or_default()
            .push(action);
    }
    let mut placement_tracks = BTreeMap::new();
    let mut placement_locations = BTreeMap::new();
    let mut placed = BTreeSet::new();
    let mut output_tracks = Vec::new();
    for boundary in &container.boundaries {
        let actions = actions_by_boundary
            .get(&boundary.token)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if let Some(plan) = selected_by_boundary.get(&boundary.token) {
            for action in actions {
                let dropped = objects.remove(&action.placement);
                let pending = container.pending_nests.remove(&action.placement);
                if let Some(PackedObject::Nest(nest)) = &dropped {
                    reject_nested_audio(&nest.sequence)?;
                }
                if let Some(nest) = &pending {
                    reject_nested_audio(&nest.sequence)?;
                }
                if let Some(child) = action.child {
                    discard_container(recipe, child)?;
                }
            }
            if boundary.token == plan.first && placed.insert(plan.replacement) {
                let replacement = &replacements[plan.replacement];
                let item = foreign_item(
                    replacement,
                    final_output,
                    used_media_ids,
                    media,
                    foreign_paths,
                )?;
                place_replayed_item(&mut output_tracks, item, 0);
            }
            continue;
        }
        for action in actions {
            let object = match action.kind {
                PlacementKind::PendingNest => {
                    let mut nest = container
                        .pending_nests
                        .remove(&action.placement)
                        .ok_or_else(|| unsupported("pending nested header is missing"))?;
                    let child = action
                        .child
                        .ok_or_else(|| unsupported("pending nested container is missing"))?;
                    replay_container(
                        recipe,
                        child,
                        &mut nest.sequence,
                        plans,
                        replacements,
                        final_output,
                        used_media_ids,
                        media,
                        foreign_paths,
                    )?;
                    if nest.sequence.video_items().next().is_none()
                        && nest.sequence.nest_occurrences().next().is_none()
                    {
                        continue;
                    }
                    PackedObject::Nest(nest)
                }
                PlacementKind::Nest => {
                    let mut object = objects
                        .remove(&action.placement)
                        .ok_or_else(|| unsupported("stale nested placement inventory"))?;
                    let PackedObject::Nest(nest) = &mut object else {
                        return Err(unsupported("nested placement token names an item"));
                    };
                    if let Some(child) = action.child {
                        replay_container(
                            recipe,
                            child,
                            &mut nest.sequence,
                            plans,
                            replacements,
                            final_output,
                            used_media_ids,
                            media,
                            foreign_paths,
                        )?;
                    }
                    object
                }
                PlacementKind::Item => objects
                    .remove(&action.placement)
                    .ok_or_else(|| unsupported("stale item placement inventory"))?,
            };
            let min_track = container
                .mattes
                .iter()
                .filter(|relation| relation.source == action.placement)
                .filter_map(|relation| placement_tracks.get(&relation.consumer).copied())
                .max()
                .map_or(0, |track| track + 1);
            let track = match object {
                PackedObject::Item(item) => {
                    let track = place_replayed_item(&mut output_tracks, item, min_track);
                    placement_locations.insert(
                        action.placement,
                        PlacementLocation::Item {
                            track,
                            position: output_tracks[track].items.len() - 1,
                        },
                    );
                    track
                }
                PackedObject::Nest(nest) => {
                    let track = place_replayed_nest(&mut output_tracks, nest, min_track);
                    placement_locations.insert(
                        action.placement,
                        PlacementLocation::Nest {
                            track,
                            position: output_tracks[track].nests.len() - 1,
                        },
                    );
                    track
                }
            };
            placement_tracks.insert(action.placement, track);
        }
    }
    if !objects.is_empty() {
        return Err(unsupported(
            "stale picture placement inventory has unconsumed objects",
        ));
    }
    for relation in &container.mattes {
        if selected_by_boundary.contains_key(&relation.consumer_boundary) {
            continue;
        }
        let source_track = *placement_tracks
            .get(&relation.source)
            .ok_or_else(|| unsupported("retained track matte source was not replayed"))?;
        set_matte_track(
            &mut output_tracks,
            relation.consumer,
            source_track,
            &placement_locations,
        )?;
    }
    if placed.len() != container_plans.len() {
        return Err(unsupported("replacement interval has no packing action"));
    }
    for track in &mut output_tracks {
        track.items.sort_by_key(|item| item.timeline_ticks().start);
        track.nests.sort_by_key(|nest| nest.start_ticks);
    }
    sequence.video_tracks = output_tracks;
    Ok(())
}

fn reject_nested_audio(sequence: &PrSequence) -> Result<()> {
    let mut pending = vec![sequence];
    while let Some(sequence) = pending.pop() {
        if !sequence.audio.is_empty() {
            return Err(unsupported(
                "picture replacement cannot discard nested native audio",
            ));
        }
        pending.extend(sequence.nest_occurrences().map(|nest| &nest.sequence));
    }
    Ok(())
}

fn discard_container(
    recipe: &mut PicturePackingRecipe,
    token: PictureContainerToken,
) -> Result<()> {
    let mut pending = vec![token];
    while let Some(token) = pending.pop() {
        let container = recipe
            .containers
            .remove(&token)
            .ok_or_else(|| unsupported("discarded picture container is missing or shared"))?;
        for nest in container.pending_nests.values() {
            reject_nested_audio(&nest.sequence)?;
        }
        pending.extend(container.actions.iter().filter_map(|action| action.child));
    }
    Ok(())
}

#[derive(Debug)]
enum PackedObject {
    Item(PrVideoItem),
    Nest(PrNestOccurrence),
}

fn take_objects(
    sequence: &mut PrSequence,
    inventories: &[TrackInventory],
) -> Result<BTreeMap<PlacementToken, PackedObject>> {
    if sequence.video_tracks.len() != inventories.len() {
        return Err(unsupported("stale picture track inventory"));
    }
    let mut objects = BTreeMap::new();
    for (mut track, inventory) in sequence.video_tracks.drain(..).zip(inventories) {
        if !track.transitions.is_empty()
            || track.items.len() != inventory.items.len()
            || track.nests.len() != inventory.nests.len()
        {
            return Err(unsupported("stale picture item/nest inventory"));
        }
        for (item, token) in track.items.drain(..).zip(&inventory.items) {
            if objects.insert(*token, PackedObject::Item(item)).is_some() {
                return Err(unsupported("duplicate picture placement token"));
            }
        }
        for (nest, token) in track.nests.drain(..).zip(&inventory.nests) {
            if objects.insert(*token, PackedObject::Nest(nest)).is_some() {
                return Err(unsupported("duplicate picture placement token"));
            }
        }
    }
    Ok(objects)
}

fn place_replayed_item(
    tracks: &mut Vec<PrVideoTrack>,
    item: PrVideoItem,
    min_track: usize,
) -> usize {
    let index = tracks
        .iter()
        .rposition(|track| track.overlaps(&item.timeline_ticks()))
        .map_or(0, |index| index + 1)
        .max(min_track);
    ensure_track(tracks, index);
    tracks[index].items.push(item);
    index
}

fn place_replayed_nest(
    tracks: &mut Vec<PrVideoTrack>,
    nest: PrNestOccurrence,
    min_track: usize,
) -> usize {
    let range = nest.timeline_ticks();
    let index = tracks
        .iter()
        .rposition(|track| track.overlaps(&range))
        .map_or(0, |index| index + 1)
        .max(min_track);
    ensure_track(tracks, index);
    tracks[index].nests.push(nest);
    index
}

fn ensure_track(tracks: &mut Vec<PrVideoTrack>, index: usize) {
    while tracks.len() <= index {
        tracks.push(PrVideoTrack {
            items: Vec::new(),
            nests: Vec::new(),
            transitions: Vec::new(),
        });
    }
}

fn set_matte_track(
    tracks: &mut [PrVideoTrack],
    consumer: PlacementToken,
    source_track: usize,
    locations: &BTreeMap<PlacementToken, PlacementLocation>,
) -> Result<()> {
    let location = locations
        .get(&consumer)
        .ok_or_else(|| unsupported("retained track matte consumer was not replayed"))?;
    let matte = match *location {
        PlacementLocation::Item { track, position } => {
            match tracks
                .get_mut(track)
                .and_then(|lane| lane.items.get_mut(position))
            {
                Some(PrVideoItem::Media(clip)) => clip.track_matte.as_mut(),
                _ => None,
            }
        }
        PlacementLocation::Nest { track, position } => tracks
            .get_mut(track)
            .and_then(|lane| lane.nests.get_mut(position))
            .and_then(|nest| nest.track_matte.as_mut()),
    }
    .ok_or_else(|| unsupported("track matte consumer object is missing"))?;
    matte.track_index = source_track;
    Ok(())
}

fn foreign_item(
    replacement: &PictureReplacement,
    final_output: &Path,
    used_media_ids: &mut BTreeSet<MediaId>,
    media: &mut BTreeMap<MediaId, PrMedia>,
    foreign_paths: &mut Vec<PathBuf>,
) -> Result<PrVideoItem> {
    let picture = &replacement.picture;
    let identity = PrAfterEffectsComposition::parse(&picture.composition_guid)
        .ok_or_else(|| unsupported("replacement composition GUID became invalid"))?;
    let path = normalized_foreign_path(&picture.relative_path)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| unsupported("foreign AEP filename must be UTF-8"))?
        .to_owned();
    let mut suffix = 0_u64;
    let media_id = loop {
        let candidate = MediaId(format!("hybrid-after-effects-{suffix}"));
        if used_media_ids.insert(candidate.clone()) {
            break candidate;
        }
        suffix = suffix
            .checked_add(1)
            .ok_or_else(|| unsupported("foreign media identity overflow"))?;
    };
    let relative = format!("./{}", path.to_string_lossy());
    let absolute = final_output.join(&path);
    media.insert(
        media_id.clone(),
        PrMedia {
            name,
            relative_path: Some(relative.clone()),
            relative_paths: vec![relative],
            absolute_paths: vec![
                (MediaPathField::ActualMediaFilePath, absolute.clone()),
                (MediaPathField::FilePath, absolute),
            ],
            video: Some(PrVideoStream {
                orientation: crate::schema::VideoOrientation::Identity,
                intrinsic_ticks: picture.intrinsic_duration_ticks,
                frame_rate: (picture.frame_rate).into(),
                width: picture.dimensions[0],
                height: picture.dimensions[1],
                kind: PrMediaKind::AfterEffectsComposition(identity),
            }),
            audio: None,
        },
    );
    foreign_paths.push(path);
    Ok(PrVideoItem::Media(PrVideoOccurrence {
        enabled: picture.enabled,
        ..PrVideoOccurrence::unedited(
            media_id,
            picture.timeline_ticks.clone(),
            picture.source_ticks.clone(),
        )
    }))
}

#[cfg(test)]
#[path = "packing/tests.rs"]
mod tests;
