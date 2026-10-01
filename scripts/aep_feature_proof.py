#!/usr/bin/env python3
"""Local, non-CI workflow for honest After Effects feature-proof records.

The registry deliberately references, rather than copies, the authoritative
``aep_video_references.json`` source and publication metadata.  This module does
not compare Adobe output with a fresh conversion; reports always say that the
visual gate is unmeasured.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from fractions import Fraction
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import sys
import tempfile
from typing import Any, Iterable

SCHEMA_VERSION = 1
DEFAULT_REGISTRY = Path("crates/aftereffects_file/tests/fixtures/aep_feature_cases.json")
DEFAULT_MANIFEST = Path(
    "crates/aftereffects_file/tests/fixtures/aep_video_references.json"
)
CASE_ID_RE = re.compile(r"^aep-[a-z0-9]+(?:-[a-z0-9]+)*-c[1-9][0-9]*$")
SYMBOL_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
SECRET_KEY_RE = re.compile(
    r"(?:secret|authorization|signed.?url|upload.?url|access.?token|bearer)", re.I
)
SIGNED_URL_RE = re.compile(
    r"(?:X-Goog-(?:Algorithm|Credential|Signature)|[?&](?:token|signature)=)", re.I
)
PLACEHOLDER_RE = re.compile(r"\b(?:todo|tbd|placeholder|something|works?)\b", re.I)
MAX_EXPRESSION_SAMPLES_BYTES = 128 * 1024 * 1024


class ProofError(ValueError):
    """A safe, user-actionable workflow validation error."""


@dataclass(frozen=True)
class Paths:
    workspace: Path
    registry: Path
    manifest: Path


@dataclass(frozen=True)
class CaseState:
    level: str
    problems: tuple[str, ...]


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def _is_workspace(path: Path) -> bool:
    return (
        (path / "Cargo.toml").is_file()
        and (path / "scripts").is_dir()
        and (path / "crates/aftereffects_file").is_dir()
    )


def discover_workspace(explicit: str | None = None) -> Path:
    """Find this checkout without relying on a machine-specific absolute path."""
    if explicit:
        candidate = Path(explicit).expanduser().resolve()
        if not _is_workspace(candidate):
            raise ProofError(f"not a conversion workspace: {candidate}")
        return candidate

    starts = [Path.cwd().resolve(), Path(__file__).resolve().parent]
    for start in starts:
        for candidate in (start, *start.parents):
            if _is_workspace(candidate):
                return candidate
    raise ProofError("could not discover workspace; pass --workspace")


def resolve_repo_path(workspace: Path, value: str | Path, *, must_exist: bool) -> Path:
    raw = Path(value)
    candidate = raw if raw.is_absolute() else workspace / raw
    candidate = candidate.resolve(strict=False)
    try:
        candidate.relative_to(workspace)
    except ValueError as exc:
        raise ProofError(f"path escapes workspace: {value}") from exc
    if must_exist:
        if not candidate.is_file() or candidate.is_symlink():
            raise ProofError(f"expected a regular non-symlink file: {value}")
    return candidate


def relative_path(workspace: Path, path: Path) -> str:
    return path.resolve(strict=False).relative_to(workspace).as_posix()


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise ProofError(f"cannot read {path}: {exc.strerror or type(exc).__name__}") from exc
    except json.JSONDecodeError as exc:
        raise ProofError(f"invalid JSON in {path}: line {exc.lineno}, column {exc.colno}") from exc
    if not isinstance(value, dict):
        raise ProofError(f"expected a JSON object: {path}")
    return value


def atomic_write(path: Path, content: str, *, mode: int = 0o644) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.parent.is_symlink() or (path.exists() and path.is_symlink()):
        raise ProofError(f"refusing symlink write: {path}")
    descriptor, temporary_name = tempfile.mkstemp(
        dir=path.parent, prefix=f".{path.name}.", suffix=".tmp"
    )
    temporary = Path(temporary_name)
    try:
        os.fchmod(descriptor, mode)
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def write_json(path: Path, value: dict[str, Any], *, mode: int = 0o644) -> None:
    atomic_write(path, json.dumps(value, indent=2, ensure_ascii=False) + "\n", mode=mode)


def expected_case_id(source_path: str, composition_id: int) -> str:
    normalized = source_path.replace("\\", "/")
    marker = "/fixtures/"
    identity = normalized.split(marker, 1)[1] if marker in normalized else normalized
    if identity.lower().endswith(".aep"):
        identity = identity[:-4]
    slug = re.sub(r"[^a-z0-9]+", "-", identity.lower()).strip("-")
    if not slug:
        raise ProofError("source_path cannot produce a stable case ID")
    return f"aep-{slug}-c{composition_id}"


def _require_exact_keys(value: dict[str, Any], required: set[str], optional: set[str], where: str) -> None:
    missing = sorted(required - value.keys())
    unknown = sorted(value.keys() - required - optional)
    if missing:
        raise ProofError(f"{where} missing keys: {', '.join(missing)}")
    if unknown:
        raise ProofError(f"{where} has unknown keys: {', '.join(unknown)}")


def _text(value: Any, where: str, *, minimum: int = 1) -> str:
    if not isinstance(value, str) or len(value.strip()) < minimum:
        raise ProofError(f"{where} must be a non-empty concrete string")
    return value.strip()


def _expression_samples_descriptor(case: dict[str, Any]) -> dict[str, Any] | None:
    if "expression_samples" not in case:
        return None
    descriptor = case["expression_samples"]
    case_id = _text(case.get("case_id"), "case.case_id")
    if not isinstance(descriptor, dict):
        raise ProofError(f"{case_id}.expression_samples must be an object")
    _require_exact_keys(
        descriptor,
        {"path", "bytes", "sha256"},
        set(),
        f"{case_id}.expression_samples",
    )
    path = _text(descriptor["path"], f"{case_id}.expression_samples.path")
    canonical = PurePosixPath(path)
    if (
        canonical.is_absolute()
        or canonical.as_posix() != path
        or "\\" in path
        or any(part in {".", ".."} for part in canonical.parts)
        or canonical.suffix != ".json"
    ):
        raise ProofError(
            f"{case_id}.expression_samples.path must be a canonical workspace-relative JSON path"
        )
    size = descriptor["bytes"]
    if not isinstance(size, int) or isinstance(size, bool) or not 0 < size <= MAX_EXPRESSION_SAMPLES_BYTES:
        raise ProofError(
            f"{case_id}.expression_samples.bytes must be between 1 and "
            f"{MAX_EXPRESSION_SAMPLES_BYTES}"
        )
    digest = descriptor["sha256"]
    if not isinstance(digest, str) or not SHA256_RE.fullmatch(digest):
        raise ProofError(f"{case_id}.expression_samples.sha256 must be canonical SHA-256")
    return descriptor


def verify_expression_samples(
    workspace: Path,
    case: dict[str, Any],
    expected_source_sha256: str,
    expected_composition_id: int,
) -> tuple[Path, dict[str, Any]] | None:
    """Verify one optional sidecar's bounded file and exact source binding."""
    descriptor = _expression_samples_descriptor(case)
    if descriptor is None:
        return None
    workspace = workspace.resolve()
    unresolved = workspace / descriptor["path"]
    if unresolved.is_symlink():
        raise ProofError(f"expression samples must not be a symlink: {descriptor['path']}")
    path = resolve_repo_path(workspace, descriptor["path"], must_exist=True)
    try:
        with path.open("rb") as stream:
            content = stream.read(MAX_EXPRESSION_SAMPLES_BYTES + 1)
    except OSError as exc:
        raise ProofError(f"cannot read expression samples {descriptor['path']}: {exc}") from exc
    size = len(content)
    if size > MAX_EXPRESSION_SAMPLES_BYTES:
        raise ProofError("expression samples exceed the 128MiB input limit")
    digest = hashlib.sha256(content).hexdigest()
    if size != descriptor["bytes"] or digest != descriptor["sha256"]:
        raise ProofError("expression samples byte count or SHA-256 does not match the registry")
    try:
        payload = json.loads(content)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ProofError(f"invalid JSON in expression samples {descriptor['path']}") from exc
    if not isinstance(payload, dict):
        raise ProofError("expression samples must contain a JSON object")
    if payload.get("source_sha256") != expected_source_sha256:
        raise ProofError("expression samples source_sha256 does not match the registered AEP source")
    version = payload.get("version")
    scope = payload.get("capture_scope")
    if version == 1:
        if scope is not None:
            raise ProofError("legacy expression samples must not declare capture_scope")
    elif version == 2:
        if not isinstance(scope, dict):
            raise ProofError("version-2 expression samples must declare capture_scope")
        if scope == {"mode": "all_compositions"}:
            pass
        elif set(scope) == {"mode", "root_composition_id"} and scope.get("mode") == "selected_composition":
            if scope["root_composition_id"] != expected_composition_id:
                raise ProofError(
                    "expression samples selected capture root does not match the registered composition"
                )
        else:
            raise ProofError("expression samples contain an invalid capture_scope")
    else:
        raise ProofError("expression samples use an unsupported version")
    return path, {
        "path": descriptor["path"],
        "bytes": size,
        "sha256": descriptor["sha256"],
        "embedded_source_sha256": expected_source_sha256,
        "capture_scope": scope,
    }


