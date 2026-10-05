//! Fresh-output assembly and recoverable-error rollback (not crash atomicity).

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

use anyhow::{ensure, Context};
use fx_conv::{Artifact, ArtifactKind};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub(super) struct InputArtifact {
    pub source: PathBuf,
    pub artifact: Artifact,
    digest: [u8; 32],
}

impl InputArtifact {
    // Pin bytes when accepting an owned stage, not only after assembly starts.
    // This is a freshness check, not a filesystem lock or snapshot primitive.
    pub(super) fn capture(source: PathBuf, artifact: Artifact) -> anyhow::Result<Self> {
        ensure!(
            fs::symlink_metadata(&source)?.is_file(),
            "staged artifact is not a regular file"
        );
        let digest = sha256(&source)?;
        Ok(Self {
            source,
            artifact,
            digest,
        })
    }
}

/// Connect already-staged outputs, without selecting or certifying FX scopes.
/// Semantic/dependency admission must precede production use of this bridge.
/// Missing, duplicate or unexpected foreign projects reject before accepting files.
pub(super) fn picture_stage_inputs(
    premiere: &premiere_file::StagedPicturePremiereExport,
    scopes: &[(u16, aftereffects_file::StagedAfterEffectsPictureExport)],
) -> anyhow::Result<Vec<InputArtifact>> {
    ensure!(scopes.len() <= 256, "picture scope count exceeds 256");
    let mut supplied = BTreeSet::new();
    let mut count = premiere.report().artifacts.len();
    ensure!(count <= 4096, "hybrid artifact count exceeds 4096");
    for (scope, stage) in scopes {
        ensure!(
            supplied.insert(scoped_aep_path(*scope)?),
            "duplicate AEP scope"
        );
        count = count
            .checked_add(stage.report().artifacts.len())
            .context("hybrid artifact count overflow")?;
        ensure!(count <= 4096, "hybrid artifact count exceeds 4096");
    }
    let required: BTreeSet<_> = premiere.after_effects_paths().iter().collect();
    ensure!(
        required == supplied.iter().collect(),
        "supplied AEP scopes do not match Premiere foreign requirements"
    );
    let mut inputs = Vec::with_capacity(count);
    for artifact in &premiere.report().artifacts {
        let input =
            InputArtifact::capture(premiere.directory().join(&artifact.path), artifact.clone())?;
        inputs.push(input);
    }
    let mut projects = inputs
        .iter()
        .filter(|input| input.artifact.kind == ArtifactKind::Project);
    let project = projects
        .next()
        .context("captured Premiere project artifact missing")?;
    ensure!(
        projects.next().is_none() && project.artifact.path == Path::new("project.prproj"),
        "Premiere stage must declare exactly one project"
    );
    ensure!(
        &project.digest == premiere.generated_project_sha256(),
        "staged Premiere project changed after generation"
    );
    for (scope, stage) in scopes {
        let scoped = scoped_aep_inputs(*scope, stage.directory(), &stage.report().artifacts)?;
        let project = scoped
            .iter()
            .find(|input| input.artifact.kind == ArtifactKind::Project)
            .context("captured AEP project artifact missing")?;
        ensure!(
            &project.digest == stage.generated_project_sha256(),
            "staged AEP project changed after generation"
        );
        inputs.extend(scoped);
    }
    Ok(inputs)
}

pub(super) fn scoped_aep_path(scope: u16) -> anyhow::Result<PathBuf> {
    ensure!(
        (1..=9999).contains(&scope),
        "AEP scope index must be 0001–9999"
    );
    Ok(PathBuf::from(format!(
        "media/ae-{scope:04}/compositions.aep"
    )))
}

