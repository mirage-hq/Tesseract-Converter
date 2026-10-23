//! Custom `.cube` files of Lumetri Input LUTs and Looks
//! ([`crate::schema::PrLumetriLut`]).
//!
//! Import locates each file and packages it as an FX asset (an `Other` asset
//! with a `.cube` path). Premiere saves the absolute path of a browsed LUT. A
//! project moved with its LUT keeps the file beside it, so a `.cube` of the
//! saved name next to the project is used first, then the saved absolute
//! path. When neither exists, the copy that Premiere embedded in the project
//! and references from the `Blob` ([`crate::schema::EmbeddedLut`]) is
//! written as a `.cube` of the saved name and packaged instead, as Premiere
//! renders the LUT from it. Every placement of one file shares one asset,
//! `premiere-lut-<n>` in first-appearance order.
//!
//! Export writes each `lookTransform` asset once into the package's `media`
//! folder beside the media ([`bind_luts`]) and saves its absolute path in the
//! Lumetri Look path slot, as Premiere saves a browsed file.
use crate::{
    error::{unsupported, Result},
    format::PrProjectFile,
    hash::hash,
    schema::{
        EmbeddedLut, LutFile, PrEffect, PrEffectParams, PrLumetriLut, PrSequence, PrVideoItem,
        ResolvedLut,
    },
};
use fx_schema::AssetId;
use std::{
    collections::BTreeSet,
    fs::File,
    path::{Path, PathBuf},
};
use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};

/// Call `visit` on every custom LUT of the effects that `sequence` writes or
/// reads: clip, clip source and nest effects, in nested sequences too.
fn for_each_lut(sequence: &mut PrSequence, visit: &mut impl FnMut(&mut PrLumetriLut)) {
    fn effects(effects: &mut [PrEffect], visit: &mut impl FnMut(&mut PrLumetriLut)) {
        for effect in effects {
            match &mut effect.params {
                PrEffectParams::LumetriInputLut(lut) => visit(lut),
                PrEffectParams::LumetriLook(look) => visit(&mut look.lut),
                _ => {}
            }
        }
    }
    for track in &mut sequence.video_tracks {
        for item in &mut track.items {
            if let PrVideoItem::Media(clip) = item {
                effects(&mut clip.effects, visit);
                if let Some(source) = &mut clip.source_effects {
                    effects(&mut source.effects, visit);
                }
            }
        }
        for nest in &mut track.nests {
            effects(&mut nest.effects, visit);
            for_each_lut(&mut nest.sequence, visit);
        }
    }
}

/// The located `.cube` files of one import, in asset order, and the
/// directory holding those written from embedded copies. Keep this value
/// until the archive that packages them is written.
#[derive(Debug, Default)]
pub(crate) struct LutFiles {
    files: Vec<ResolvedLut>,
    /// Saved path and written file of each embedded copy.
    written: Vec<(String, PathBuf)>,
    directory: Option<tempfile::TempDir>,
}

impl LutFiles {
    /// Locate every custom LUT of `sequence` and its nested sequences beside
    /// `root` (the project's folder), recording the result on each effect.
    pub(crate) fn resolve(root: &Path, sequence: &mut PrSequence) -> Self {
        let mut files = Self::default();
        for_each_lut(sequence, &mut |lut| {
            let located = locate(root, &lut.native_path).or_else(|reason| match &lut.embedded {
                Some(embedded) => files.write_embedded(&lut.native_path, embedded),
                None => Err(reason),
            });
            lut.file = match located {
                Ok(path) => LutFile::Found(files.asset(path)),
                Err(reason) => LutFile::Missing(reason),
            };
        });
        files
    }

    /// The embedded copy for `native_path` as `<n>/<saved name>` in this
    /// import's directory, written once per saved path.
    fn write_embedded(
        &mut self,
        native_path: &str,
        embedded: &EmbeddedLut,
    ) -> std::result::Result<PathBuf, String> {
        if let Some((_, path)) = self.written.iter().find(|(saved, _)| saved == native_path) {
            return Ok(path.clone());
        }
        let name = saved_name(native_path)?;
        let failed = |error: std::io::Error| {
            format!("its embedded copy could not be written as a .cube file: {error}")
        };
        let root = match &self.directory {
            Some(directory) => directory.path().to_owned(),
            None => {
                let directory = tempfile::tempdir().map_err(failed)?;
                let root = directory.path().to_owned();
                self.directory = Some(directory);
                root
            }
        };
        let folder = root.join(self.written.len().to_string());
        std::fs::create_dir(&folder).map_err(failed)?;
        let path = folder.join(name);
        std::fs::write(&path, embedded.cube_text()).map_err(failed)?;
        self.written.push((native_path.to_owned(), path.clone()));
        Ok(path)
    }

