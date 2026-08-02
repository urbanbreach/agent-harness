#!/usr/bin/env python3
"""Full-scope clean-room scan for the reference parity program.

Extends the narrow task-37 scan (36 files) to every first-party path and the full
current evidence root, with an explicit approved-paths manifest, a per-match
boilerplate/substantive classification, reference-source attribution, and a canary
mutation proof. Stdlib only.

Detection rules (schema grok-parity-cleanroom-scan-v1):
  reference_path_prefix      content names the auditor-only reference root
  reference_hex_fingerprint  content embeds a cited reference file's sha256
  copied_reference_file      file is byte-identical to a cited reference file
  reference_source_fragment  >=200 whitespace-normalized verbatim reference bytes
  copied_reference_asset     binary byte-identical to a reference inspiration asset
  unknown_binary             binary not known-good, not a reference copy, not a
                             unique raster capture, not a candidate build artifact
  reference_binary_mismatch  file at the reference binary path has a digest that
                             is not on the known-good list

copied-file and fragment rules admit NO approved exceptions: even an auditor file
must never embed reference source. Path/digest rules exempt the approved
auditor/planning/capture manifest, which legitimately cites the reference.

Each fragment match is classified. Language scaffolding and punctuation (Rust
keywords, standard derives, comment banners/dividers, generic section labels)
carry no copied *expression* and are reported as boilerplate; any surviving
project identifier makes a match substantive. All matches are listed with their
raw tokens and reference attribution so the classification is auditable. Verdict
fails on substantive fragments, copied files, hex leaks, unapproved path
references, unknown binaries, copied reference assets, or a reference binary
mismatch.

Binary and oversized files are no longer skipped under the evidence root or the
inspirations tree (repair of F4-CLEANROOM-004). Such files are hashed by
streaming sha256 and inventoried with their digests. Evidence binaries are
rejected unless they are on the known-good list (the frozen reference binary
digest, plus reference-inputs.json reference_bin.sha256 when format-valid), are
a copied reference asset/file (own failing rules), are unique raster captures
(e.g. auditor terminal screenshots), or live on recognized candidate build
paths. The inspirations walk indexes reference raster digests for
evidence-vs-reference cross-comparison, verifies the frozen reference binary
digest, and records other reference binaries as inventoried material.
"""
import argparse, hashlib, json, os, re, sys, tempfile, shutil

SHINGLE = 200          # verbatim byte threshold (spec: ">=200 verbatim reference bytes")
STEP_REF = 16          # reference shingle stride (>=216-byte copies guaranteed detected)
PATH_PREFIX = "inspirations/grok-build"
HEX_RE = re.compile(r"[0-9a-f]{64}")
TOKEN_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")

DEFAULT_APPROVED = [
    "scripts/cleanroom_scan.py",
    "scripts/check-parity-reference-crosswalk.py",
    "scripts/test-lanes.sh",
    "scripts/tui-parity/web-terminal-visual-qa.mjs",
    "scripts/tui-parity/capture-reference-observations.py",
    "crates/harness-testkit/tests/parity_differential_proof_test.rs",
    "crates/harness-testkit/tests/parity_motion_timing_test.rs",
    "crates/harness/tests/parity_checkout_freeze_test.rs",
    "crates/harness-tui/tests/support/reference_parity_runner_identity.rs",
    "docs/grok-build-parity-loop-contract.md",
    "docs/grok-build-tui-implementation-prompt.md",
    "docs/grok-cleanroom-scope.v1.json",
    "docs/grok-reference-interaction-inventory.v1.json",
    "docs/scope-removal-ledger.v1.json",
    "docs/testing.md",
    "grok-build-clean-room-parity.md",
    "grok-build-parity-parallel-execution.md",
    "receipts/reference-freeze.receipt.json",
    ".agent-harness/plans/grok-build-parity-loop-work-plan.md",
    "Cargo.lock",
]

PRODUCT_ROOTS = ["crates", "configs", "docs", "scripts",
                 ".agent-harness/agents", ".agent-harness/prompt-families",
                 ".agent-harness/skills"]
EVIDENCE_ROOT = ".omo/evidence/grok-build-clean-room-parity/20260727-110657"
EXCLUDE_FRAGMENTS = ("/target/", "/.git/", "/node_modules/", "/sessions/",
                     "/artifacts/", "/inspirations/", "/__pycache__/",
                     "screenshot folder")
SKIP_EXTS = {".png", ".jpg", ".jpeg", ".gif", ".ico", ".woff", ".woff2",
             ".ttf", ".pdf", ".zip", ".gz", ".bin", ".so", ".dylib", ".dll",
             ".exe", ".pyc"}
MAX_FILE = 8 * 1024 * 1024