/// Retain each AEP's own `media/` and `fonts/` subtrees; same basenames/GUIDs
/// in different files are not deduplication keys. The After Effects stage
/// packages embedded Text fonts as `fonts/<sha256>.<ext>` plus
/// `fonts/manifest.json` next to its project, so those stay beside the scoped
/// AEP. Validate local path shapes before opening files.
pub(super) fn scoped_aep_inputs(
    scope: u16,
    directory: &Path,
    artifacts: &[Artifact],
) -> anyhow::Result<Vec<InputArtifact>> {
    let project = scoped_aep_path(scope)?;
    let parent = project.parent().context("AEP scope directory missing")?;
    ensure!(artifacts.len() <= 4096, "AEP artifact count exceeds 4096");
    let mut projects = 0;
    for artifact in artifacts {
        let path = &artifact.path;
        ensure!(
            path.to_str().is_some_and(|path| path.len() <= 4096 && path.is_ascii())
                && path.components().count() <= 32
                && path.components().all(|part| matches!(part, Component::Normal(name) if name.to_str().is_some_and(portable_component)))
                && path.components().collect::<PathBuf>().as_os_str() == path.as_os_str(),
            "AEP artifact is not a bounded portable local path"
        );
        match artifact.kind {
            ArtifactKind::Project => {
                ensure!(
                    path == Path::new("project.aep"),
                    "unexpected AEP project artifact"
                );
                projects += 1;
            }
            ArtifactKind::Media => ensure!(
                (path.starts_with("media") || path.starts_with("fonts"))
                    && path.components().count() >= 2,
                "AEP media must remain within its local media or fonts subtree"
            ),
        }
    }
    ensure!(projects == 1, "AEP stage must declare exactly one project");
    ensure!(
        fs::symlink_metadata(directory)?.is_dir(),
        "AEP stage must be an owned directory, not a link"
    );
    artifacts
        .iter()
        .map(|artifact| {
            let mut source = directory.to_owned();
            for part in artifact.path.components() {
                source.push(part);
                ensure!(
                    !fs::symlink_metadata(&source)?.file_type().is_symlink(),
                    "AEP artifact traverses a symlink"
                );
            }
            let path = match artifact.kind {
                ArtifactKind::Project => project.clone(),
                ArtifactKind::Media => parent.join(&artifact.path),
            };
            InputArtifact::capture(
                source,
                Artifact {
                    path,
                    kind: artifact.kind,
                },
            )
        })
        .collect()
}

pub(super) fn fresh_output(path: &Path) -> anyhow::Result<PathBuf> {
    let name = path
        .file_name()
        .context("output needs a fresh directory name")?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    let output = parent.join(name);
    ensure!(
        parent.is_dir() && output.to_str().is_some(),
        "output needs an existing parent and UTF-8 path"
    );
    match fs::symlink_metadata(&output) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(output),
        Err(error) => Err(error.into()),
        Ok(_) => anyhow::bail!("output already exists; choose a fresh directory"),
    }
}

pub(super) fn sha256(path: &Path) -> anyhow::Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest.finalize().into())
}

pub(super) fn unchanged(path: &Path, expected: &[u8]) -> anyhow::Result<()> {
    ensure!(
        sha256(path)? == expected,
        "source archive changed during hybrid conversion"
    );
    Ok(())
}

fn portable_component(name: &str) -> bool {
    if name.is_empty()
        || !name.is_ascii()
        || name.ends_with(['.', ' '])
        || name.chars().any(|character| {
            character.is_ascii_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
        })
    {
        return false;
    }
    // Windows device names remain reserved with extensions and regardless of
    // case. Check every component, including directory names, before copying.
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    let device = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
    );
    let bytes = stem.as_bytes();
    let numbered_port = bytes.len() == 4
        && (bytes.starts_with(b"COM") || bytes.starts_with(b"LPT"))
        && (b'1'..=b'9').contains(&bytes[3]);
    !device && !numbered_port
}

pub(super) fn assemble(
    directory: &Path,
    inputs: &[InputArtifact],
) -> anyhow::Result<Vec<Artifact>> {
    let mut names = BTreeSet::new();
    let mut spellings = BTreeMap::new();
    for input in inputs {
        let path = &input.artifact.path;
        ensure!(
            !path.as_os_str().is_empty()
                && path.to_str().is_some_and(str::is_ascii)
                && path.components().all(|component| match component {
                    Component::Normal(name) => name.to_str().is_some_and(portable_component),
                    _ => false,
                })
                && path.components().collect::<PathBuf>().as_os_str() == path.as_os_str(),
            "package artifact must use a portable ASCII relative path without reserved device names: {}", 
            path.display()
        );
        ensure!(
            names.insert(path.to_string_lossy().to_ascii_lowercase()),
            "case-folded package artifact collision: {}",
            path.display()
        );
        for prefix in path
            .ancestors()
            .filter(|prefix| !prefix.as_os_str().is_empty())
        {
            let key = prefix.to_string_lossy().to_ascii_lowercase();
            if let Some(previous) = spellings.insert(key, prefix.to_owned()) {
                ensure!(
                    previous == prefix,
                    "case-folded directory spelling collision: {}",
                    prefix.display()
                );
            }
        }
        ensure!(
            fs::symlink_metadata(&input.source)?.is_file(),
            "staged artifact is not a regular file"
        );
        ensure!(
            sha256(&input.source)? == input.digest,
            "accepted staged artifact changed before assembly: {}",
            path.display()
        );
    }
    for input in inputs {
        for parent in input
            .artifact
            .path
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            ensure!(
                !names.contains(&parent.to_string_lossy().to_ascii_lowercase()),
                "package file/directory collision: {}",
                parent.display()
            );
        }
    }
    for input in inputs {
        let destination = directory.join(&input.artifact.path);
        fs::create_dir_all(destination.parent().context("artifact parent missing")?)?;
        let expected = input.digest;
        let mut source = File::open(&input.source)?;
        let mut target = File::create_new(&destination)?;
        std::io::copy(&mut source, &mut target)?;
        target.flush()?;
        target.sync_all()?;
        ensure!(
            sha256(&destination)? == expected && sha256(&input.source)? == expected,
            "staged artifact changed during assembly: {}",
            input.artifact.path.display()
        );
    }
    Ok(inputs.iter().map(|input| input.artifact.clone()).collect())
}

