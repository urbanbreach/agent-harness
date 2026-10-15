#!/usr/bin/env python3
"""Rebuild crates/harness-tokenizer/data from the public upstream vocabulary files.

Usage: pack_assets.py CACHE_DIR [--fetch]

CACHE_DIR holds the upstream files under the names listed in SOURCES. With --fetch missing files
are downloaded from their public origin. Every file is checked against its pinned SHA-256 before
packing, so upstream drift fails loudly instead of changing token counts. Output is
data/<name>.bin.zst (or .json.zst for the Claude count tables) plus data/manifest.json.

Requires the `zstd` command line tool. No network access is needed once CACHE_DIR is populated.
"""

import hashlib
import json
import re
import subprocess
import sys
import urllib.request
from pathlib import Path

DATA = Path(__file__).resolve().parent.parent / "data"
CTOK_REV = "df3b59b5e645289a5eadc8e24036b99d39c333c4"
CTOK_RAW = f"https://raw.githubusercontent.com/sanderland/ctok/{CTOK_REV}/ctok/data/"

# name -> (encoding, url, upstream file name, sha256 of the downloaded bytes, license id)
SOURCES = {
    "o200k_base": (
        "O200k",
        "https://openaipublic.blob.core.windows.net/encodings/o200k_base.tiktoken",
        "o200k_base.tiktoken",
        "446a9538cb6c348e3516120d7c08b09f57c36495e2acfffe59a5bf8b0cfb1a2d",
        "MIT",
    ),
    "kimi_k2": (
        "KimiK2",
        "https://huggingface.co/moonshotai/Kimi-K3/resolve/main/tiktoken.model",
        "tiktoken.model",
        "b6c497a7469b33ced9c38afb1ad6e47f03f5e5dc05f15930799210ec050c5103",
        "Kimi K3 License",
    ),
    "qwen3": (
        "Qwen3",
        "https://huggingface.co/Qwen/Qwen3.8-27B/resolve/main/tokenizer.json",
        "tokenizer.json",
        "0997f410c57a1f4e53b09e4be8f4a172d90edd9564368fb0847030937229b9f3",
        "Apache-2.0",
    ),
    "deepseek3": (
        "DeepSeekV3",
        "https://huggingface.co/deepseek-ai/DeepSeek-V4-Flash/resolve/main/tokenizer.json",
        "tokenizer.json",
        "8f9f37ca37fdc4f5fd36d5cf4d3b0e8392edb4e894fd10cc0d70b4957c8633cf",
        "MIT",
    ),
    "glm5": (
        "Glm5",
        "https://huggingface.co/zai-org/GLM-5/resolve/main/tokenizer.json",
        "tokenizer.json",
        "19e773648cb4e65de8660ea6365e10acca112d42a854923df93db4a6f333a82d",
        "MIT (model card declaration)",
    ),
    "ctok_v3": (
        "ClaudeV3",
        CTOK_RAW + "pieces_v3.json",
        "ctok/data/pieces_v3.json",
        "45cfe22c2dc3bd6277e074bc6848d6578adab99cda76db52f7d788c79821437a",
        "MIT",
    ),
    "ctok_v4_7": (
        "ClaudeV47, ClaudeV5, ClaudeV5Sonnet",
        CTOK_RAW + "pieces_v4_7.json",
        "ctok/data/pieces_v4_7.json",
        "4c3baace1160eb2b2bf59e5b1f9f33730e77b250f074a2ee96a2d2d1c683e739",
        "MIT",
    ),
}

# The reference observed these hashes on whitespace-free (minified) re-serialisations of the HF
# JSON files with JavaScript key order. Reproducing them proves the downloaded files carry the
# same content as the snapshots the count fixtures were taken from.
CANONICAL = {
    "qwen3": (9089356, "ae50d65939a34a089eaf1e541c15c8a7d866079a5fe9a1d35454e0aa88577753"),
    "deepseek3": (4363807, "3ad46a8c78edf140516eb696f948ae6e7c3522ec64993b219833145e2e9951d0"),
    "glm5": (8683422, "f73933d2e4b633ce5a75f049a68b339790c52b966025e895aae0a0eda67d5cda"),
}

