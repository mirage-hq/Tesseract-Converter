use super::*;

fn item(source: &Path, path: &str) -> InputArtifact {
    InputArtifact::capture(source.to_owned(), Artifact::media(path)).unwrap()
}

#[test]
fn scoped_aep_packages_keep_colliding_local_names_separate() {
    let root = tempfile::tempdir().unwrap();
    let mut inputs = Vec::new();
    for (scope, bytes) in [(1, b"first".as_slice()), (2, b"second".as_slice())] {
        let stage = root.path().join(format!("stage-{scope}"));
        fs::create_dir_all(stage.join("media")).unwrap();
        fs::write(stage.join("project.aep"), bytes).unwrap();
        fs::write(stage.join("media/picture.png"), bytes).unwrap();
        inputs.extend(
            scoped_aep_inputs(
                scope,
                &stage,
                &[
                    Artifact::project("project.aep"),
                    Artifact::media("media/picture.png"),
                ],
            )
            .unwrap(),
        );
    }
    let assembly = root.path().join("assembly");
    fs::create_dir(&assembly).unwrap();
    let artifacts = assemble(&assembly, &inputs).unwrap();
    assert_eq!(artifacts.len(), 4);
    for (scope, bytes) in [(1, b"first".as_slice()), (2, b"second".as_slice())] {
        assert_eq!(
            fs::read(assembly.join(scoped_aep_path(scope).unwrap())).unwrap(),
            bytes
        );
        assert_eq!(
            fs::read(assembly.join(format!("media/ae-{scope:04}/media/picture.png"))).unwrap(),
            bytes
        );
    }
}

#[test]
fn accepted_stage_drift_rejects_before_copying_any_artifact() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::write(&source, b"before").unwrap();
    let inputs = vec![item(&source, "first"), item(&source, "second")];
    fs::write(&source, b"changed").unwrap();
    let assembly = tempfile::tempdir_in(root.path()).unwrap();
    assert!(assemble(assembly.path(), &inputs)
        .unwrap_err()
        .to_string()
        .contains("changed before assembly"));
    assert_eq!(fs::read_dir(assembly.path()).unwrap().count(), 0);
}

#[test]
fn scoped_aep_manifest_rejects_missing_multiple_and_escaping_projects() {
    let root = tempfile::tempdir().unwrap();
    for artifacts in [
        vec![],
        vec![Artifact::project("other.aep")],
        vec![
            Artifact::project("project.aep"),
            Artifact::project("project.aep"),
        ],
        vec![
            Artifact::project("project.aep"),
            Artifact::media("../escape"),
        ],
        vec![
            Artifact::project("project.aep"),
            Artifact::media("other/picture.png"),
        ],
        vec![
            Artifact::project("project.aep"),
            Artifact::media("media/CON.png"),
        ],
    ] {
        assert!(scoped_aep_inputs(1, root.path(), &artifacts).is_err());
    }
    assert!(scoped_aep_path(0).is_err());
    assert!(scoped_aep_path(10000).is_err());
    assert_eq!(
        scoped_aep_path(9999).unwrap(),
        Path::new("media/ae-9999/compositions.aep")
    );
}

#[cfg(unix)]
#[test]
fn scoped_aep_manifest_does_not_follow_media_directory_symlinks() {
    let root = tempfile::tempdir().unwrap();
    let stage = root.path().join("stage");
    let outside = root.path().join("outside");
    fs::create_dir(&stage).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(stage.join("project.aep"), b"aep").unwrap();
    fs::write(outside.join("picture.png"), b"outside").unwrap();
    std::os::unix::fs::symlink(&outside, stage.join("media")).unwrap();
    let error = scoped_aep_inputs(
        1,
        &stage,
        &[
            Artifact::project("project.aep"),
            Artifact::media("media/picture.png"),
        ],
    )
    .unwrap_err();
    assert!(error.to_string().contains("symlink"));
}

#[test]
fn assembly_rejects_collisions_and_escaping_paths_before_copying() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::write(&source, b"content").unwrap();
    for paths in [
        vec!["media/a", "media/A"],
        vec!["media/a", "media/a/b"],
        vec!["Media/a", "media/b"],
        vec!["media/a."],
        vec!["media/a:b"],
        vec!["../escape"],
        vec!["/absolute"],
        vec!["./local"],
    ] {
        let assembly = tempfile::tempdir_in(root.path()).unwrap();
        let inputs: Vec<_> = paths.iter().map(|path| item(&source, path)).collect();
        assert!(assemble(assembly.path(), &inputs).is_err());
        assert_eq!(fs::read_dir(assembly.path()).unwrap().count(), 0);
    }
}

