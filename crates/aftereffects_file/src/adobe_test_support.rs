//! Per-native-target assertion isolation for the unified local Adobe runner.
//! A successful record means the complete assertion callback passed, not that
//! Adobe rendering or RGB/alpha fidelity has been established.
use std::{
    any::Any,
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod fill_isolated;
mod sampled_position_expression;

#[derive(Deserialize)]
struct Registry {
    cases: Vec<Target>,
}

#[derive(Deserialize)]
struct Target {
    case_id: String,
    source_path: String,
    composition_id: u32,
    tests: Vec<TestLink>,
}

#[derive(Deserialize)]
struct TestLink {
    path: String,
    symbol: String,
}

#[derive(Deserialize)]
struct Manifest {
    sources: Vec<Source>,
}

#[derive(Deserialize)]
struct Source {
    source_path: String,
    source_sha256: String,
}

struct Catalog {
    targets: HashMap<(String, u32), Target>,
    sources: HashMap<String, String>,
}

fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let registry: Registry =
            serde_json::from_str(include_str!("../tests/fixtures/aep_feature_cases.json"))
                .expect("valid Adobe case registry");
        let manifest: Manifest =
            serde_json::from_str(include_str!("../tests/fixtures/aep_video_references.json"))
                .expect("valid Adobe source manifest");
        Catalog {
            targets: registry
                .cases
                .into_iter()
                .map(|target| ((target.source_path.clone(), target.composition_id), target))
                .collect(),
            sources: manifest
                .sources
                .into_iter()
                .map(|source| (source.source_path, source.source_sha256))
                .collect(),
        }
    })
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source_hash(path: &str) -> Result<String, String> {
    static HASHES: OnceLock<Mutex<HashMap<String, Result<String, String>>>> = OnceLock::new();
    let mut hashes = HASHES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    hashes
        .entry(path.to_owned())
        .or_insert_with(|| {
            fs::read(workspace().join(path))
                .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
                .map_err(|error| format!("cannot hash native source {path}: {error}"))
        })
        .clone()
}

fn linked_symbol(link: &TestLink) -> String {
    let module = link
        .path
        .strip_prefix("crates/aftereffects_file/src/")
        .expect("Adobe test link belongs to this crate")
        .strip_suffix(".rs")
        .expect("Adobe test link names Rust source")
        .trim_end_matches("/mod")
        .replace('/', "::");
    format!("{module}::{}", link.symbol)
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else {
        "assertion panicked with a non-string payload".to_owned()
    }
}

fn assertions_error(assertions: impl FnOnce()) -> Option<String> {
    catch_unwind(AssertUnwindSafe(assertions))
        .err()
        .map(|payload| panic_message(payload.as_ref()))
}

fn test_binary_identity() -> &'static serde_json::Value {
    static IDENTITY: OnceLock<serde_json::Value> = OnceLock::new();
    IDENTITY.get_or_init(|| {
        match std::env::current_exe().and_then(|path| fs::read(&path).map(|bytes| (path, bytes))) {
            Ok((path, bytes)) => serde_json::json!({
                "path": path,
                "sha256": format!("{:x}", Sha256::digest(&bytes)),
                "bytes": bytes.len(),
            }),
            Err(error) => serde_json::json!({"error": error.to_string()}),
        }
    })
}

#[derive(Serialize)]
struct Record<'a> {
    schema_version: u32,
    case_id: &'a str,
    source_path: &'a str,
    source_sha256: &'a str,
    composition_id: u32,
    direction: &'static str,
    test_symbol: &'a str,
    test_binary: &'static serde_json::Value,
    status: &'static str,
    attempted: bool,
    assertions_executed: bool,
    error: Option<&'a str>,
}

pub(crate) fn artifact_directory() -> PathBuf {
    let directory = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/adobe-test/fx_exports"
    ));
    fs::create_dir_all(&directory).expect("Adobe artifact directory");
    directory
        .canonicalize()
        .expect("resolve Adobe artifact directory")
}

fn journal(name: &str) -> PathBuf {
    let directory = artifact_directory().parent().unwrap().to_path_buf();
    fs::create_dir_all(&directory).expect("Adobe journal directory");
    directory.join(name)
}

fn append_record(path: &Path, record: &impl Serialize) -> Result<(), String> {
    // The parent runner clears the fixed scratch journal. Never append to a
    // native fixture, existing symlink, or a path beneath fixture storage.
    if path
        .extension()
        .is_none_or(|extension| extension != "jsonl")
    {
        return Err("Adobe journal must name a scratch .jsonl file".to_owned());
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|error| format!("cannot resolve Adobe journal parent: {error}"))?;
    let fixtures = workspace()
        .join("crates/aftereffects_file/tests/fixtures")
        .canonicalize()
        .map_err(|error| format!("cannot resolve fixture storage: {error}"))?;
    if parent.starts_with(fixtures)
        || fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err("Adobe journals must not overwrite fixtures or symlink targets".to_owned());
    }
    static WRITE_LOCK: Mutex<()> = Mutex::new(());
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let mut bytes = serde_json::to_vec(record).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|error| format!("cannot append Adobe target result: {error}"))
}

/// Execute every target even when an earlier target has a genuine assertion
/// failure. Call `finish` after the last target to fail the enclosing Rust test.
#[must_use = "run the target callbacks and call finish to propagate failures"]
pub(crate) struct CaseBatch {
    test_symbol: String,
    seen: HashSet<String>,
    failures: usize,
}

impl CaseBatch {
    pub(crate) fn new() -> Self {
        Self {
            test_symbol: std::thread::current()
                .name()
                .unwrap_or("unnamed Adobe test thread")
                .to_owned(),
            seen: HashSet::new(),
            failures: 0,
        }
    }