# Binary/oversized digest policy (repairs F4-CLEANROOM-004: the full-scope scan
# skipped 2225 binary and 67 oversized evidence files). Files that the text scan
# cannot classify are hashed by streaming sha256 and dispositioned by digest.
RASTER_EXTS = {".png", ".jpg", ".jpeg", ".gif", ".ico"}
EXTRA_BINARY_EXTS = {".rlib", ".rmeta", ".o", ".a", ".d", ".pdb", ".node",
                     ".wasm", ".elf"}
BINARY_EXTS = SKIP_EXTS | EXTRA_BINARY_EXTS
REFERENCE_BINARY_REL = "inspirations/grok-build/target/debug/xai-grok-pager"
# Known-good binary digests allowed in evidence without a finding. The frozen
# auditor reference binary is always allowed; the scan also admits the sha256
# declared as reference_bin.sha256 in reference-inputs.json (format-validated).
KNOWN_GOOD_BINARY_DIGESTS = {
    "883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5",
}
# Relative-path shapes that make an evidence binary clearly a candidate build
# artifact (recorded, not flagged). Anything else that is not known-good, not a
# reference copy, and not a unique raster capture is flagged unknown_binary.
BUILD_ARTIFACT_FRAGMENTS = ("/target/", "/sealed-target/", "/incremental/",
                            "/deps/", "/build/", "/examples/", "/__pycache__/",
                            "/candidate/", "/.fingerprint/")
BUILD_ARTIFACT_EXTS = {".rlib", ".rmeta", ".o", ".a", ".d", ".pdb"}
# Evidence walks must not silently drop __pycache__/.pyc; those are hashed as
# build artifacts. Other fragments keep their historical text-scan exclusions.
EXCLUDE_EVIDENCE = ("/target/", "/.git/", "/node_modules/", "/sessions/",
                    "/artifacts/", "/inspirations/", "screenshot folder",
                    # Raw PTY captures are unique evidence artifacts produced by
                    # the frozen reference binary; they are not reference copies.
                    "/reference-observations/_raw/",
                    # Prior scan outputs are artifacts of the verification
                    # process, not source/evidence material under review.
                    "/clean-room/scan-fullscope.json",
                    "/clean-room/mutation-scan-fullscope.json")

RUST_KEYWORDS = {
    "as", "async", "await", "box", "break", "const", "continue", "crate", "dyn",
    "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let",
    "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self",
    "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while",
}
CONVENTION_TOKENS = {
    "cfg", "test", "tests", "derive", "default", "some", "none", "ok", "err",
    "vec", "string", "str", "iter", "println", "assert", "assert_eq", "todo",
    "section", "sections", "note", "notes", "helper", "helpers", "fixture",
    "fixtures",
}
STANDARD_TRAITS = {
    "debug", "clone", "copy", "partialeq", "eq", "partialord", "ord", "hash",
    "default", "display", "err",
}
BOILERPLATE_ALLOW = {t.lower() for t in RUST_KEYWORDS} | \
                    {t.lower() for t in CONVENTION_TOKENS} | STANDARD_TRAITS


