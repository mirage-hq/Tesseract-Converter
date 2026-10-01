//! Exact adjacent Collect Files paths; never a filename search.

use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    ffi::OsStr,
    path::{Path, PathBuf},
};

use crate::{
    media::MediaKind,
    structure::{ItemKind, ProjectItem, StructuralProject},
    structure_document::{MediaAssetKind, MediaAssetRequest},
};

struct CollectedSource {
    authored: PathBuf,
    relative: PathBuf,
    sequence_folder: bool,
}

#[derive(Default)]
pub(super) struct CollectedPaths {
    sources: HashMap<u32, Result<CollectedSource, &'static str>>,
}

impl CollectedPaths {
    pub(super) fn new(project: &StructuralProject) -> Self {
        let folders: HashMap<_, _> = project
            .items
            .iter()
            .filter(|item| matches!(item.kind, ItemKind::Folder))
            .map(|item| (item.id, item))
            .collect();
        let mut sources = HashMap::new();
        let mut owners = HashMap::new();
        let mut conflicts = HashSet::new();
        for item in &project.items {
            let Some(Ok(descriptor)) = &item.media else {
                continue;
            };
            let source = collected_source(item, &folders);
            if let Ok(source) = &source {
                let identity = (&descriptor.authored_path, source.sequence_folder);
                match owners.entry(source.relative.clone()) {
                    Entry::Vacant(entry) => {
                        entry.insert(identity);
                    }
                    Entry::Occupied(entry) if *entry.get() != identity => {
                        conflicts.insert(source.relative.clone());
                    }
                    Entry::Occupied(_) => {}
                }
            }
            sources.insert(item.id, source);
        }
        // Mark every owner, including later duplicates of an already conflicting source.
        for source in sources.values_mut() {
            if source
                .as_ref()
                .is_ok_and(|source| conflicts.contains(&source.relative))
            {
                *source = Err("conflicting collected media identities");
            }
        }
        Self { sources }
    }

    pub(super) fn candidate(
        &self,
        request: &MediaAssetRequest,
    ) -> Option<Result<PathBuf, &'static str>> {
        let source = match self.sources.get(&request.source_item_id)? {
            Ok(source) => source,
            Err(reason) => return Some(Err(reason)),
        };
        let authored = Path::new(&request.authored_path);
        if source.sequence_folder {
            if request.kind != MediaAssetKind::SequenceImage
                || authored.parent() != Some(&source.authored)
            {
                return Some(Err(
                    "collected sequence request does not match its native source folder",
                ));
            }
            let Some(filename) = authored.file_name().filter(|name| safe_component(name)) else {
                return Some(Err("unsafe collected sequence filename"));
            };
            Some(Ok(source.relative.join(filename)))
        } else if authored == source.authored {
            Some(Ok(source.relative.clone()))
        } else {
            Some(Err(
                "collected request does not match its native source path",
            ))
        }
    }
}

fn collected_source(
    item: &ProjectItem,
    folders: &HashMap<u32, &ProjectItem>,
) -> Result<CollectedSource, &'static str> {
    let descriptor = item
        .media
        .as_ref()
        .and_then(|media| media.as_ref().ok())
        .ok_or("collected source descriptor unavailable")?;
    let sequence_folder =
        descriptor.kind == MediaKind::ImageSequence && descriptor.target_is_folder;
    if descriptor.kind == MediaKind::ImageSequence && !sequence_folder {
        return Err("collected image sequences require a native folder alias");
    }
    let authored = Path::new(&descriptor.authored_path);
    let basename = authored
        .file_name()
        .filter(|name| safe_component(name))
        .ok_or("unsafe collected filename")?;
    let mut parts = Vec::new();
    let mut current = item.parent_folder;
    let mut seen = HashSet::new();
    while let Some(id) = current {
        if !seen.insert(id) {
            return Err("cyclic native folder ancestry");
        }
        let folder = folders.get(&id).ok_or("unknown native folder ancestry")?;
        if !safe_component(OsStr::new(&folder.name)) {
            return Err("unsafe native folder component");
        }
        parts.push(folder.name.as_str());
        current = folder.parent_folder;
    }
    let mut relative = PathBuf::from("(Footage)");
    for part in parts.into_iter().rev() {
        relative.push(part);
    }
    relative.push(basename);
    Ok(CollectedSource {
        authored: authored.to_owned(),
        relative,
        sequence_folder,
    })
}

fn safe_component(value: &OsStr) -> bool {
    let text = value.to_string_lossy();
    !text.is_empty() && text != "." && text != ".." && !text.contains(['/', '\\', ':', '\0'])
}