    pub(crate) fn run(
        &mut self,
        source_path: &str,
        composition_id: u32,
        assertions: impl FnOnce(),
    ) {
        let catalog = catalog();
        let Some(target) = catalog
            .targets
            .get(&(source_path.to_owned(), composition_id))
        else {
            self.failures += 1;
            eprintln!("Unregistered Adobe target: {source_path} composition {composition_id}");
            return;
        };
        let selection_path = journal("selected-case-ids.json");
        if selection_path.exists() {
            let selected: Vec<String> = serde_json::from_slice(
                &fs::read(selection_path).expect("read Adobe case selection"),
            )
            .expect("parse Adobe case selection");
            if !selected.is_empty() && !selected.contains(&target.case_id) {
                return;
            }
        }
        let expected_hash = catalog
            .sources
            .get(source_path)
            .map(String::as_str)
            .unwrap_or("");
        let mut assertions_executed = false;
        let error = assertions_error(|| {
            assert!(
                self.seen.insert(target.case_id.clone()),
                "duplicate target callback {}",
                target.case_id
            );
            assert!(
                target
                    .tests
                    .iter()
                    .any(|link| linked_symbol(link) == self.test_symbol),
                "{} is not linked to executing test {}",
                target.case_id,
                self.test_symbol
            );
            assert_eq!(
                source_hash(source_path).expect("read pinned Adobe source"),
                expected_hash,
                "native source hash differs from the pinned manifest"
            );
            assertions_executed = true;
            assertions();
        });
        if error.is_some() {
            self.failures += 1;
        }
        {
            let path = journal("adobe-test-records.jsonl");
            let record = Record {
                schema_version: 1,
                case_id: &target.case_id,
                source_path,
                source_sha256: expected_hash,
                composition_id,
                direction: "import",
                test_symbol: &self.test_symbol,
                test_binary: test_binary_identity(),
                status: if error.is_some() {
                    "failure"
                } else {
                    "success"
                },
                attempted: true,
                assertions_executed,
                error: error.as_deref(),
            };
            if let Err(error) = append_record(Path::new(&path), &record) {
                self.failures += 1;
                eprintln!("{error}");
            }
        }
    }

    pub(crate) fn finish(self) {
        assert_eq!(
            self.failures, 0,
            "Adobe target assertions failed; every callback in {} was attempted",
            self.test_symbol
        );
    }
}

fn export_artifacts(name: &str) -> serde_json::Value {
    let directory = artifact_directory();
    let mut artifacts = serde_json::Map::new();
    {
        for (key, extension) in [
            ("fx_json", "fx.json"),
            ("expected_json", "expected.json"),
            ("aep", "aep"),
            ("tsrct", "tsrct"),
        ] {
            let path = Path::new(&directory).join(name).with_extension(extension);
            if !fs::symlink_metadata(&path)
                .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            {
                continue;
            }
            if let Ok(bytes) = fs::read(&path) {
                artifacts.insert(
                    key.to_owned(),
                    serde_json::json!({
                        "path": path,
                        "sha256": format!("{:x}", Sha256::digest(&bytes)),
                        "bytes": bytes.len(),
                    }),
                );
            }
        }
    }
    serde_json::Value::Object(artifacts)
}

/// Explicit FX exports already have one Rust test per case. Record the entire
/// callback, including artifact construction, and preserve its failing exit.
pub(crate) fn export_case(name: &str, assertions: impl FnOnce()) {
    let error = assertions_error(assertions);
    let thread = std::thread::current();
    {
        let path = journal("adobe-export-records.jsonl");
        let record = serde_json::json!({
            "schema_version": 1,
            "case_id": format!("fx-export-{name}"),
            "case_name": name,
            "direction": "export",
            "artifacts": export_artifacts(name),
            "test_binary": test_binary_identity(),
            "test_symbol": thread.name().unwrap_or("unnamed Adobe export test"),
            "status": if error.is_some() { "failure" } else { "success" },
            "attempted": true,
            "assertions_executed": true,
            "error": error,
        });
        append_record(Path::new(&path), &record)
            .expect("persist explicit FX export assertion result");
    }
    assert!(
        error.is_none(),
        "Adobe export case {name} failed: {error:?}"
    );
}

#[test]
fn failed_assertion_does_not_prevent_later_target_callbacks() {
    let mut visited = Vec::new();
    let mut results = Vec::new();
    for index in 0..3 {
        results.push(assertions_error(|| {
            visited.push(index);
            assert_ne!(index, 1, "independent failing target");
        }));
    }
    assert_eq!(visited, [0, 1, 2]);
    assert!(results[0].is_none());
    assert!(
        results[1]
            .as_ref()
            .unwrap()
            .contains("independent failing target")
    );
    assert!(results[2].is_none());
}

#[test]
fn batch_failure_is_not_silently_converted_into_a_passing_test() {
    let mut batch = CaseBatch::new();
    batch.failures = 1;
    assert!(assertions_error(|| batch.finish()).is_some());
}

#[test]
fn registry_test_paths_bind_the_exact_rust_module_and_symbol() {
    let link = TestLink {
        path: "crates/aftereffects_file/src/structure_document/tests/native_shape_cases.rs"
            .to_owned(),
        symbol: "native_rectangle_controls_import_static_values_and_key_tracks".to_owned(),
    };
    assert_eq!(
        linked_symbol(&link),
        "structure_document::tests::native_shape_cases::native_rectangle_controls_import_static_values_and_key_tracks"
    );
}