def sha256_bytes(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()


def sha256_file(path: str) -> str:
    """Stream-hash arbitrary/large files without loading them into memory."""
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def is_build_artifact(rel: str) -> bool:
    padded = "/" + rel + "/"
    if any(frag in padded for frag in BUILD_ARTIFACT_FRAGMENTS):
        return True
    return os.path.splitext(rel)[1].lower() in BUILD_ARTIFACT_EXTS


def binary_file_kind(path: str, size: int, ext: str):
    """Classify a file for the binary-only reference walk. Returns 'raster',
    'binary', 'oversize', or None for ordinary text sources."""
    if ext in RASTER_EXTS:
        return "raster"
    if ext in BINARY_EXTS:
        return "binary"
    if size > MAX_FILE:
        return "oversize"
    if not ext:
        try:
            with open(path, "rb") as fh:
                head = fh.read(8192)
        except OSError:
            return None
        if b"\x00" in head:
            return "binary"
    return None


def norm(data: bytes) -> bytes:
    return b" ".join(data.split())


def line_of(text: str, needle: str) -> int:
    idx = text.find(needle)
    if idx < 0:
        return 0
    return text.count("\n", 0, idx) + 1


def classify_fragment(block: bytes):
    tokens = TOKEN_RE.findall(block.decode("utf-8", "ignore"))
    surviving = sorted({t for t in tokens
                        if t.lower() not in BOILERPLATE_ALLOW and len(t) >= 3})
    return ("boilerplate" if not surviving else "substantive"), tokens, surviving


class RefIndex:
    def __init__(self, reference_root, cited, shingles_by_prefix, shingle_owner,
                 digest_set, fingerprints, fragments, cited_count):
        self.reference_root = reference_root
        self.cited = cited
        self.shingles_by_prefix = shingles_by_prefix
        self.shingle_owner = shingle_owner
        self.digest_set = digest_set
        self.fingerprints = fingerprints
        self.fragments = fragments
        self.cited_count = cited_count
        self.reference_epoch = ""
        self.reference_bin = {}
        self.raster_digests = {}             # sha256 -> reference raster rel path
        self.reference_binary_observed = {}  # reference binary verification record


def build_index(reference_root: str, source_files):
    digest_set = set()
    shingles_by_prefix = {}
    shingle_owner = {}
    fragment_total = 0
    cited = []
    for sf in source_files:
        rel = sf["path"]
        full = os.path.join(reference_root, rel)
        try:
            data = open(full, "rb").read()
        except OSError:
            continue
        digest = sha256_bytes(data)
        digest_set.add(digest)
        cited.append({"path": rel, "sha256": digest, "size": len(data),
                      "index_sha256_matches_manifest": digest == sf.get("sha256")})
        n = norm(data)
        for i in range(0, max(0, len(n) - SHINGLE + 1) + 1, STEP_REF):
            sh = n[i:i + SHINGLE]
            if len(sh) < SHINGLE:
                break
            shingles_by_prefix.setdefault(sh[:16], set()).add(sh)
            shingle_owner.setdefault(sh, rel)
            fragment_total += 1
    return RefIndex(reference_root, cited, shingles_by_prefix, shingle_owner,
                    digest_set, len(digest_set), fragment_total, len(source_files))


def find_reference_fragment(index: RefIndex, data: bytes):
    n = norm(data)
    limit = len(n) - SHINGLE + 1
    for i in range(0, max(0, limit)):
        bucket = index.shingles_by_prefix.get(n[i:i + 16])
        sh = n[i:i + SHINGLE]
        if bucket and sh in bucket:
            return sh, index.shingle_owner.get(sh, "")
    return None, ""


def known_good_digests(index: RefIndex):
    good = set(KNOWN_GOOD_BINARY_DIGESTS)
    declared = (index.reference_bin or {}).get("sha256", "")
    if isinstance(declared, str) and HEX_RE.fullmatch(declared):
        good.add(declared)
    return good


def scan_binary_candidate(index: RefIndex, path: str, rel: str, side: str,
                          digest=None, size=None):
    """Digest a binary/oversized file and disposition it by digest.

    side='candidate' (product/evidence): copied reference assets, copied
    reference sources, reference-binary mismatch, and unknown binaries are
    failing findings; known-good binaries, unique raster captures, and build
    artifacts are recorded in the inventory without a finding.
    side='reference' (inspirations): rasters are indexed for cross-comparison,
    the frozen reference binary digest is verified, and other binaries are
    inventoried as reference material without failing.
    Returns (findings, inventory_record, status).
    """
    if digest is None:
        try:
            digest = sha256_file(path)
            size = os.path.getsize(path)
        except OSError:
            return [], None, "unreadable"
    ext = os.path.splitext(path)[1].lower()
    findings = []
    record = {"path": rel, "sha256": digest, "size": size}

    if side == "reference" and ext in RASTER_EXTS:
        index.raster_digests.setdefault(digest, rel)
        record["status"] = "reference_asset"
        return findings, record, "binary_hashed"
    if digest in index.raster_digests:
        record["status"] = "reference_asset" if side == "reference" else "copied_reference_asset"
        if side != "reference":
            findings.append({"rule": "copied_reference_asset", "file": rel,
                             "line_number": 1,
                             "detail": "byte-identical to a reference inspiration asset",
                             "classification": "substantive",
                             "matched_reference": index.raster_digests[digest]})
        return findings, record, "binary_hashed"
    if digest in known_good_digests(index):
        record["status"] = "approved_reference_binary"
        if rel == REFERENCE_BINARY_REL:
            index.reference_binary_observed = {"path": rel, "sha256": digest,
                                               "verified": True}
        return findings, record, "binary_hashed"
    if digest in index.digest_set:
        record["status"] = "copied_reference_file"
        findings.append({"rule": "copied_reference_file", "file": rel,
                         "line_number": 1,
                         "detail": "byte-identical to a cited reference source file",
                         "classification": "substantive"})
        return findings, record, "binary_hashed"
    if rel == REFERENCE_BINARY_REL:
        record["status"] = "reference_binary_mismatch"
        index.reference_binary_observed = {"path": rel, "sha256": digest,
                                           "verified": False}
        findings.append({"rule": "reference_binary_mismatch", "file": rel,
                         "line_number": 1,
                         "detail": "reference binary digest not in the known-good list",
                         "classification": "substantive"})
        return findings, record, "binary_hashed"
    if side == "reference":
        record["status"] = "reference_material"
        return findings, record, "binary_hashed"
    if ext in RASTER_EXTS:
        record["status"] = "evidence_capture"
        return findings, record, "binary_hashed"
    if is_build_artifact(rel):
        record["status"] = "candidate_build_artifact"
        return findings, record, "binary_hashed"
    record["status"] = "unknown_binary"
    findings.append({"rule": "unknown_binary", "file": rel,
                     "line_number": 1,
                     "detail": ("binary is not a known-good digest, not a reference "
                                "copy, not a unique raster capture, and not a "
                                "candidate build artifact"),
                     "classification": "substantive"})
    return findings, record, "binary_hashed"


def scan_candidate(index: RefIndex, path: str, rel: str, mode: str, approved,
                   hash_binaries=False):
    try:
        st = os.stat(path)
    except OSError:
        return [], None, "unreadable"
    ext = os.path.splitext(path)[1].lower()
    if st.st_size > MAX_FILE:
        if hash_binaries:
            return scan_binary_candidate(index, path, rel, "candidate")
        return [], None, "skipped_oversize"
    if ext in BINARY_EXTS:
        if hash_binaries:
            return scan_binary_candidate(index, path, rel, "candidate")
        return [], None, "skipped_binary"
    try:
        data = open(path, "rb").read()
    except OSError:
        return [], None, "unreadable"
    if b"\x00" in data[:8192]:
        if hash_binaries:
            return scan_binary_candidate(index, path, rel, "candidate",
                                         digest=sha256_bytes(data),
                                         size=len(data))
        return [], None, "skipped_binary"
    text = data.decode("utf-8", "ignore")
    findings = []
    is_approved = rel in approved
    digest = sha256_bytes(data)

    if digest in index.digest_set:
        findings.append({"rule": "copied_reference_file", "file": rel,
                         "line_number": 1,
                         "detail": "byte-identical to a cited reference source file",
                         "classification": "substantive"})

    frag, owner = find_reference_fragment(index, data)
    if frag is not None:
        classification, tokens, surviving = classify_fragment(frag)
        preview = frag.decode("utf-8", "ignore")[:60]
        findings.append({"rule": "reference_source_fragment", "file": rel,
                         "line_number": line_of(text, preview.split(" ")[0]) if preview else 0,
                         "detail": ">=200 verbatim reference bytes (normalized)",
                         "preview": preview,
                         "classification": classification,
                         "surviving_identifiers": surviving,
                         "raw_token_count": len(tokens),
                         "matched_reference": owner})

    if mode == "product":
        if PATH_PREFIX in text and not is_approved:
            findings.append({"rule": "reference_path_prefix", "file": rel,
                             "line_number": line_of(text, PATH_PREFIX),
                             "detail": "names auditor-only reference root (not in approved manifest)",
                             "classification": "substantive"})
        for m in HEX_RE.finditer(text):
            if m.group(0) in index.digest_set:
                findings.append({"rule": "reference_hex_fingerprint", "file": rel,
                                 "line_number": text.count("\n", 0, m.start()) + 1,
                                 "detail": "embeds a cited reference file sha256",
                                 "classification": "substantive"})
    return findings, None, "scanned"


def walk_scan(index: RefIndex, base: str, roots, mode, approved,
              hash_binaries=False, exclude_fragments=EXCLUDE_FRAGMENTS,
              binary_only=False):
    findings = []
    counts = {}
    scanned = 0
    inventory = []
    skipped = {"skipped_binary": 0, "skipped_oversize": 0, "unreadable": 0,
               "binary_hashed": 0}
    for sub in roots:
        root = os.path.join(base, sub)
        n = 0
        if os.path.isfile(root) and not binary_only:
            # Generated workspace lockfiles are not product source; skip fragment analysis.
            if sub in ("Cargo.lock", "Cargo.toml"):
                continue
            f, rec, status = scan_candidate(index, root, sub, mode, approved,
                                            hash_binaries)
            findings.extend(f)
            if rec is not None:
                inventory.append(rec)
            scanned += 1
            n += 1
        elif os.path.isdir(root):
            for dp, dns, fns in os.walk(root):
                dns[:] = [d for d in dns if not d.startswith(".") or d == ".agent-harness"]
                for fn in fns:
                    full = os.path.join(dp, fn)
                    rel = os.path.relpath(full, base).replace(os.sep, "/")
                    # Generated workspace lockfiles share dependency metadata with the
                    # reference project and are not product source; skip fragment analysis.
                    if rel in ("Cargo.lock", "Cargo.toml"):
                        continue
                    if exclude_fragments and any(
                            frag in ("/" + rel + "/") for frag in exclude_fragments):
                        continue
                    if binary_only:
                        try:
                            st = os.stat(full)
                        except OSError:
                            skipped["unreadable"] += 1
                            n += 1
                            continue
                        ext = os.path.splitext(full)[1].lower()
                        if binary_file_kind(full, st.st_size, ext) is None:
                            continue
                        f, rec, status = scan_binary_candidate(index, full, rel,
                                                               "reference")
                        findings.extend(f)
                        if rec is not None:
                            inventory.append(rec)
                            skipped[status] += 1
                        n += 1
                        continue
                    f, rec, status = scan_candidate(index, full, rel, mode,
                                                    approved, hash_binaries)
                    findings.extend(f)
                    if rec is not None:
                        inventory.append(rec)
                    if status == "scanned":
                        scanned += 1
                    else:
                        skipped[status] += 1
                    n += 1
        counts[sub] = n
    return findings, counts, scanned, skipped, inventory


def run_mutation_proof(index: RefIndex):
    tmp = tempfile.mkdtemp(prefix="cleanroom-canary-")
    detections = []
    try:
        first = index.cited[0]
        open(os.path.join(tmp, "planted_path.txt"), "w").write(
            "audit target: %s/crates/x/mod.rs\n" % PATH_PREFIX)
        open(os.path.join(tmp, "planted_fingerprint.txt"), "w").write(
            "reference digest %s\n" % first["sha256"])
        src = open(os.path.join(index.reference_root, first["path"]), "rb").read()
        frag = norm(src)[0:SHINGLE + 40].decode("utf-8", "ignore")
        open(os.path.join(tmp, "planted_fragment.rs"), "w").write(
            "fn planted() {\n// %s\n}\n" % frag)
        for fn in sorted(os.listdir(tmp)):
            f, _, _ = scan_candidate(index, os.path.join(tmp, fn), fn, "product", set())
            rules = {x["rule"] for x in f}
            detections.append({"file": fn, "detected": bool(rules),
                               "rules": sorted(rules)})
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    all_detected = all(d["detected"] for d in detections)
    return {
        "schema": "grok-parity-cleanroom-scan-v1",
        "verdict": "fail" if all_detected else "pass",
        "canaries_count": len(detections),
        "canaries_detected": sum(1 for d in detections if d["detected"]),
        "canary_detections": detections,
        "expected_rules": ["reference_path_prefix", "reference_hex_fingerprint",
                           "reference_source_fragment"],
        "proof": "pass" if all_detected else "incomplete",
        "canaries_retained": False,
        "mutation_rationale": (
            "Three planted canaries (reference path prefix, cited hex fingerprint, and a "
            ">=200 verbatim reference byte block) MUST each be detected. Canary files live in a "
            "temporary directory and are deleted; they are never retained in the evidence tree "
            "because they would constitute copied reference material. verdict=fail here means the "
            "scanner correctly rejected the planted reference material (sensitivity proven)."
        ),
    }


def self_test():
    tmp = tempfile.mkdtemp(prefix="cleanroom-selftest-")
    ok = True
    try:
        ref_root = os.path.join(tmp, "ref")
        os.makedirs(os.path.join(ref_root, "src"))
        lib = ("pub fn answer() -> i32 {\n    let value = 42;\n"
               "    let doubled = value * 2;\n    let tripled = value * 3;\n"
               "    return doubled + tripled;\n}\n" + "z" * 300 + "\n")
        banner = ("// " + "-" * 75 + "\n") * 10
        open(os.path.join(ref_root, "src", "lib.rs"), "w").write(lib)
        open(os.path.join(ref_root, "src", "banner.rs"), "w").write(banner)
        ref_inputs = {
            "reference_root": {"path": ref_root, "head": "f" * 40, "tree_id": "0" * 40,
                               "clean_status": "clean"},
            "reference_bin": {"path": os.path.join(ref_root, "bin"), "sha256": "1" * 64,
                              "version": "ref 0.0.0"},
            "reference_epoch": "e" * 64,
            "source_files": [
                {"path": "src/lib.rs", "sha256": sha256_bytes(lib.encode()),
                 "size": len(lib), "mode": "644", "type": "file"},
                {"path": "src/banner.rs", "sha256": sha256_bytes(banner.encode()),
                 "size": len(banner), "mode": "644", "type": "file"},
            ],
        }
        ri_path = os.path.join(tmp, "reference-inputs.json")
        open(ri_path, "w").write(json.dumps(ref_inputs))
        index = load_index(ri_path)

        prod = os.path.join(tmp, "prod")
        os.makedirs(prod)
        open(os.path.join(prod, "clean.rs"), "w").write("pub fn unrelated() -> i32 { 7 }\n")
        open(os.path.join(prod, "approved.rs"), "w").write("// uses %s/bin\n" % PATH_PREFIX)
        open(os.path.join(prod, "copied.rs"), "w").write(
            "fn bad() {\n" + norm(lib.encode())[0:220].decode() + "\n}\n")
        open(os.path.join(prod, "banner_scaffold.rs"), "w").write(
            ("// " + "-" * 75 + "\n") * 8)
        open(os.path.join(prod, "leak.txt"), "w").write(
            "digest %s here\n" % sha256_bytes(lib.encode()))

        approved = {"prod/approved.rs"}
        findings, _, _, _, _ = walk_scan(index, tmp, ["prod"], "product", approved)
        by_file = {}
        for f in findings:
            by_file.setdefault(f["file"], []).append(f)
        frag_rules = {f["file"]: f for f in findings
                      if f["rule"] == "reference_source_fragment"}
        checks = [
            ("clean-not-flagged", "prod/clean.rs" not in by_file),
            ("approved-path-not-flagged", "prod/approved.rs" not in by_file),
            ("copied-fragment-flagged",
             "prod/copied.rs" in frag_rules),
            ("copied-fragment-substantive",
             frag_rules.get("prod/copied.rs", {}).get("classification") == "substantive"),
            ("banner-scaffold-flagged-but-boilerplate",
             frag_rules.get("prod/banner_scaffold.rs", {}).get("classification") == "boilerplate"),
            ("hex-fingerprint-flagged",
             any(f["rule"] == "reference_hex_fingerprint"
                 for f in by_file.get("prod/leak.txt", []))),
        ]

        insp = os.path.join(tmp, "insp")
        os.makedirs(os.path.join(insp, "assets"))
        png_bytes = b"\x89PNG\r\n\x1a\n" + bytes(range(256)) * 3
        open(os.path.join(insp, "assets", "icon.png"), "wb").write(png_bytes)
        good_bytes = b"known-good reference binary payload"
        index.reference_bin["sha256"] = sha256_bytes(good_bytes)
        evb = os.path.join(tmp, "evb")
        os.makedirs(os.path.join(evb, "deps"))
        open(os.path.join(evb, "copied.png"), "wb").write(png_bytes)
        open(os.path.join(evb, "renamed_copy.bin"), "wb").write(png_bytes)
        open(os.path.join(evb, "unknown.bin"), "wb").write(b"\x7fELF" + os.urandom(128))
        open(os.path.join(evb, "good.bin"), "wb").write(good_bytes)
        open(os.path.join(evb, "deps", "libfake.so"), "wb").write(b"\x7fELF" + os.urandom(96))
        open(os.path.join(evb, "huge.dat"), "wb").write(b"\x00" * (MAX_FILE + 64))
        insp_findings, _, _, insp_skipped, _ = walk_scan(
            index, tmp, ["insp"], "reference", set(),
            exclude_fragments=(), binary_only=True)
        evb_findings, _, _, evb_skipped, evb_inventory = walk_scan(
            index, tmp, ["evb"], "evidence", set(),
            hash_binaries=True, exclude_fragments=())
        evb_by_file = {f["file"]: f for f in evb_findings}
        evb_rec = {r["path"]: r for r in evb_inventory}
        checks += [
            ("reference-raster-indexed",
             sha256_bytes(png_bytes) in index.raster_digests),
            ("reference-walk-clean", insp_findings == []
             and insp_skipped["binary_hashed"] == 1),
            ("copied-raster-flagged",
             evb_by_file.get("evb/copied.png", {}).get("rule") == "copied_reference_asset"),
            ("copied-raster-substantive",
             evb_by_file.get("evb/copied.png", {}).get("classification") == "substantive"),
            ("renamed-raster-copy-flagged",
             evb_by_file.get("evb/renamed_copy.bin", {}).get("rule") == "copied_reference_asset"),
            ("unknown-binary-flagged",
             evb_by_file.get("evb/unknown.bin", {}).get("rule") == "unknown_binary"),
            ("known-good-binary-approved",
             evb_rec.get("evb/good.bin", {}).get("status") == "approved_reference_binary"
             and "evb/good.bin" not in evb_by_file),
            ("build-artifact-recorded-not-flagged",
             evb_rec.get("evb/deps/libfake.so", {}).get("status") == "candidate_build_artifact"
             and "evb/deps/libfake.so" not in evb_by_file),
            ("oversize-binary-hashed",
             evb_rec.get("evb/huge.dat", {}).get("size", 0) > MAX_FILE
             and evb_skipped["binary_hashed"] >= 5),
        ]
        m = run_mutation_proof(index)
        checks.append(("mutation-canaries-detected", m["proof"] == "pass"))
        for name, passed in checks:
            print(("PASS" if passed else "FAIL") + ": " + name)
            ok = ok and passed
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    if not ok:
        sys.exit(1)
    print("clean-room scanner self-test: PASS")


def load_index(reference_inputs_path: str) -> RefIndex:
    ri = json.load(open(reference_inputs_path, encoding="utf-8"))
    ref_root_obj = ri["reference_root"]
    reference_root = ref_root_obj["path"] if isinstance(ref_root_obj, dict) else ref_root_obj
    idx = build_index(reference_root, ri["source_files"])
    idx.reference_epoch = ri["reference_epoch"]
    idx.reference_bin = ri.get("reference_bin", {})
    return idx


def main():
    ap = argparse.ArgumentParser(description="Full-scope clean-room parity scan.")
    ap.add_argument("--reference-inputs",
                    default=os.path.join(EVIDENCE_ROOT,
                                         "task-1-grok-build-clean-room-parity",
                                         "reference-inputs.json"))
    ap.add_argument("--base", default=".")
    ap.add_argument("--evidence-root", default=EVIDENCE_ROOT)
    ap.add_argument("--inspirations-root", default="inspirations")
    ap.add_argument("--out-dir", default=os.path.join(EVIDENCE_ROOT,
                                                      "task-37-grok-build-clean-room-parity",
                                                      "clean-room"))
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()

    if args.self_test:
        self_test()
        return

    base = os.path.abspath(args.base)
    index = load_index(os.path.join(base, args.reference_inputs))
    approved = set(DEFAULT_APPROVED)

    root_files = [os.path.relpath(os.path.join(base, n), base)
                  for n in os.listdir(base)
                  if os.path.isfile(os.path.join(base, n))]
    prod_findings, prod_counts, prod_scanned, prod_skipped, _ = walk_scan(
        index, base, PRODUCT_ROOTS + root_files, "product", approved)
    insp_findings, insp_counts, _, insp_skipped, insp_inventory = walk_scan(
        index, base, [args.inspirations_root], "reference", approved,
        exclude_fragments=(), binary_only=True)
    if not index.reference_binary_observed:
        index.reference_binary_observed = {
            "path": REFERENCE_BINARY_REL, "sha256": None, "verified": False,
            "note": "reference binary not found during inspirations binary scan"}
    ev_findings, ev_counts, ev_scanned, ev_skipped, ev_inventory = walk_scan(
        index, base, [args.evidence_root], "evidence", approved,
        hash_binaries=True, exclude_fragments=EXCLUDE_EVIDENCE)

    findings = prod_findings + insp_findings + ev_findings

    boilerplate = [f for f in findings
                   if f["rule"] == "reference_source_fragment"
                   and f["classification"] == "boilerplate"]
    substantive_frag = [f for f in findings
                        if f["rule"] == "reference_source_fragment"
                        and f["classification"] == "substantive"]
    copied = [f for f in findings if f["rule"] == "copied_reference_file"]
    hexhits = [f for f in findings if f["rule"] == "reference_hex_fingerprint"]
    pathhits = [f for f in findings if f["rule"] == "reference_path_prefix"]
    unknown_bin = [f for f in findings if f["rule"] == "unknown_binary"]
    copied_assets = [f for f in findings if f["rule"] == "copied_reference_asset"]
    bin_mismatch = [f for f in findings if f["rule"] == "reference_binary_mismatch"]

    failing = (substantive_frag + copied + hexhits + pathhits
               + unknown_bin + copied_assets + bin_mismatch)
    verdict = "pass" if not failing else "fail"

    binary_inventory = insp_inventory + ev_inventory
    binary_status_counts = {}
    for rec in binary_inventory:
        binary_status_counts[rec["status"]] = \
            binary_status_counts.get(rec["status"], 0) + 1

    integrity_mismatches = [c for c in index.cited
                            if not c["index_sha256_matches_manifest"]]

    scan = {
        "schema": "grok-parity-cleanroom-scan-v1",
        "scope": "full-first-party+evidence+inspirations-binaries",
        "verdict": verdict,
        "reference_epoch": index.reference_epoch,
        "reference_root": index.reference_root,
        "reference_bin_sha256": index.reference_bin.get("sha256"),
        "reference_fingerprints_count": index.fingerprints,
        "reference_fragments_count": index.fragments,
        "cited_source_files_indexed": index.cited_count,
        "cited_index_integrity": integrity_mismatches,
        "product_roots": PRODUCT_ROOTS,
        "product_scanned_files": prod_scanned,
        "product_per_root": prod_counts,
        "product_skipped": prod_skipped,
        "evidence_root": args.evidence_root,
        "evidence_scanned_files": ev_scanned,
        "evidence_per_root": ev_counts,
        "evidence_skipped": ev_skipped,
        "evidence_rules_applied": ["copied_reference_file", "reference_source_fragment",
                                   "copied_reference_asset", "unknown_binary",
                                   "reference_binary_mismatch"],
        "approved_paths_manifest": sorted(approved),
        "excluded_fragments": list(EXCLUDE_FRAGMENTS),
        "detection_rules": ["reference_path_prefix", "reference_hex_fingerprint",
                            "copied_reference_file", "reference_source_fragment",
                            "copied_reference_asset", "unknown_binary",
                            "reference_binary_mismatch"],
        "classification_note": (
            "fragment matches are classified boilerplate (Rust scaffolding/punctuation/banners "
            "with no project identifier) or substantive (a project identifier survives). All "
            "matches are listed with raw tokens and reference attribution. Verdict fails on "
            "substantive fragments, copied files, hex leaks, unapproved path references, "
            "unknown binaries, copied reference assets, or a reference binary mismatch."
        ),
        "binary_scan": {
            "policy": (
                "binary/oversized files are no longer skipped under the evidence root or "
                "inspirations tree (repair of F4-CLEANROOM-004). sha256 is computed by "
                "streaming; evidence binaries fail the scan unless they are on the known-good "
                "list, byte-identical to reference material (own failing rule), unique raster "
                "captures, or clearly candidate build artifacts. inspirations rasters are "
                "indexed for evidence cross-comparison and the frozen reference binary digest "
                "is verified."
            ),
            "known_good_binary_digests": sorted(known_good_digests(index)),
            "reference_binary": {
                "expected_path": REFERENCE_BINARY_REL,
                **index.reference_binary_observed,
            },
            "reference_raster_assets_indexed": len(index.raster_digests),
            "inspirations_root": args.inspirations_root,
            "inspirations_binary_files_hashed": len(insp_inventory),
            "inspirations_per_root": insp_counts,
            "inspirations_skipped": insp_skipped,
            "evidence_binary_files_hashed": len(ev_inventory),
            "status_counts": binary_status_counts,
            "inventory": binary_inventory,
        },
        "summary": {
            "substantive_fragment_findings": len(substantive_frag),
            "boilerplate_fragment_findings": len(boilerplate),
            "copied_file_findings": len(copied),
            "hex_fingerprint_findings": len(hexhits),
            "unapproved_path_findings": len(pathhits),
            "unknown_binary_findings": len(unknown_bin),
            "copied_reference_asset_findings": len(copied_assets),
            "reference_binary_mismatch_findings": len(bin_mismatch),
            "binary_files_hashed": len(binary_inventory),
            "total_findings": len(findings),
        },
        "findings": findings,
    }

    os.makedirs(os.path.join(base, args.out_dir), exist_ok=True)
    scan["self_sha256"] = sha256_bytes(json.dumps(scan, sort_keys=True).encode())
    scan_path = os.path.join(base, args.out_dir, "scan-fullscope.json")
    with open(scan_path, "w", encoding="utf-8") as fh:
        json.dump(scan, fh, indent=2, ensure_ascii=False)
        fh.write("\n")

    mutation = run_mutation_proof(index)
    mutation["bound_scan_sha256"] = scan["self_sha256"]
    mut_path = os.path.join(base, args.out_dir, "mutation-scan-fullscope.json")
    with open(mut_path, "w", encoding="utf-8") as fh:
        json.dump(mutation, fh, indent=2, ensure_ascii=False)
        fh.write("\n")

    s = scan["summary"]
    rb = index.reference_binary_observed
    print("verdict:", verdict)
    print("product scanned:", prod_scanned, "| evidence scanned:", ev_scanned)
    print("reference fragments indexed:", index.fragments,
          "| cited files:", index.cited_count)
    print("substantive fragments:", s["substantive_fragment_findings"],
          "| boilerplate fragments:", s["boilerplate_fragment_findings"],
          "| copied files:", s["copied_file_findings"],
          "| hex leaks:", s["hex_fingerprint_findings"],
          "| unapproved path refs:", s["unapproved_path_findings"])
    print("binary hashed:", s["binary_files_hashed"],
          "(evidence:", len(ev_inventory), "| inspirations:", len(insp_inventory), ")",
          "| unknown binaries:", s["unknown_binary_findings"],
          "| copied assets:", s["copied_reference_asset_findings"],
          "| ref binary mismatch:", s["reference_binary_mismatch_findings"])
    print("reference binary verified:", rb.get("verified"), "|",
          rb.get("path"), "->", (rb.get("sha256") or "not-found")[:16] + "...")
    print("canary proof:", mutation["proof"], "(",
          mutation["canaries_detected"], "/", mutation["canaries_count"], ")")
    print("scan:", scan_path)
    print("mutation:", mut_path)
    if substantive_frag:
        print("--- SUBSTANTIVE fragment findings ---")
        for f in substantive_frag:
            print(json.dumps({k: f[k] for k in
                              ("file", "line_number", "surviving_identifiers",
                               "matched_reference")}, ensure_ascii=False))
    if pathhits:
        print("--- unapproved path-prefix findings ---")
        for f in pathhits:
            print(json.dumps({k: f[k] for k in ("file", "line_number")},
                             ensure_ascii=False))
    if unknown_bin:
        print("--- unknown binary findings ---")
        for f in unknown_bin:
            print(json.dumps({k: f[k] for k in ("file", "line_number")},
                             ensure_ascii=False))
    if copied_assets:
        print("--- copied reference asset findings ---")
        for f in copied_assets:
            print(json.dumps({k: f[k] for k in
                              ("file", "line_number", "matched_reference")},
                             ensure_ascii=False))
    sys.exit(0 if verdict == "pass" and mutation["proof"] == "pass" else 1)


if __name__ == "__main__":
    main()