# name -> (vocabulary entries, merges, excluded added tokens)
HF_SHAPE = {
    "qwen3": (248044, 247587, 33),
    "deepseek3": (128000, 127741, 1283),
    "glm5": (154820, 321649, 36),
}
TIKTOKEN_SIZE = {"o200k_base": 199998, "kimi_k2": 163584}

SPECIAL_ATOM = {"bow": "\ufdd0", "eow": "\ufdd1", "shift": "\ufdd3", "caps": "\ufdd4"}
ATOM_RE = re.compile(r"\u27e8(?:bow|eow|shift|caps|pad|0x[0-9A-Fa-f]{2})\u27e9|.", re.DOTALL)


def varint(n: int) -> bytes:
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def zstd_pack(payload: bytes, out: Path) -> None:
    out.write_bytes(
        subprocess.run(
            ["zstd", "-19", "-q", "-c"], input=payload, check=True, stdout=subprocess.PIPE
        ).stdout
    )


def js_key_order(obj):
    """Order object keys the way JSON.stringify does: canonical array indices first."""
    if isinstance(obj, dict):
        ints = sorted(
            (int(k), k) for k in obj if re.fullmatch(r"0|[1-9][0-9]*", k) and int(k) < 2**32 - 1
        )
        index_keys = {k for _, k in ints}
        out = {k: js_key_order(obj[k]) for _, k in ints}
        out.update({k: js_key_order(v) for k, v in obj.items() if k not in index_keys})
        return out
    if isinstance(obj, list):
        return [js_key_order(v) for v in obj]
    return obj


def pack_tiktoken(name: str, raw: bytes) -> bytes:
    import base64

    entries = {}
    for line in raw.decode("ascii").splitlines():
        token, rank = line.split(" ")
        entries[int(rank)] = base64.b64decode(token)
    n = TIKTOKEN_SIZE[name]
    assert sorted(entries) == list(range(n)), f"{name}: ranks are not contiguous 0..{n - 1}"
    out = bytearray(varint(n))
    for rank in range(n):
        out += varint(len(entries[rank])) + entries[rank]
    return bytes(out)


def pack_hf(name: str, raw: bytes) -> tuple[bytes, dict]:
    doc = json.loads(raw)
    size, canon_sha = len(raw), None
    canonical = json.dumps(js_key_order(doc), separators=(",", ":"), ensure_ascii=False).encode()
    want_bytes, want_sha = CANONICAL[name]
    canon_sha = hashlib.sha256(canonical).hexdigest()
    assert (len(canonical), canon_sha) == (want_bytes, want_sha), f"{name}: canonical form drift"

    model = doc["model"]
    n_vocab, n_merges, n_added = HF_SHAPE[name]
    assert model["type"] == "BPE"
    vocab = model["vocab"]
    assert len(vocab) == n_vocab and sorted(vocab.values()) == list(range(n_vocab))
    assert len(model["merges"]) == n_merges and len(doc["added_tokens"]) == n_added
    for key in ("dropout", "unk_token", "continuing_subword_prefix", "end_of_word_suffix"):
        assert model.get(key) in (None, ""), f"{name}: unsupported model.{key}"
    assert not model.get("fuse_unk") and not model.get("byte_fallback")

    merges = []
    for merge in model["merges"]:
        left, right = merge.split(" ") if isinstance(merge, str) else merge
        assert left and right and left + right in vocab, f"{name}: bad merge"
        merges.append((vocab[left], vocab[right], vocab[left + right]))
    # Every vocabulary entry that is the result of a merge is rebuilt from its parts at load time,
    # so only the remaining entries (the byte alphabet and a few extras) are stored as text.
    produced = {result for _, _, result in merges}
    literals = [(ident, token) for token, ident in sorted(vocab.items(), key=lambda kv: kv[1])
                if ident not in produced]
    header = {
        "normalizer": doc["normalizer"],
        "pre_tokenizer": doc["pre_tokenizer"],
        "ignore_merges": bool(model.get("ignore_merges", False)),
        "vocab": n_vocab,
        "merges": n_merges,
        "literals": len(literals),
    }
    head = json.dumps(header, separators=(",", ":"), ensure_ascii=False).encode()
    out = bytearray(len(head).to_bytes(4, "little") + head)
    previous = 0
    for ident, token in literals:
        data = token.encode()
        out += varint(ident - previous) + varint(len(data)) + data
        previous = ident
    previous = 0
    for _, _, result in merges:  # zigzag deltas: merge results are nearly sequential
        delta = result - previous
        out += varint(delta << 1 if delta >= 0 else ((-delta) << 1) - 1)
        previous = result
    for side in (0, 1):  # byte planes of the 24-bit left then right part ids
        for shift in (0, 8, 16):
            out += bytes((m[side] >> shift) & 0xFF for m in merges)
    return bytes(out), {"canonical_json_bytes": len(canonical), "canonical_json_sha256": canon_sha}


