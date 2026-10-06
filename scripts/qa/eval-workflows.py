#!/usr/bin/env python3
"""Opt-in live A/B measurements. Uses synthetic workspaces and existing auth.

Pass two built harness binaries and their gpt-6.md prompt files. Results contain
only counts, correctness and timings; provider payloads/credentials stay out.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import subprocess
import time


def fixture(root, task):
    root.mkdir(parents=True)
    if task == "research":
        facts = {
            "api": {"owner": "delivery", "retries": 2, "timeout_ms": 1800},
            "queue": {"owner": "platform", "retries": 5, "timeout_ms": 4000},
            "billing": {"owner": "payments", "retries": 0, "timeout_ms": 2200},
            "search": {"owner": "discovery", "retries": 3, "timeout_ms": 900},
            "media": {"owner": "delivery", "retries": 1, "timeout_ms": 6000},
            "identity": {"owner": "platform", "retries": 2, "timeout_ms": 1500},
        }
        for name, fields in facts.items():
            (root / f"{name}.json").write_text(json.dumps({"service": name, **fields}))
        expected = {name: fields["owner"] for name, fields in facts.items()
                    if fields["retries"] >= 2 and fields["timeout_ms"] < 2000}
        prompt = "Inspect every service JSON file. Which services have at least 2 retries and timeout_ms below 2000? Return a service-to-owner object."
    elif task == "filter":
        expected = {"paid_eu_cents": 0, "paid_eu_count": 0}
        for shard in range(6):
            rows = [{"id": shard * 200 + i, "region": "EU" if i % 3 else "US",
                     "status": "paid" if i % 5 else "void", "cents": (i * 37 + shard * 19) % 9000}
                    for i in range(200)]
            (root / f"orders-{shard}.jsonl").write_text("".join(json.dumps(r) + "\n" for r in rows))
            for row in rows:
                if row["region"] == "EU" and row["status"] == "paid":
                    expected["paid_eu_cents"] += row["cents"]
                    expected["paid_eu_count"] += 1
        prompt = "Inspect all orders-*.jsonl shards. Compute total cents and count for paid EU orders. Return paid_eu_cents and paid_eu_count."
    else:
        (root / "prices.py").write_text("def total(items):\n    return sum(item['price_cents'] for item in items)\n")
        (root / "verify.py").write_text("from prices import total\nassert total([]) == 0\nassert total([{'price_cents': 105, 'quantity': 3}]) == 315\nprint('verified')\n")
        expected = {"fixed": True}
        prompt = "Fix prices.total: each item contributes price_cents times quantity, and a missing quantity means 1. Keep integer cents exact, handle empty lists, and run verification. Return fixed: true when done."
    return prompt + ' End with one line: ANSWER: <JSON object>.', expected


def measure(binary, prompt_file, config, workspace, sessions, prompt, expected, task):
    command = [str(binary), "--config", str(config), "--cwd", str(workspace),
               "--session-dir", str(sessions), "prompt", "--format", "json",
               "--model", MODEL, "--reasoning-effort", "low", "--max-turns", "10",
               "--no-subagents", "--no-memory", "--disable-web-search",
               "--tools", "read,glob,grep,edit,write,bash,eval",
               "--system-prompt-override", prompt_file.read_text(), "--text", prompt]
    start = time.monotonic()
    try:
        run = subprocess.run(command, text=True, capture_output=True, timeout=240)
    except subprocess.TimeoutExpired:
        return {"correct": False, "failure": "wall_timeout", "elapsed_seconds": 240}
    elapsed = round(time.monotonic() - start, 3)
    try:
        events = [r["event"]["payload"] for r in json.loads(run.stdout) if r["delivery"] == "durable"]
    except (ValueError, KeyError):
        # Error output may include private paths/provider diagnostics. Keep it local.
        (sessions.parent / "diagnostic.txt").write_text(run.stderr)
        return {"correct": False, "failure": "invalid_event_output", "exit_code": run.returncode,
                "elapsed_seconds": elapsed}
    types = Counter(e["event_type"] for e in events)
    finished = [e["data"] for e in events if e["event_type"] == "provider_request_finished"]
    messages = [e["data"] for e in events if e["event_type"] == "assistant_message_finished"]
    answer = "\n".join(p.get("text", "") for m in messages for p in m["parts"] if p.get("kind") == "text")
    # AssistantPart uses a tagged representation; accept the documented text tag.
    if not answer:
        answer = "\n".join(p.get("text", "") for m in messages for p in m["parts"] if p.get("type") == "text")
    matches = re.findall(r"ANSWER:\s*(\{[^\n]*\})", answer)
    try:
        correct = bool(matches) and json.loads(matches[-1]) == expected
    except ValueError:
        correct = False
    if task == "edit":
        verify = subprocess.run(["python3", "-c", "from prices import total; assert total([])==0; assert total([{'price_cents':7},{'price_cents':11,'quantity':4},{'price_cents':3,'quantity':0}])==51"], cwd=workspace, capture_output=True)
        correct = correct and verify.returncode == 0
    return {"correct": correct and run.returncode == 0, "exit_code": run.returncode,
            "elapsed_seconds": elapsed, "provider_requests": types["provider_request_started"],
            "usage_complete": bool(finished) and all(e.get("usage") is not None for e in finished),
            "usage": {k: sum((e.get("usage") or {}).get(k, 0) for e in finished)
                      for k in ["prompt_tokens", "completion_tokens", "total_tokens"]},
            "cache_read_tokens": sum((e.get("metadata") or {}).get("cache_read_tokens") or 0 for e in finished),
            "tools": dict(Counter(e["data"]["tool_id"] for e in events if e["event_type"] == "tool_call_requested")),
            "task_failed": types["task_failed"], "run_failed": types["run_failed"]}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--baseline-prompt", type=Path, required=True)
    parser.add_argument("--candidate-prompt", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--model", default="openai-codex/gpt-6-sol")
    args = parser.parse_args()
    MODEL = args.model
    args.output.mkdir(parents=True, exist_ok=True)
    config = args.output / "measurement-config.json"
    config.write_text(json.dumps({"model": MODEL, "permission": "allow", "formatter": False,
        "provider": {"openai-codex": {"type": "openai_compatible", "options": {
            "authProvider": "codex", "baseURL": "https://api.openai.com/v1", "cacheRetention": "none"},
            "models": {"gpt-6-sol": {"limit": {"context": 1050000, "input": 288384, "output": 128000},
                "metadata": {"supportsToolCalls": True}}}}}}))
    results = {"model": MODEL, "reasoning_effort": "low", "max_turns": 10,
               "order": "ABBA per task", "binaries": {}, "runs": []}
    for arm in ["baseline", "candidate"]:
        results["binaries"][arm] = {"sha256": hashlib.sha256(getattr(args, arm).read_bytes()).hexdigest(),
            "prompt_sha256": hashlib.sha256(getattr(args, arm + "_prompt").read_bytes()).hexdigest()}
    for task in ["research", "filter", "edit"]:
        for index, arm in enumerate(["baseline", "candidate", "candidate", "baseline"]):
            run_root = args.output / f"{task}-{index}-{arm}"
            prompt, expected = fixture(run_root / "workspace", task)
            result = measure(getattr(args, arm).resolve(), getattr(args, arm + "_prompt"), config.resolve(),
                             (run_root / "workspace").resolve(), (run_root / "sessions").resolve(),
                             prompt, expected, task)
            results["runs"].append({"task": task, "arm": arm, **result})
            (args.output / "results.json").write_text(json.dumps(results, indent=2) + "\n")
            print(json.dumps(results["runs"][-1]), flush=True)
            if result.get("failure"):
                raise SystemExit("Live run failed; inspect local diagnostics before continuing")
    raise SystemExit(0 if all(r["correct"] and r["usage_complete"] for r in results["runs"]) else 1)
