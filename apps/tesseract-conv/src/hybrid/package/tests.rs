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
        fs::create_dir_all(stage.join("fonts")).unwrap();
        fs::write(stage.join("project.aep"), bytes).unwrap();
        fs::write(stage.join("media/picture.png"), bytes).unwrap();
        // The AE stage packages embedded Text fonts beside its project.
        fs::write(stage.join("fonts/face.otf"), bytes).unwrap();
        fs::write(stage.join("fonts/manifest.json"), bytes).unwrap();
        inputs.extend(
            scoped_aep_inputs(
                scope,
                &stage,
                &[
                    Artifact::project("project.aep"),
                    Artifact::media("media/picture.png"),
                    Artifact::media("fonts/face.otf"),
                    Artifact::media("fonts/manifest.json"),
                ],
            )
            .unwrap(),
        );
    }
    let assembly = root.path().join("assembly");
    fs::create_dir(&assembly).unwrap();
    let artifacts = assemble(&assembly, &inputs).unwrap();
    assert_eq!(artifacts.len(), 8);
    for (scope, bytes) in [(1, b"first".as_slice()), (2, b"second".as_slice())] {
        assert_eq!(
            fs::read(assembly.join(scoped_aep_path(scope).unwrap())).unwrap(),
            bytes
        );
        for local in ["media/picture.png", "fonts/face.otf", "fonts/manifest.json"] {
            assert_eq!(
                fs::read(assembly.join(format!("media/ae-{scope:04}/{local}"))).unwrap(),
                bytes
            );
        }
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
        vec![Artifact::project("project.aep"), Artifact::media("fonts")],
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

#[cfg(unix)]
#[test]
fn publication_from_read_only_assembly_preserves_bytes_and_rollback() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::write(&source, b"source").unwrap();
    let digest = sha256(&source).unwrap();
    let assembly = root.path().join("assembly");
    fs::create_dir(&assembly).unwrap();
    fs::write(assembly.join("first"), b"artifact").unwrap();
    let permissions = fs::metadata(&assembly).unwrap().permissions();
    fs::set_permissions(&assembly, fs::Permissions::from_mode(0o555)).unwrap();
    let output = root.path().join("output");
    let result = publish(
        &assembly,
        &output,
        &[Artifact::project("first")],
        &source,
        &digest,
    );
    let partial = root.path().join("partial");
    let failure = publish(
        &assembly,
        &partial,
        &[Artifact::project("first"), Artifact::media("missing")],
        &source,
        &digest,
    );
    fs::set_permissions(&assembly, permissions).unwrap();
    result.unwrap();
    assert_eq!(fs::read(output.join("first")).unwrap(), b"artifact");
    assert!(failure.is_err());
    assert!(!partial.exists());
    assert_eq!(fs::read(assembly.join("first")).unwrap(), b"artifact");
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
        directories: vec![InstalledPath {
            path: output.clone(),
            identity: Some(FileIdentity::Handle(
                same_file::Handle::from_path(&output).unwrap(),
            )),
        }],
        committed: false,
    };
    let no_links = |_: &Path, _: &Path| Err(std::io::Error::from(std::io::ErrorKind::Unsupported));
    install_file(&source, &output.join("first"), &mut installed, no_links).unwrap();
    assert_eq!(fs::read(output.join("first")).unwrap(), b"verified");
    let disappearing = root.path().join("disappearing");
    fs::write(&disappearing, b"removed before copy").unwrap();
    assert!(install_file(
        &disappearing,
        &output.join("partial"),
        &mut installed,
        |source, _| {
            fs::remove_file(source)?;
            Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
        }
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
fn directory_identity_failure_reports_and_preserves_unverified_paths() {
    for child in [false, true] {
        for replace in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("source");
            fs::write(&source, b"source").unwrap();
            let digest = sha256(&source).unwrap();
            let output = root.path().join("output");
            let failed = if child {
                output.join("b")
            } else {
                output.clone()
            };
            let original = root.path().join("original");
            let error = publish_with_directory_identity(
                root.path(),
                &output,
                &[Artifact::media("a/file"), Artifact::media("b/file")],
                &source,
                &digest,
                |path| {
                    if path == failed {
                        if replace {
                            fs::rename(path, &original)?;
                            fs::create_dir(path)?;
                        }
                        return Err(std::io::Error::other("injected directory identity failure"));
                    }
                    same_file::Handle::from_path(path)
                },
            )
            .unwrap_err();
            let message = format!("{error:#}");
            assert!(message.contains("injected directory identity failure"));
            assert!(message.contains(&failed.display().to_string()));
            assert!(message.contains("ownership unverified"));
            assert!(message.contains("manual inspection and cleanup required"));
            assert!(failed.is_dir());
            assert!(
                !output.join("a").exists(),
                "verified sibling must be cleaned"
            );
            if replace {
                assert!(original.is_dir());
            }
            // Publication has already dropped its Installation; neither the
            // unverified original nor an empty replacement may be retried.
            assert_eq!(fs::read_dir(&failed).unwrap().count(), 0);
        }
    }
}

#[test]
fn copied_file_identity_failure_reports_and_preserves_unverified_paths() {
    for replace in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let verified = root.path().join("verified");
        let destination = root.path().join("unverified");
        let original = root.path().join("original");
        let directory = root.path().join("verified-directory");
        fs::write(&source, b"source").unwrap();
        let mut installed = Installation {
            files: Vec::new(),
            directories: Vec::new(),
            committed: false,
        };
        installed
            .create_directory(&directory, &mut |path| same_file::Handle::from_path(path))
            .unwrap();
        install_file(&source, &verified, &mut installed, |source, target| {
            fs::hard_link(source, target)
        })
        .unwrap();
        let error = install_file_with_identity(
            &source,
            &destination,
            &mut installed,
            |_, _| Err(std::io::Error::from(std::io::ErrorKind::Unsupported)),
            |file| {
                if replace {
                    fs::rename(&destination, &original)?;
                    fs::write(&destination, b"replacement")?;
                }
                drop(file);
                Err(std::io::Error::other(
                    "injected copied-file identity failure",
                ))
            },
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("injected copied-file identity failure"));
        let cleanup = installed.rollback().unwrap_err().to_string();
        assert!(cleanup.contains(&destination.display().to_string()));
        assert!(cleanup.contains("ownership unverified"));
        assert!(cleanup.contains("manual inspection and cleanup required"));
        assert!(!verified.exists());
        assert!(!directory.exists());
        let expected = if replace {
            b"replacement".as_slice()
        } else {
            b""
        };
        assert_eq!(fs::read(&destination).unwrap(), expected);
        drop(installed);
        assert_eq!(fs::read(&destination).unwrap(), expected);
        if replace {
            assert_eq!(fs::read(&original).unwrap(), b"");
        }
    }
}

