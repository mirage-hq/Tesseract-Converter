use super::*;

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, MediaMap) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let input = root.join("source.aep");
    fs::write(&input, b"native project bytes").unwrap();
    fs::create_dir(root.join("prepared")).unwrap();
    let mut replacements = Vec::new();
    for (folder, bytes) in [
        ("first", b"first original".as_slice()),
        ("second", b"second original".as_slice()),
    ] {
        fs::create_dir(root.join(folder)).unwrap();
        let original = root.join(folder).join("same.mov");
        fs::write(&original, bytes).unwrap();
        let replacement = PathBuf::from(format!("{folder}.mp4"));
        let prepared = root.join("prepared").join(&replacement);
        fs::write(&prepared, format!("prepared {folder}")).unwrap();
        replacements.push(MediaReplacement {
            original: canonical(&original).unwrap(),
            original_sha256: sha256_file(&original).unwrap(),
            replacement,
            replacement_sha256: sha256_file(&prepared).unwrap(),
        });
    }
    let map = MediaMap {
        version: 1,
        source: MediaMapSource {
            format: "after-effects".into(),
            sha256: sha256_file(&input).unwrap(),
            target: "4".into(),
        },
        replacements,
    };
    let map_path = root.join("prepared/media-map.json");
    save(&map_path, &map);
    (directory, input, map_path, map)
}

fn save(path: &Path, map: &MediaMap) {
    fs::write(path, serde_json::to_vec(map).unwrap()).unwrap();
}

#[test]
fn multiple_same_named_sources_and_repeated_lookups_preserve_identity() {
    let (_directory, input, path, map) = fixture();
    let validated = ValidatedMediaMap::load(&path).unwrap();
    validated
        .validate_for(&input, "after-effects", "4")
        .unwrap();
    let first = validated
        .replacement_for(&map.replacements[0].original)
        .unwrap()
        .unwrap();
    assert_eq!(first.file_name().unwrap(), "first.mp4");
    assert_eq!(
        Some(first),
        validated
            .replacement_for(&map.replacements[0].original)
            .unwrap()
    );
    let second = validated
        .replacement_for(&map.replacements[1].original)
        .unwrap()
        .unwrap();
    assert_eq!(second.file_name().unwrap(), "second.mp4");
    assert_ne!(first, second);
    assert!(validated.replacement_for(&input).unwrap().is_none());
}

#[test]
fn review_bare_relative_media_map_matches_explicit_paths() {
    let marker = Path::new("relative-media-map-child");
    if !marker.exists() {
        let (_directory, _input, absolute, _map) = fixture();
        let base = absolute.parent().unwrap();
        fs::write(base.join(marker), b"child").unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "media::tests::review_bare_relative_media_map_matches_explicit_paths",
                "--nocapture",
            ])
            .current_dir(base)
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let absolute = fs::canonicalize("media-map.json").unwrap();
    let expected = ValidatedMediaMap::load(&absolute).unwrap();
    for relative in [Path::new("media-map.json"), Path::new("./media-map.json")] {
        let actual = ValidatedMediaMap::load(relative).unwrap();
        assert_eq!(actual.base, expected.base);
        for entry in &expected.entries {
            assert_eq!(
                actual.replacement_for(&entry.original).unwrap(),
                Some(entry.replacement.as_path())
            );
        }
    }
}

#[test]
fn source_project_target_and_format_are_bound() {
    let (_directory, input, path, _map) = fixture();
    let validated = ValidatedMediaMap::load(&path).unwrap();
    assert!(validated
        .validate_for(&input, "after-effects", "5")
        .is_err());
    assert!(validated.validate_for(&input, "premiere", "4").is_err());
    fs::write(&input, b"changed project").unwrap();
    assert!(validated
        .validate_for(&input, "after-effects", "4")
        .is_err());
}

#[test]
fn original_and_prepared_mutation_are_rechecked_before_publication() {
    for change_original in [true, false] {
        let (_directory, input, path, map) = fixture();
        let validated = ValidatedMediaMap::load(&path).unwrap();
        let changed = if change_original {
            map.replacements[0].original.clone()
        } else {
            path.parent()
                .unwrap()
                .join(&map.replacements[0].replacement)
        };
        fs::write(&changed, b"modified").unwrap();
        assert!(validated
            .validate_for(&input, "after-effects", "4")
            .is_err());
        assert!(ValidatedMediaMap::load(&path).is_err());
    }
}

#[test]
fn duplicate_sources_versions_and_path_escape_are_rejected() {
    let (_directory, _input, path, map) = fixture();
    let mut duplicate = map.clone();
    duplicate
        .replacements
        .push(duplicate.replacements[0].clone());
    save(&path, &duplicate);
    assert!(ValidatedMediaMap::load(&path).is_err());
    let mut future = map.clone();
    future.version = 2;
    save(&path, &future);
    assert!(ValidatedMediaMap::load(&path).is_err());
    for replacement in [
        PathBuf::from("../first/same.mov"),
        map.replacements[0].original.clone(),
    ] {
        let mut escaping = map.clone();
        escaping.replacements[0].replacement = replacement;
        save(&path, &escaping);
        assert!(ValidatedMediaMap::load(&path).is_err());
    }
}

#[cfg(unix)]
#[test]
fn symlink_escape_and_retargeting_are_rejected() {
    use std::os::unix::fs::symlink;
    let (_directory, input, path, mut map) = fixture();
    let link = path.parent().unwrap().join("alias.mp4");
    symlink("first.mp4", &link).unwrap();
    map.replacements[0].replacement = PathBuf::from("alias.mp4");
    save(&path, &map);
    let validated = ValidatedMediaMap::load(&path).unwrap();
    fs::remove_file(&link).unwrap();
    symlink(&map.replacements[0].original, &link).unwrap();
    assert!(validated
        .validate_for(&input, "after-effects", "4")
        .is_err());
    assert!(ValidatedMediaMap::load(&path).is_err());
}