def _scan_secrets(value: Any, where: str = "registry") -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            if SECRET_KEY_RE.search(str(key)):
                raise ProofError(f"{where} contains prohibited credential/URL key: {key}")
            _scan_secrets(child, f"{where}.{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            _scan_secrets(child, f"{where}[{index}]")
    elif isinstance(value, str):
        if SIGNED_URL_RE.search(value) or value.lower().startswith("bearer "):
            raise ProofError(f"{where} contains credential or signed-URL material")


def validate_manifest(manifest: dict[str, Any]) -> dict[tuple[str, int], tuple[dict[str, Any], dict[str, Any]]]:
    _require_exact_keys(
        manifest,
        {"schema_version", "scope", "reference_settings", "sources"},
        set(),
        "manifest",
    )
    if manifest["schema_version"] != 1:
        raise ProofError("unsupported AE reference manifest schema")
    settings = manifest["reference_settings"]
    if not isinstance(settings, dict) or settings.get("output_fps") != 30:
        raise ProofError("AE reference manifest must declare 30fps output")
    sources = manifest["sources"]
    if not isinstance(sources, list):
        raise ProofError("manifest.sources must be an array")

    targets: dict[tuple[str, int], tuple[dict[str, Any], dict[str, Any]]] = {}
    reference_paths: dict[str, tuple[str, int]] = {}
    for source_index, source in enumerate(sources):
        if not isinstance(source, dict):
            raise ProofError(f"manifest.sources[{source_index}] must be an object")
        source_path = _text(source.get("source_path"), f"manifest source {source_index}.source_path")
        source_sha = source.get("source_sha256")
        if not isinstance(source_sha, str) or not SHA256_RE.fullmatch(source_sha):
            raise ProofError(f"manifest source {source_path} has invalid SHA-256")
        compositions = source.get("compositions")
        if not isinstance(compositions, list):
            raise ProofError(f"manifest source {source_path} compositions must be an array")
        for composition in compositions:
            if not isinstance(composition, dict):
                raise ProofError(f"manifest source {source_path} contains a non-object composition")
            composition_id = composition.get("composition_id")
            if not isinstance(composition_id, int) or isinstance(composition_id, bool) or composition_id <= 0:
                raise ProofError(f"manifest source {source_path} has invalid composition_id")
            key = (source_path, composition_id)
            if key in targets:
                raise ProofError(f"duplicate manifest target: {source_path} composition {composition_id}")
            targets[key] = (source, composition)
            reference = composition.get("reference")
            if reference is not None:
                if not isinstance(reference, dict):
                    raise ProofError(f"manifest target {key} reference must be an object")
                path = reference.get("path")
                if not isinstance(path, str) or not path or Path(path).is_absolute() or ".." in Path(path).parts:
                    raise ProofError(f"manifest target {key} has invalid reference path")
                previous = reference_paths.get(path)
                if previous is not None and previous != key:
                    raise ProofError(f"reference path {path} is reused by {previous} and {key}")
                reference_paths[path] = key
    return targets


def _validate_rational(value: Any, where: str) -> None:
    if not isinstance(value, str):
        raise ProofError(f"{where} must be an exact rational string")
    try:
        rational = Fraction(value)
    except (ValueError, ZeroDivisionError) as exc:
        raise ProofError(f"{where} is not an exact rational") from exc
    if rational < 0:
        raise ProofError(f"{where} cannot be negative")


def _extract_test_body(text: str, symbol: str) -> str | None:
    match = re.search(
        rf"(?P<attrs>(?:\s*#\[[^\]\n]+\]\s*)+)fn\s+{re.escape(symbol)}\s*\(",
        text,
    )
    if match is None or "#[test]" not in match.group("attrs"):
        return None
    start = text.find("{", match.end())
    if start < 0:
        return None
    depth = 0
    for index in range(start, len(text)):
        character = text[index]
        if character == "{":
            depth += 1
        elif character == "}":
            depth -= 1
            if depth == 0:
                return text[start : index + 1]
    return None


def validate_case_schema(case: dict[str, Any]) -> None:
    _require_exact_keys(
        case,
        {
            "case_id",
            "direction",
            "feature",
            "requirement",
            "supported_by",
            "source_path",
            "composition_id",
            "critical_frames",
            "tests",
            "execution",
            "visual",
            "limitations",
        },
        {"authoring", "expression_samples", "pending_reason"},
        "case",
    )
    case_id = _text(case["case_id"], "case.case_id")
    if not CASE_ID_RE.fullmatch(case_id):
        raise ProofError(f"invalid stable case_id: {case_id}")
    source_path = _text(case["source_path"], f"{case_id}.source_path")
    _expression_samples_descriptor(case)
    composition_id = case["composition_id"]
    if not isinstance(composition_id, int) or isinstance(composition_id, bool) or composition_id <= 0:
        raise ProofError(f"{case_id}.composition_id must be a positive integer")
    expected = expected_case_id(source_path, composition_id)
    if case_id != expected:
        raise ProofError(f"case_id must be {expected} for this source/composition")
    if case["direction"] != "import":
        raise ProofError(
            f"{case_id}.direction must be import for this fixture-only registry; "
            "import proof never establishes export proof"
        )
    _text(case["feature"], f"{case_id}.feature", minimum=4)
    if case["requirement"] not in {"required", "supporting"}:
        raise ProofError(f"{case_id}.requirement must be required or supporting")
    supported_by = case["supported_by"]
    if not isinstance(supported_by, list) or any(
        not isinstance(item, str) or not CASE_ID_RE.fullmatch(item) for item in supported_by
    ):
        raise ProofError(f"{case_id}.supported_by must be an array of stable case IDs")
    if len(set(supported_by)) != len(supported_by) or case_id in supported_by:
        raise ProofError(f"{case_id}.supported_by contains a duplicate or self-reference")
    if case["requirement"] == "required" and supported_by:
        raise ProofError(f"required case {case_id} cannot use supported_by to evade coverage")
    if case["requirement"] == "supporting" and not supported_by:
        raise ProofError(f"supporting case {case_id} must name at least one required consumer")
    frames = case["critical_frames"]
    if not isinstance(frames, list):
        raise ProofError(f"{case_id}.critical_frames must be an array")
    for index, frame in enumerate(frames):
        _validate_rational(frame, f"{case_id}.critical_frames[{index}]")
    if len(set(frames)) != len(frames):
        raise ProofError(f"{case_id}.critical_frames contains duplicates")
    tests = case["tests"]
    if not isinstance(tests, list):
        raise ProofError(f"{case_id}.tests must be an array")
    for test_index, test in enumerate(tests):
        if not isinstance(test, dict):
            raise ProofError(f"{case_id}.tests[{test_index}] must be an object")
        _require_exact_keys(test, {"path", "symbol", "assertions"}, set(), f"{case_id}.tests[{test_index}]")
        _text(test["path"], f"{case_id}.tests[{test_index}].path")
        symbol = _text(test["symbol"], f"{case_id}.tests[{test_index}].symbol")
        if not SYMBOL_RE.fullmatch(symbol):
            raise ProofError(f"{case_id} has invalid Rust test symbol: {symbol}")
        assertions = test["assertions"]
        if not isinstance(assertions, list):
            raise ProofError(f"{case_id} test assertions must be an array")
        for assertion_index, assertion in enumerate(assertions):
            if not isinstance(assertion, dict):
                raise ProofError(f"{case_id} assertion {assertion_index} must be an object")
            _require_exact_keys(
                assertion,
                {"description", "source_anchor"},
                set(),
                f"{case_id} assertion {assertion_index}",
            )
            description = _text(
                assertion["description"],
                f"{case_id} assertion {assertion_index}.description",
                minimum=12,
            )
            if PLACEHOLDER_RE.search(description):
                raise ProofError(f"{case_id} assertion {assertion_index} is a placeholder")
            _text(assertion["source_anchor"], f"{case_id} assertion {assertion_index}.source_anchor", minimum=3)
    if case["execution"] != {"status": "UNRUN"}:
        raise ProofError(f"{case_id}.execution must remain {{\"status\": \"UNRUN\"}} until an external runner records evidence")
    if case["visual"] != {"status": "unmeasured"}:
        raise ProofError(f"{case_id}.visual must remain {{\"status\": \"unmeasured\"}}; comparison is not implemented")
    limitations = case["limitations"]
    if not isinstance(limitations, list):
        raise ProofError(f"{case_id}.limitations must be an array")
    for index, limitation in enumerate(limitations):
        _text(limitation, f"{case_id}.limitations[{index}]", minimum=6)
    if "pending_reason" in case:
        _text(case["pending_reason"], f"{case_id}.pending_reason", minimum=6)
    if "authoring" in case:
        authoring = case["authoring"]
        if not isinstance(authoring, dict):
            raise ProofError(f"{case_id}.authoring must be an object")
        _require_exact_keys(
            authoring,
            {
                "feature_jsx_path",
                "feature_jsx_sha256",
                "wrapper_jsx_path",
                "wrapper_jsx_sha256",
                "readback_path",
                "readback_sha256",
                "receipt_path",
                "receipt_sha256",
                "adobe_version",
                "recorded_at",
            },
            set(),
            f"{case_id}.authoring",
        )
        for key in (
            "feature_jsx_sha256",
            "wrapper_jsx_sha256",
            "readback_sha256",
            "receipt_sha256",
        ):
            if not isinstance(authoring[key], str) or not SHA256_RE.fullmatch(authoring[key]):
                raise ProofError(f"{case_id}.authoring.{key} must be SHA-256")
        for key in (
            "feature_jsx_path",
            "wrapper_jsx_path",
            "readback_path",
            "receipt_path",
            "adobe_version",
            "recorded_at",
        ):
            _text(authoring[key], f"{case_id}.authoring.{key}")
    _scan_secrets(case, case_id)


def validate_registry(registry: dict[str, Any]) -> None:
    _require_exact_keys(registry, {"schema_version", "scope", "cases"}, set(), "registry")
    if registry["schema_version"] != SCHEMA_VERSION:
        raise ProofError(f"unsupported feature registry schema: {registry['schema_version']}")
    _text(registry["scope"], "registry.scope", minimum=12)
    cases = registry["cases"]
    if not isinstance(cases, list):
        raise ProofError("registry.cases must be an array")
    ids: set[str] = set()
    targets: set[tuple[str, int]] = set()
    for case in cases:
        if not isinstance(case, dict):
            raise ProofError("registry case must be an object")
        validate_case_schema(case)
        case_id = case["case_id"]
        target = (case["source_path"], case["composition_id"])
        if case_id in ids:
            raise ProofError(f"duplicate case_id: {case_id}")
        if target in targets:
            raise ProofError(f"duplicate registry target: {target[0]} composition {target[1]}")
        ids.add(case_id)
        targets.add(target)
    cases_by_id = {case["case_id"]: case for case in cases}
    for case in cases:
        for consumer_id in case["supported_by"]:
            consumer = cases_by_id.get(consumer_id)
            if consumer is None:
                raise ProofError(f"{case['case_id']}.supported_by has dangling ID: {consumer_id}")
            if consumer["requirement"] != "required":
                raise ProofError(
                    f"{case['case_id']}.supported_by consumer must be required: {consumer_id}"
                )
            if consumer["direction"] != case["direction"]:
                raise ProofError(f"support relation crosses directions: {case['case_id']} -> {consumer_id}")
    _scan_secrets(registry)


def _reference_problems(composition: dict[str, Any]) -> list[str]:
    problems: list[str] = []
    if composition.get("status") != "verified":
        problems.append("native reference publication is not verified")
        return problems
    reference = composition.get("reference")
    verification = composition.get("verification")
    if not isinstance(reference, dict) or not isinstance(verification, dict):
        problems.append("verified target lacks reference/verification objects")
        return problems
    path = reference.get("path")
    if not isinstance(path, str) or not path or Path(path).is_absolute() or ".." in Path(path).parts:
        problems.append("reference local path is missing or invalid")
    if reference.get("fps") != 30:
        problems.append("reference is not 30fps")
    expected_frames = composition.get("expected_frame_count")
    if reference.get("frame_count") != expected_frames:
        problems.append("reference frame count does not match expected 30fps range")
    for key in ("native_render", "full_decode"):
        if verification.get(key) is not True:
            problems.append(f"reference verification {key} is not true")
    if verification.get("fx_render_comparison") != "not_run":
        problems.append("manifest unexpectedly claims an FX render comparison")
    return problems


def evaluate_case(
    case: dict[str, Any],
    targets: dict[tuple[str, int], tuple[dict[str, Any], dict[str, Any]]],
    workspace: Path,
    *,
    verify_files: bool,
) -> CaseState:
    problems: list[str] = []
    target = targets.get((case["source_path"], case["composition_id"]))
    if target is None:
        problems.append("target is absent from aep_video_references.json")
    else:
        source, composition = target
        problems.extend(_reference_problems(composition))
        if verify_files:
            source_path = resolve_repo_path(workspace, source["source_path"], must_exist=True)
            if sha256_file(source_path) != source["source_sha256"]:
                problems.append("source AEP SHA-256 does not match manifest")
            if source_path.stat().st_size != source.get("source_bytes"):
                problems.append("source AEP byte count does not match manifest")
            try:
                reference_path = resolve_repo_path(
                    workspace, composition["reference"]["path"], must_exist=True
                )
                if not reference_path.is_file() or reference_path.is_symlink():
                    problems.append("reference path is not a regular non-symlink file")
                verify_expression_samples(
                    workspace,
                    case,
                    source["source_sha256"],
                    case["composition_id"],
                )
            except (KeyError, ProofError) as exc:
                problems.append(str(exc))

    if not case["critical_frames"]:
        problems.append("no feature-critical frames are recorded")
    if not case["tests"]:
        problems.append("no executable test linkage is recorded")
    for test in case["tests"]:
        try:
            path = resolve_repo_path(workspace, test["path"], must_exist=True)
        except ProofError as exc:
            problems.append(str(exc))
            continue
        if path.suffix != ".rs":
            problems.append(f"test link is not Rust source: {test['path']}")
            continue
        text = path.read_text(encoding="utf-8")
        body = _extract_test_body(text, test["symbol"])
        if body is None:
            problems.append(f"test symbol is missing or lacks #[test]: {test['path']}::{test['symbol']}")
            continue
        if not test["assertions"]:
            problems.append(f"test has no mapped assertions: {test['path']}::{test['symbol']}")
        for assertion in test["assertions"]:
            if assertion["source_anchor"] not in body:
                problems.append(
                    f"assertion anchor is absent from {test['path']}::{test['symbol']}: "
                    f"{assertion['source_anchor']!r}"
                )
    if "authoring" in case and verify_files:
        for path_key, sha_key in (
            ("feature_jsx_path", "feature_jsx_sha256"),
            ("wrapper_jsx_path", "wrapper_jsx_sha256"),
            ("readback_path", "readback_sha256"),
            ("receipt_path", "receipt_sha256"),
        ):
            try:
                path = resolve_repo_path(workspace, case["authoring"][path_key], must_exist=True)
            except ProofError as exc:
                problems.append(str(exc))
                continue
            if sha256_file(path) != case["authoring"][sha_key]:
                problems.append(f"authoring provenance hash mismatch: {case['authoring'][path_key]}")
    if case.get("pending_reason"):
        problems.append(f"declared pending: {case['pending_reason']}")

    if problems:
        return CaseState("pending", tuple(problems))
    return CaseState("links_checked", ())


def _paths(args: argparse.Namespace) -> Paths:
    workspace = discover_workspace(args.workspace)
    registry = resolve_repo_path(workspace, args.registry, must_exist=False)
    manifest = resolve_repo_path(workspace, args.manifest, must_exist=True)
    return Paths(workspace, registry, manifest)


def _load_all(paths: Paths) -> tuple[dict[str, Any], dict[str, Any], dict[tuple[str, int], tuple[dict[str, Any], dict[str, Any]]]]:
    registry = load_json(paths.registry)
    validate_registry(registry)
    manifest = load_json(paths.manifest)
    targets = validate_manifest(manifest)
    return registry, manifest, targets


def command_init(args: argparse.Namespace) -> int:
    paths = _paths(args)
    if paths.registry.exists():
        raise ProofError(f"registry already exists; refusing replacement: {paths.registry}")
    registry = {
        "schema_version": SCHEMA_VERSION,
        "scope": (
            "Local AE import feature cases. Publication is native-source evidence; "
            "actual-vs-expected conversion comparison remains unimplemented."
        ),
        "cases": [],
    }
    write_json(paths.registry, registry)
    print(relative_path(paths.workspace, paths.registry))
    return 0


def command_case_id(args: argparse.Namespace) -> int:
    print(expected_case_id(args.source_path, args.composition_id))
    return 0


def command_register(args: argparse.Namespace) -> int:
    paths = _paths(args)
    registry = load_json(paths.registry)
    validate_registry(registry)
    case_file = Path(args.case_file).expanduser().resolve()
    case = load_json(case_file)
    validate_case_schema(case)

    existing_index = next(
        (index for index, item in enumerate(registry["cases"]) if item["case_id"] == case["case_id"]),
        None,
    )
    duplicate_target = next(
        (
            item["case_id"]
            for item in registry["cases"]
            if (item["source_path"], item["composition_id"])
            == (case["source_path"], case["composition_id"])
            and item["case_id"] != case["case_id"]
        ),
        None,
    )
    if duplicate_target:
        raise ProofError(f"target is already registered as {duplicate_target}")
    if existing_index is not None and not args.replace:
        raise ProofError(f"case already exists; pass --replace explicitly: {case['case_id']}")
    if existing_index is not None:
        old = registry["cases"][existing_index]
        if (old["source_path"], old["composition_id"]) != (
            case["source_path"],
            case["composition_id"],
        ):
            raise ProofError("--replace cannot change a case identity")
        registry["cases"][existing_index] = case
    else:
        registry["cases"].append(case)
    registry["cases"].sort(key=lambda item: item["case_id"])
    validate_registry(registry)
    write_json(paths.registry, registry)
    print(case["case_id"])
    return 0


def _verify_references(
    cases: Iterable[dict[str, Any]],
    targets: dict[tuple[str, int], tuple[dict[str, Any], dict[str, Any]]],
    workspace: Path,
) -> list[tuple[str, bool]]:
    errors: list[tuple[str, bool]] = []
    for case in cases:
        target = targets.get((case["source_path"], case["composition_id"]))
        if target is None:
            continue
        reference = target[1].get("reference")
        if not isinstance(reference, dict):
            continue
        try:
            path = resolve_repo_path(workspace, reference["path"], must_exist=True)
            if not path.is_file() or path.is_symlink():
                raise ProofError("reference path is not a regular non-symlink file")
        except (KeyError, ProofError) as exc:
            errors.append((f"{case['case_id']}: local reference verification failed ({exc})",
                           case["requirement"] == "required"))
    return errors


def command_check(args: argparse.Namespace) -> int:
    paths = _paths(args)
    registry, _manifest, targets = _load_all(paths)
    states: dict[str, CaseState] = {}
    failed = False
    for case in registry["cases"]:
        state = evaluate_case(case, targets, paths.workspace, verify_files=True)
        states[case["case_id"]] = state
        print(f"{case['case_id']}: {state.level}")
        for problem in state.problems:
            print(f"  - {problem}")
        if case["requirement"] == "required" and state.level != "links_checked":
            failed = True
    if args.verify_references:
        reference_errors = _verify_references(registry["cases"], targets, paths.workspace)
        for error, _required in reference_errors:
            print(f"reference: {error}")
        failed = failed or any(required for _error, required in reference_errors)
    if args.require == "full":
        print("full proof: blocked — AEP/FX render comparison is not implemented")
        failed = True
    required = sum(case["requirement"] == "required" for case in registry["cases"])
    ready = sum(
        case["requirement"] == "required" and states[case["case_id"]].level == "links_checked"
        for case in registry["cases"]
    )
    # The shared manifest also contains native targets owned by the separate
    # FX-export registry; only registered import cases belong to this check.
    if required == 0:
        failed = True
    print(f"required link-checked cases: {ready}/{required}; execution=UNRUN; visual=unmeasured")
    return 1 if failed else 0


def _markdown_escape(value: Any) -> str:
    return str(value).replace("|", "\\|").replace("\n", " ")


def build_report(
    registry: dict[str, Any],
    manifest: dict[str, Any],
    targets: dict[tuple[str, int], tuple[dict[str, Any], dict[str, Any]]],
    workspace: Path,
    registry_sha256: str,
    manifest_sha256: str,
) -> str:
    lines = [
        "# After Effects import feature proof report",
        "",
        "> **Visual comparison is not implemented.** A committed Adobe reference is source evidence,",
        "> not proof that a fresh import renders equivalently. Every case remains visual `unmeasured`",
        "> and test execution remains `UNRUN` in this local registry. `links_checked` means only",
        "> that the named Rust symbol and assertion anchors were found; it is not a verified test result.",
        "",
        f"- Registry schema: `{registry['schema_version']}`",
        f"- Registry SHA-256: `{registry_sha256}`",
        f"- AE reference manifest SHA-256: `{manifest_sha256}`",
        f"- Native output policy: `{manifest['reference_settings']['output_fps']}fps`, committed local references",
        f"- Registered import feature cases: `{len(registry['cases'])}`",
        f"- Meaningful executable test links authored, not run: `{sum(bool(case['tests']) for case in registry['cases'])}`",
        f"- Targets pending a meaningful executable test link: `{sum(not case['tests'] for case in registry['cases'])}`",
        "- Test execution: `UNRUN`; fresh-import visual comparison: `unmeasured`",
        "",
        "| Case | Requirement | Feature | Source / composition | Native reference | Structural state | Execution | Visual |",
        "|---|---|---|---|---|---|---|---|",
    ]
    evaluations: dict[str, CaseState] = {}
    for case in registry["cases"]:
        state = evaluate_case(case, targets, workspace, verify_files=False)
        evaluations[case["case_id"]] = state
        target = targets.get((case["source_path"], case["composition_id"]))
        if target is None:
            source_identity = f"`{case['source_path']}` / comp `{case['composition_id']}` (manifest target missing)"
            native_reference = "missing"
        else:
            source, composition = target
            source_identity = (
                f"`{case['source_path']}` SHA `{source['source_sha256']}` / "
                f"comp `{case['composition_id']}` `{composition.get('composition_name', '')}`"
            )
            reference = composition.get("reference", {})
            native_reference = (
                f"`{reference.get('path', 'missing')}` / "
                f"{reference.get('frame_count', '?')} frames"
            )
        lines.append(
            "| "
            + " | ".join(
                _markdown_escape(value)
                for value in (
                    case["case_id"],
                    case["requirement"],
                    case["feature"],
                    source_identity,
                    native_reference,
                    state.level,
                    case["execution"]["status"],
                    case["visual"]["status"],
                )
            )
            + " |"
        )

    for case in registry["cases"]:
        state = evaluations[case["case_id"]]
        lines.extend(["", f"## `{case['case_id']}` — {case['feature']}", ""])
        lines.append(
            f"Direction: **{case['direction']}**. Target: `{case['source_path']}` composition "
            f"`{case['composition_id']}`. Derived state: **{state.level}**."
        )
        if case["supported_by"]:
            lines.append(
                "Supporting evidence consumed by: "
                + ", ".join(f"`{consumer}`" for consumer in case["supported_by"])
            )
        lines.append("Critical frames: " + (", ".join(f"`{value}s`" for value in case["critical_frames"]) or "pending"))
        if case["tests"]:
            lines.append("\nExecutable test links (not run by this helper):")
            for test in case["tests"]:
                lines.append(f"- `{test['path']}::{test['symbol']}`")
                for assertion in test["assertions"]:
                    lines.append(
                        f"  - {assertion['description']} — source anchor `{assertion['source_anchor']}`"
                    )
        else:
            lines.append("\n- No executable test linkage registered.")
        if state.problems:
            lines.append("\nPending/blocking details:")
            lines.extend(f"- {problem}" for problem in state.problems)
        if case["limitations"]:
            lines.append("\nRecorded limitations:")
            lines.extend(f"- {item}" for item in case["limitations"])
        lines.append("\nComparison result: **unmeasured** (no comparator or score was run).")
    lines.append("")
    return "\n".join(lines)


def command_report(args: argparse.Namespace) -> int:
    paths = _paths(args)
    registry, manifest, targets = _load_all(paths)
    registry_sha = sha256_file(paths.registry)
    manifest_sha = sha256_file(paths.manifest)
    content = build_report(
        registry,
        manifest,
        targets,
        paths.workspace,
        registry_sha,
        manifest_sha,
    )
    if args.check_stale:
        report_path = resolve_repo_path(paths.workspace, args.check_stale, must_exist=True)
        report_text = report_path.read_text(encoding="utf-8")
        if report_text != content:
            print(
                "report is stale or modified: regenerate it from the current registry and manifest",
                file=sys.stderr,
            )
            return 1
        print("report content is current; this does not execute tests or measure visuals")
        return 0
    if args.output:
        output = resolve_repo_path(paths.workspace, args.output, must_exist=False)
        atomic_write(output, content)
        print(relative_path(paths.workspace, output))
    else:
        sys.stdout.write(content)
    return 0


def _find_case(registry: dict[str, Any], case_id: str) -> dict[str, Any]:
    matches = [case for case in registry["cases"] if case["case_id"] == case_id]
    if len(matches) != 1:
        raise ProofError(f"case_id is not uniquely registered: {case_id}")
    return matches[0]


def command_reference(args: argparse.Namespace) -> int:
    from aep_feature_proof_publish import (
        author_source,
        inspect_render,
        publish_reference,
        render_reference,
    )

    paths = _paths(args)
    registry, _manifest, targets = _load_all(paths)
    case = _find_case(registry, args.case_id)
    if args.reference_command == "author-template":
        author_source(paths, case, args)
        return 0
    if args.reference_command == "record-authoring":
        authoring = inspect_render(paths, case, args, authoring=True)
        case["authoring"] = authoring
        validate_registry(registry)
        write_json(paths.registry, registry)
        print(case["case_id"])
        return 0
    target = targets.get((case["source_path"], case["composition_id"]))
    if target is None:
        raise ProofError("case target is absent from the AE reference manifest")
    if args.reference_command in {"render", "publish"} and args.confirm_case_id != case["case_id"]:
        raise ProofError(
            f"--confirm-case-id must exactly match {case['case_id']} for this destructive operation"
        )
    if args.reference_command == "render":
        render_reference(paths, case, target[0], target[1], args)
    elif args.reference_command == "inspect":
        inspect_render(paths, case, args)
    elif args.reference_command == "publish":
        publish_reference(paths, case, target[0], target[1])
    else:
        raise ProofError(f"unknown reference command: {args.reference_command}")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Local AE feature-proof registry and bounded native-reference helper"
    )
    parser.add_argument("--workspace", help="conversion workspace root (auto-detected by default)")
    parser.add_argument("--registry", default=str(DEFAULT_REGISTRY))
    parser.add_argument("--manifest", default=str(DEFAULT_MANIFEST))
    subparsers = parser.add_subparsers(dest="command", required=True)

    initialize = subparsers.add_parser("init", help="create an empty separate case registry")
    initialize.set_defaults(handler=command_init)

    case_id = subparsers.add_parser("case-id", help="derive the stable ID for a target")
    case_id.add_argument("--source-path", required=True)
    case_id.add_argument("--composition-id", required=True, type=int)
    case_id.set_defaults(handler=command_case_id)

    register = subparsers.add_parser("register", help="register one reviewed case JSON")
    register.add_argument("--case-file", required=True)
    register.add_argument("--replace", action="store_true")
    register.set_defaults(handler=command_register)

    check = subparsers.add_parser("check", help="validate case links and proof inputs without running tests")
    check.add_argument("--require", choices=("structural", "full"), default="structural")
    check.add_argument(
        "--verify-references",
        action="store_true",
        help="verify every registered committed local reference exists",
    )
    check.set_defaults(handler=command_check)

    report = subparsers.add_parser("report", help="write or stale-check an honest Markdown report")
    report_group = report.add_mutually_exclusive_group()
    report_group.add_argument("--output", help="repository-local output path; stdout by default")
    report_group.add_argument("--check-stale", help="existing report whose embedded input hashes must still match")
    report.set_defaults(handler=command_report)

    reference = subparsers.add_parser("reference", help="bounded native author/render/local-record operations")
    reference_subparsers = reference.add_subparsers(dest="reference_command", required=True)

    author = reference_subparsers.add_parser(
        "author-template",
        help="emit, but never execute, an ownership-guarded wrapper around feature-specific JSX",
    )
    author.add_argument("--case-id", required=True)
    author.add_argument("--feature-jsx", required=True)
    author.add_argument("--wrapper-jsx", required=True)
    author.add_argument("--readback", required=True, help="new repository-local JSON output for JSX")
    author.add_argument("--receipt", required=True, help="new repository-local ownership receipt for JSX")
    author.set_defaults(handler=command_reference)

    record_authoring = reference_subparsers.add_parser(
        "record-authoring",
        help="hash completed JSX/readback provenance without executing Adobe",
    )
    record_authoring.add_argument("--case-id", required=True)
    record_authoring.add_argument("--feature-jsx", required=True)
    record_authoring.add_argument("--wrapper-jsx", required=True)
    record_authoring.add_argument("--readback", required=True)
    record_authoring.add_argument("--receipt", required=True)
    record_authoring.set_defaults(handler=command_reference)

    render = reference_subparsers.add_parser("render", help="render exactly one unpublished target at native 30fps")
    render.add_argument("--case-id", required=True)
    render.add_argument("--aerender", help="exact aerender executable path")
    render.add_argument("--timeout", type=int, default=600)
    render.add_argument(
        "--confirm-case-id",
        required=True,
        help="repeat the exact case ID to acknowledge Adobe execution",
    )
    render.set_defaults(handler=command_reference)

    inspect = reference_subparsers.add_parser("inspect", help="record human inspection provenance for one local render")
    inspect.add_argument("--case-id", required=True)
    inspect.add_argument("--note", required=True)
    inspect.set_defaults(handler=command_reference)

    publish = reference_subparsers.add_parser("publish", help="copy one inspected native render into tests/references/aep")
    publish.add_argument("--case-id", required=True)
    publish.add_argument(
        "--confirm-case-id",
        required=True,
        help="repeat the exact case ID to confirm writing a new committed reference",
    )
    publish.set_defaults(handler=command_reference)
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return args.handler(args)
    except ProofError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    except KeyboardInterrupt:
        print("error: interrupted; local files and restart journal were preserved", file=sys.stderr)
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