#[test]
fn rollback_preserves_same_bytes_file_replacement() {
    for use_copy in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("installed");
        let original = root.path().join("original");
        fs::write(&source, b"same bytes").unwrap();
        let mut installed = Installation {
            files: Vec::new(),
            directories: Vec::new(),
            committed: false,
        };
        install_file(&source, &destination, &mut installed, |source, target| {
            if use_copy {
                Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
            } else {
                fs::hard_link(source, target)
            }
        })
        .unwrap();
        // Keep the installed inode alive so its identity cannot be reused.
        fs::rename(&destination, &original).unwrap();
        fs::write(&destination, b"same bytes").unwrap();
        let result = installed.rollback();
        assert_eq!(fs::read(&destination).unwrap(), b"same bytes");
        assert!(
            result.is_err(),
            "replacement must be reported, copy={use_copy}"
        );
        drop(installed);
        assert_eq!(fs::read(&destination).unwrap(), b"same bytes");
        assert_eq!(fs::read(&original).unwrap(), b"same bytes");
        assert_eq!(fs::read(&source).unwrap(), b"same bytes");
    }
}

#[test]
fn rollback_preserves_directory_replacement() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("installed");
    let directory = root.path().join("directory");
    let original = root.path().join("original-directory");
    fs::write(&source, b"owned file").unwrap();
    fs::create_dir(&directory).unwrap();
    let mut installed = Installation {
        files: Vec::new(),
        directories: vec![InstalledPath {
            path: directory.clone(),
            identity: Some(FileIdentity::Handle(
                same_file::Handle::from_path(&directory).unwrap(),
            )),
        }],
        committed: false,
    };
    install_file(&source, &destination, &mut installed, |source, target| {
        fs::hard_link(source, target)
    })
    .unwrap();
    fs::rename(&directory, &original).unwrap();
    // An empty replacement must survive, not merely a nonempty directory that
    // remove_dir would already refuse to delete.
    fs::create_dir(&directory).unwrap();
    let result = installed.rollback();
    assert!(directory.is_dir());
    assert!(result.is_err(), "directory replacement must be reported");
    assert!(
        !destination.exists(),
        "unreplaced owned file must be removed"
    );
    drop(installed);
    assert!(directory.is_dir());
    assert!(original.is_dir());
    assert_eq!(fs::read(&source).unwrap(), b"owned file");
}

#[cfg(unix)]
#[test]
fn rollback_preserves_symlink_replacement() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("installed");
    let original = root.path().join("original");
    let foreign = root.path().join("foreign");
    fs::write(&source, b"owned file").unwrap();
    fs::write(&foreign, b"foreign file").unwrap();
    let mut installed = Installation {
        files: Vec::new(),
        directories: Vec::new(),
        committed: false,
    };
    install_file(&source, &destination, &mut installed, |source, target| {
        fs::hard_link(source, target)
    })
    .unwrap();
    fs::rename(&destination, &original).unwrap();
    std::os::unix::fs::symlink(&foreign, &destination).unwrap();
    let result = installed.rollback();
    assert_eq!(fs::read_link(&destination).unwrap(), foreign);
    assert!(result.is_err(), "symlink replacement must be reported");
    drop(installed);
    assert_eq!(fs::read_link(&destination).unwrap(), foreign);
    assert_eq!(fs::read(&foreign).unwrap(), b"foreign file");
    assert_eq!(fs::read(&original).unwrap(), b"owned file");
}

#[cfg(unix)]
#[test]
fn rollback_preserves_symlink_to_owned_inode() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("installed");
    fs::write(&source, b"owned file").unwrap();
    let mut installed = Installation {
        files: Vec::new(),
        directories: Vec::new(),
        committed: false,
    };
    install_file(&source, &destination, &mut installed, |source, target| {
        fs::hard_link(source, target)
    })
    .unwrap();
    fs::remove_file(&destination).unwrap();
    std::os::unix::fs::symlink(&source, &destination).unwrap();
    assert!(installed.rollback().is_err());
    assert_eq!(fs::read_link(&destination).unwrap(), source);
    drop(installed);
    assert_eq!(fs::read_link(&destination).unwrap(), source);
    assert_eq!(fs::read(&source).unwrap(), b"owned file");
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