    fn asset(&mut self, path: PathBuf) -> ResolvedLut {
        if let Some(found) = self.files.iter().find(|found| found.path == path) {
            return found.clone();
        }
        let resolved = ResolvedLut {
            asset_id: AssetId::from_trusted(format!("premiere-lut-{}", self.files.len() + 1)),
            path,
        };
        self.files.push(resolved.clone());
        resolved
    }

    /// Add every located `.cube` as a `Lut` asset.
    pub(crate) fn package(
        &self,
        mut builder: TesseractFileBuilder,
    ) -> Result<TesseractFileBuilder> {
        for lut in &self.files {
            builder = builder.add_asset(lut.asset_id.as_str(), &lut.path, AssetKind::Other)?;
        }
        Ok(builder)
    }
}

/// The `.cube` file a saved Lumetri LUT path names: the file of that name
/// beside the project, else the saved path. A file must declare a
/// `LUT_3D_SIZE`.
fn locate(root: &Path, native_path: &str) -> std::result::Result<PathBuf, String> {
    let name = saved_name(native_path)?;
    let candidates = [root.join(name), PathBuf::from(native_path)];
    let Some(path) = candidates.iter().find(|path| path.is_file()) else {
        return Err(format!(
            "its .cube file {name:?} was found neither beside the project nor at the saved path {native_path:?}"
        ));
    };
    let text = std::fs::read(path)
        .map_err(|error| format!("its .cube file {} cannot be read: {error}", path.display()))?;
    if !is_3d_cube(&text) {
        return Err(format!(
            "its file {} is not a 3D .cube LUT (no LUT_3D_SIZE)",
            path.display()
        ));
    }
    path.canonicalize().map_err(|error| {
        format!(
            "its .cube file {} cannot be resolved: {error}",
            path.display()
        )
    })
}

/// The file name of a saved LUT path (either separator).
fn saved_name(native_path: &str) -> std::result::Result<&str, String> {
    native_path
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| format!("the saved LUT path {native_path:?} names no file"))
}

fn is_3d_cube(bytes: &[u8]) -> bool {
    String::from_utf8_lossy(bytes)
        .lines()
        .any(|line| line.trim_start().starts_with("LUT_3D_SIZE"))
}

/// The packaged asset of `lut`, or why it has none.
pub(crate) fn lut_asset(lut: &PrLumetriLut) -> std::result::Result<&AssetId, String> {
    match &lut.file {
        LutFile::Found(found) => Ok(&found.asset_id),
        LutFile::Packaged(asset_id) => Ok(asset_id),
        LutFile::Missing(reason) => Err(reason.clone()),
        LutFile::Unresolved => Err(format!(
            "its .cube file {:?} was not prepared for packaging",
            lut.native_path
        )),
    }
}