def parse_marked(public: str) -> str:
    """Public marker notation -> single-codepoint internal glyphs, byte escapes decoded."""
    parts, buf = [], bytearray()

    def flush():
        if buf:
            parts.append(bytes(buf).decode("utf-8"))
            buf.clear()

    for m in ATOM_RE.finditer(public):
        tok = m.group(0)
        if len(tok) == 1:
            flush()
            parts.append(tok)
        elif tok[1:-1] in SPECIAL_ATOM:
            flush()
            parts.append(SPECIAL_ATOM[tok[1:-1]])
        elif tok[1:-1].startswith("0x"):
            buf.append(int(tok[3:-1], 16))
        else:
            flush()
            parts.append(tok)
    flush()
    return "".join(parts)


def pack_ctok(raw: bytes) -> bytes:
    doc = json.loads(raw)
    tokens = doc["tokens"]
    pieces = [parse_marked(p) for g, ps in tokens.items() if g != "bytes_fallback" for p in ps]
    payload = {
        "pieces": pieces,
        "contractions": [parse_marked(c) for c in tokens["contractions"]],
        "bytes": list(tokens["bytes_fallback"]),
    }
    return json.dumps(payload, separators=(",", ":"), ensure_ascii=False).encode()


def fetch(url: str, dest: Path) -> None:
    with urllib.request.urlopen(url) as resp:
        dest.write_bytes(resp.read())


def main() -> None:
    cache = Path(sys.argv[1])
    cache.mkdir(parents=True, exist_ok=True)
    DATA.mkdir(exist_ok=True)
    manifest = {"ctok_revision": CTOK_REV, "assets": []}
    for name, (encoding, url, upstream, sha, license_id) in SOURCES.items():
        src = cache / f"{name}.source"
        if not src.exists() and "--fetch" in sys.argv:
            fetch(url, src)
        raw = src.read_bytes()
        digest = hashlib.sha256(raw).hexdigest()
        assert digest == sha, f"{name}: upstream drift, got {digest}"
        extra = {}
        if name in TIKTOKEN_SIZE:
            payload, ext = pack_tiktoken(name, raw), "bin"
        elif name in HF_SHAPE:
            (payload, extra), ext = pack_hf(name, raw), "bin"
        else:
            payload, ext = pack_ctok(raw), "json"
        out = DATA / f"{name}.{ext}.zst"
        zstd_pack(payload, out)
        packed = out.read_bytes()
        manifest["assets"].append(
            {
                "file": out.name,
                "encodings": encoding,
                "source_url": url,
                "upstream_name": upstream,
                "upstream_bytes": len(raw),
                "upstream_sha256": digest,
                "license": license_id,
                "payload_bytes": len(payload),
                "payload_sha256": hashlib.sha256(payload).hexdigest(),
                "packed_bytes": len(packed),
                "packed_sha256": hashlib.sha256(packed).hexdigest(),
                **extra,
            }
        )
        print(f"{out.name}: {len(raw)} -> {len(packed)} bytes")
    (DATA / "manifest.json").write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