/// Create exclusively, then link/copy only declared files; never rename over a
/// raced destination. Rollback removes only paths created by this call.
pub(super) fn publish(
    assembly: &Path,
    output: &Path,
    artifacts: &[Artifact],
    source: &Path,
    source_hash: &[u8],
) -> anyhow::Result<()> {
    unchanged(source, source_hash)?;
    fs::create_dir(output).context("create fresh hybrid package")?;
    let mut installed = Installation {
        files: Vec::new(),
        directories: vec![output.to_owned()],
        committed: false,
    };
    let result = (|| {
        let mut parents = BTreeSet::new();
        for artifact in artifacts {
            for parent in artifact
                .path
                .ancestors()
                .skip(1)
                .filter(|p| !p.as_os_str().is_empty())
            {
                parents.insert(parent.to_owned());
            }
        }
        let mut parents: Vec<_> = parents.into_iter().collect();
        parents.sort_by_key(|path| path.components().count());
        for parent in parents {
            let directory = output.join(parent);
            fs::create_dir(&directory)?;
            installed.directories.push(directory);
        }
        for artifact in artifacts {
            let destination = output.join(&artifact.path);
            install_file(
                &assembly.join(&artifact.path),
                &destination,
                &mut installed,
                |source, target| fs::hard_link(source, target),
            )
            .with_context(|| format!("publish {} without overwriting", artifact.path.display()))?;
        }
        unchanged(source, source_hash)
    })();
    if let Err(error) = result {
        if let Err(cleanup) = installed.rollback() {
            return Err(error.context(format!("package rollback also failed: {cleanup:#}")));
        }
        return Err(error);
    }
    installed.committed = true;
    Ok(())
}

fn install_file(
    source: &Path,
    destination: &Path,
    installed: &mut Installation,
    link: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> anyhow::Result<()> {
    if link(source, destination).is_ok() {
        installed.files.push(destination.to_owned());
        return Ok(());
    }
    // Filesystems without hard links use an exclusive, tracked copy. Track it
    // immediately so a read/write/hash error also removes a partially copied file.
    let mut output = File::create_new(destination)?;
    installed.files.push(destination.to_owned());
    let expected = sha256(source)?;
    let mut input = File::open(source)?;
    std::io::copy(&mut input, &mut output)?;
    output.set_permissions(input.metadata()?.permissions())?;
    output.sync_all()?;
    ensure!(
        sha256(destination)? == expected && sha256(source)? == expected,
        "artifact changed during publication copy"
    );
    Ok(())
}

struct Installation {
    files: Vec<PathBuf>,
    directories: Vec<PathBuf>,
    committed: bool,
}

impl Installation {
    fn rollback(&mut self) -> anyhow::Result<()> {
        let mut errors = Vec::new();
        for path in self.files.drain(..).rev() {
            if let Err(error) = fs::remove_file(&path) {
                errors.push(format!("{}: {error}", path.display()));
            }
        }
        for path in self.directories.drain(..).rev() {
            if let Err(error) = fs::remove_dir(&path) {
                errors.push(format!("{}: {error}", path.display()));
            }
        }
        ensure!(errors.is_empty(), "{}", errors.join("; "));
        Ok(())
    }
}

impl Drop for Installation {
    fn drop(&mut self) {
        if !self.committed {
            // Best effort on unwind; ordinary error paths report rollback errors.
            let _ = self.rollback();
        }
    }
}

#[cfg(test)]
mod tests;