/// Give every exported `lookTransform` LUT of `project` its file in the
/// package `media` folder of `output`, named after the archive's `.cube`
/// and distinct from the media names (case-insensitively), and save that
/// absolute path as its `native_path`. One asset is written once.
pub(crate) fn bind_luts(
    project: &mut PrProjectFile,
    file: &TesseractFile,
    output: &Path,
) -> Result<()> {
    let mut used: BTreeSet<String> = project
        .media
        .values()
        .map(|media| media.name.to_lowercase())
        .collect();
    let mut bound: Vec<ResolvedLut> = Vec::new();
    let mut failure = None;
    for sequence in &mut project.sequences {
        for_each_lut(sequence, &mut |lut| {
            let LutFile::Packaged(asset_id) = &lut.file else {
                return;
            };
            if let Some(found) = bound.iter().find(|found| &found.asset_id == asset_id) {
                lut.native_path = path_text(&found.path);
                lut.file = LutFile::Found(found.clone());
                return;
            }
            match lut_file_name(file, asset_id, &mut used) {
                Ok(name) => {
                    let found = ResolvedLut {
                        path: output.join("media").join(name),
                        asset_id: asset_id.clone(),
                    };
                    lut.native_path = path_text(&found.path);
                    lut.file = LutFile::Found(found.clone());
                    bound.push(found);
                }
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        });
    }
    failure.map_or(Ok(()), Err)
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// A file name for `asset_id` in the package `media` folder: its archive
/// name when ASCII and unused, else `premiere-lut-<n>.cube`.
fn lut_file_name(
    file: &TesseractFile,
    asset_id: &AssetId,
    used: &mut BTreeSet<String>,
) -> Result<String> {
    let descriptor = file
        .metadata()
        .assets
        .get(asset_id.as_str())
        .ok_or_else(|| unsupported(format!("lookTransform asset {asset_id:?} is not packaged")))?;
    let original = Path::new(&descriptor.path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if !original.to_ascii_lowercase().ends_with(".cube") {
        return Err(unsupported(format!(
            "lookTransform asset {asset_id:?} is not a .cube file, which Lumetri Color > Creative > Look requires"
        )));
    }
    let mut chosen = original.to_owned();
    let mut suffix = 1;
    while !chosen.is_ascii() || used.contains(&chosen.to_lowercase()) {
        chosen = format!("premiere-lut-{suffix:03}.cube");
        suffix += 1;
    }
    used.insert(chosen.to_lowercase());
    Ok(chosen)
}

/// The custom LUTs of the effects that `sequence` writes, as
/// [`for_each_lut`] visits them.
fn luts<'a>(sequence: &'a PrSequence, found: &mut Vec<&'a PrLumetriLut>) {
    fn effects<'a>(effects: &'a [PrEffect], found: &mut Vec<&'a PrLumetriLut>) {
        found.extend(effects.iter().filter_map(|effect| match &effect.params {
            PrEffectParams::LumetriInputLut(lut) => Some(lut),
            PrEffectParams::LumetriLook(look) => Some(&look.lut),
            _ => None,
        }));
    }
    for track in &sequence.video_tracks {
        for item in &track.items {
            if let PrVideoItem::Media(clip) = item {
                effects(&clip.effects, found);
                if let Some(source) = &clip.source_effects {
                    effects(&source.effects, found);
                }
            }
        }
        for nest in &track.nests {
            effects(&nest.effects, found);
            luts(&nest.sequence, found);
        }
    }
}

/// The asset and `media` file name of each LUT file that [`bind_luts`]
/// chose for `project`, each once.
pub(crate) fn bound_lut_files(project: &PrProjectFile) -> Vec<(AssetId, String)> {
    let mut found = Vec::new();
    for sequence in &project.sequences {
        luts(sequence, &mut found);
    }
    let mut files: Vec<(AssetId, String)> = Vec::new();
    for lut in found {
        let LutFile::Found(bound) = &lut.file else {
            continue;
        };
        if files
            .iter()
            .any(|(asset_id, _)| asset_id == &bound.asset_id)
        {
            continue;
        }
        let name = bound
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        files.push((bound.asset_id.clone(), name));
    }
    files
}

/// Write the bound LUT files of `project` into `directory/media`, each
/// checked against its archive digest.
pub(crate) fn copy_luts(
    file: &TesseractFile,
    directory: &Path,
    files: &[(AssetId, String)],
) -> Result<()> {
    for (asset_id, name) in files {
        let asset = file.asset(asset_id.as_str())?;
        let target = directory.join("media").join(name);
        let mut output = File::create_new(&target)?;
        std::io::copy(&mut asset.open()?, &mut output)?;
        output.sync_all()?;
        if hash(&target)? != asset.descriptor().sha256 {
            return Err(unsupported(
                "packaged LUT changed after converting the Premiere project",
            ));
        }
        if !is_3d_cube(&std::fs::read(&target)?) {
            return Err(unsupported(format!(
                "lookTransform asset {asset_id:?} is not a 3D .cube LUT (no LUT_3D_SIZE)"
            )));
        }
    }
    Ok(())
}