#[test]
fn windows_device_stems_reject_before_any_copy() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::write(&source, b"media").unwrap();
    for name in [
        "CON.png",
        "aUx.mov",
        "NUL.wav",
        "PRN.txt",
        "COM1.mp4",
        "com9.aep",
        "LPT1.png",
        "lpt9.wav",
        "CONIN$.txt",
        "CONOUT$.txt",
        "CLOCK$.txt",
        "CON .png",
    ] {
        let assembly = tempfile::tempdir_in(root.path()).unwrap();
        assert!(
            assemble(assembly.path(), &[item(&source, &format!("media/{name}"))]).is_err(),
            "{name}"
        );
        assert_eq!(fs::read_dir(assembly.path()).unwrap().count(), 0);
    }
    let assembly = tempfile::tempdir_in(root.path()).unwrap();
    assert!(assemble(assembly.path(), &[item(&source, "CON/clip.mp4")]).is_err());
    for name in ["COM10.mp4", "console.png", "_AUX.wav"] {
        let assembly = tempfile::tempdir_in(root.path()).unwrap();
        assemble(assembly.path(), &[item(&source, &format!("media/{name}"))]).unwrap();
    }
}

#[test]
fn assembly_preserves_aep_local_dependency_layout_and_bytes() {
    let root = tempfile::tempdir().unwrap();
    let aep = root.path().join("project.aep");
    let media = root.path().join("image.png");
    fs::write(&aep, b"project bytes").unwrap();
    fs::write(&media, b"media bytes").unwrap();
    let assembly = root.path().join("assembly");
    fs::create_dir(&assembly).unwrap();
    let artifacts = assemble(
        &assembly,
        &[
            item(&aep, "media/compositions.aep"),
            item(&media, "media/media/image.png"),
        ],
    )
    .unwrap();
    assert_eq!(artifacts.len(), 2);
    assert_eq!(
        fs::read(assembly.join("media/media/image.png")).unwrap(),
        b"media bytes"
    );
    let source_hash = sha256(&aep).unwrap();
    let output = root.path().join("output");
    publish(&assembly, &output, &artifacts, &aep, &source_hash).unwrap();
    assert_eq!(
        fs::read(output.join("media/compositions.aep")).unwrap(),
        b"project bytes"
    );
    assert_eq!(
        fs::read(output.join("media/media/image.png")).unwrap(),
        b"media bytes"
    );
}

#[test]
fn publication_failure_removes_only_owned_links_and_directories() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::write(&source, b"source").unwrap();
    let digest = sha256(&source).unwrap();
    let assembly = root.path().join("assembly");
    fs::create_dir(&assembly).unwrap();
    fs::write(assembly.join("first"), b"first").unwrap();
    let output = root.path().join("output");
    let files = [Artifact::project("first"), Artifact::media("media/missing")];
    assert!(publish(&assembly, &output, &files, &source, &digest).is_err());
    assert!(!output.exists());
    assert_eq!(fs::read(assembly.join("first")).unwrap(), b"first");
    assert_eq!(fs::read(&source).unwrap(), b"source");
}

#[test]
fn copy_fallback_verifies_bytes_and_rolls_back_partial_files() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::write(&source, b"verified").unwrap();
    let output = root.path().join("output");
    fs::create_dir(&output).unwrap();
    let mut installed = Installation {
        files: Vec::new(),
        directories: vec![output.clone()],
        committed: false,
    };
    let no_links = |_: &Path, _: &Path| Err(std::io::Error::from(std::io::ErrorKind::Unsupported));
    install_file(&source, &output.join("first"), &mut installed, no_links).unwrap();
    assert_eq!(fs::read(output.join("first")).unwrap(), b"verified");
    assert!(install_file(
        &root.path().join("missing"),
        &output.join("partial"),
        &mut installed,
        no_links
    )
    .is_err());
    assert!(output.join("partial").exists());
    installed.rollback().unwrap();
    assert!(!output.exists());
    assert_eq!(fs::read(&source).unwrap(), b"verified");
    // A refused link must never make the copy fallback overwrite a foreign file.
    let foreign = root.path().join("foreign");
    fs::write(&foreign, b"keep").unwrap();
    assert!(install_file(&source, &foreign, &mut installed, no_links).is_err());
    assert_eq!(fs::read(&foreign).unwrap(), b"keep");
}

#[test]
fn source_change_and_raced_destination_are_not_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::write(&source, b"source").unwrap();
    let digest = sha256(&source).unwrap();
    let output = root.path().join("output");
    fresh_output(&output).unwrap();
    fs::write(&source, b"changed").unwrap();
    assert!(publish(root.path(), &output, &[], &source, &digest).is_err());
    assert!(!output.exists());
    fs::write(&source, b"source").unwrap();
    fs::create_dir(&output).unwrap();
    fs::write(output.join("foreign"), b"keep").unwrap();
    assert!(publish(root.path(), &output, &[], &source, &digest).is_err());
    assert_eq!(fs::read(output.join("foreign")).unwrap(), b"keep");
}
