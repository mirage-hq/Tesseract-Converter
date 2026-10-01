//! Test-only access to byte-pinned fixtures; gzip changes storage, not evidence.

use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

const MAX_DECODED_BYTES: u64 = 16 * 1024 * 1024;

pub(crate) fn read(path: impl AsRef<Path>) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(path);
    let file = File::open(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let bytes = if path.extension().is_some_and(|extension| extension == "gz") {
        bounded(flate2::read::GzDecoder::new(file))
    } else {
        bounded(file)
    }
    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    assert!(
        bytes.len() as u64 <= MAX_DECODED_BYTES,
        "{} exceeds the decoded fixture byte limit",
        path.display()
    );
    bytes
}

fn bounded(reader: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(MAX_DECODED_BYTES + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[test]
fn compressed_references_match_all_original_provenance_pins() {
    use std::collections::HashSet;
    use std::path::PathBuf;

    use serde_json::Value;
    use sha2::{Digest, Sha256};

    fn verify(value: &Value, directory: &Path, checked: &mut HashSet<PathBuf>) {
        match value {
            Value::Object(object) => {
                if let Some(name) = object
                    .get("path")
                    .or_else(|| object.get("file"))
                    .and_then(Value::as_str)
                    .filter(|name| name.ends_with(".json.gz"))
                {
                    let path = directory.join(name);
                    let bytes = read(&path);
                    assert_eq!(
                        format!("{:x}", Sha256::digest(&bytes)),
                        object["sha256"].as_str().unwrap(),
                        "{} decoded hash",
                        path.display()
                    );
                    let size = object.get("size").or_else(|| object.get("bytes")).unwrap();
                    assert_eq!(
                        bytes.len() as u64,
                        size.as_u64().unwrap(),
                        "{}",
                        path.display()
                    );
                    assert!(
                        checked.insert(path),
                        "duplicate compressed reference: {name}"
                    );
                }
                for child in object.values() {
                    verify(child, directory, checked);
                }
            }
            Value::Array(values) => {
                for child in values {
                    verify(child, directory, checked);
                }
            }
            _ => {}
        }
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut stored = HashSet::new();
    let mut checked = HashSet::new();
    for entry in root.read_dir().unwrap() {
        let directory = entry.unwrap().path();
        if !directory.is_dir() {
            continue;
        }
        for entry in directory.read_dir().unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|extension| extension == "gz") {
                stored.insert(path);
            }
        }
        let manifest = directory.join("provenance.json");
        if manifest.is_file() {
            let value: Value = serde_json::from_slice(&read(manifest)).unwrap();
            verify(&value, &directory, &mut checked);
        }
    }
    assert!(
        !stored.is_empty(),
        "compressed fixture coverage must not be vacuous"
    );
    assert_eq!(
        stored, checked,
        "every gzip reference needs a provenance pin"
    );
}
