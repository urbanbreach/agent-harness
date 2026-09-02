import assert from "node:assert/strict";
import { chmod, mkdir, mkdtemp, readFile, rm, stat, symlink, writeFile } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { createCleanupOwner } from "./lib/cleanup.mjs";
import { prepareEvidence, writeFailureEvidence, writePassEvidence } from "./lib/evidence.mjs";
import {
  assertScreenshotSafe,
  assertSecretFree,
  safePtyEnvironment,
  validateEvidenceDir,
} from "./lib/security.mjs";

const png = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
  "base64",
);

test("validateEvidenceDir rejects destructive and escaping paths", async () => {
  // Given: a repository boundary and destructive or escaping path inputs.
  const repoRoot = await mkdtemp(join(tmpdir(), "harness-security-repo-"));
  await mkdir(join(repoRoot, ".omo", "evidence"), { recursive: true });
  const invalid = ["/", homedir(), repoRoot, ".", join(repoRoot, ".omo", "evidence"),
    join(repoRoot, ".omo", "evidence", "..", "outside"), join(tmpdir(), "ordinary-evidence")];
  try {
    // When/Then: every protected, traversal, or non-dedicated path is rejected.
    for (const path of invalid) {
      await assert.rejects(validateEvidenceDir(path, repoRoot), /unsafe evidence directory/);
    }
  } finally {
    await rm(repoRoot, { recursive: true, force: true });
  }
});

test("validateEvidenceDir accepts repo descendants and dedicated temp paths", async () => {
  // Given: one repo evidence child and one dedicated temporary evidence path.
  const repoRoot = await mkdtemp(join(tmpdir(), "harness-security-repo-"));
  const repoEvidence = join(repoRoot, ".omo", "evidence", "run-1");
  const tempEvidence = join(tmpdir(), "harness-xterm-run-1");
  try {
    // When: both paths cross the validator.
    const actual = await Promise.all([
      validateEvidenceDir(repoEvidence, repoRoot), validateEvidenceDir(tempEvidence, repoRoot),
    ]);
    // Then: their resolved dedicated paths are retained.
    assert.deepEqual(actual, [repoEvidence, tempEvidence]);
  } finally {
    await rm(repoRoot, { recursive: true, force: true });
  }
});

test("validateEvidenceDir rejects symlinked evidence roots", async () => {
  // Given: a repository whose evidence root redirects outside the repository.
  const repoRoot = await mkdtemp(join(tmpdir(), "harness-security-repo-"));
  const redirected = await mkdtemp(join(tmpdir(), "harness-security-outside-"));
  await mkdir(join(repoRoot, ".omo"), { recursive: true });
  await symlink(redirected, join(repoRoot, ".omo", "evidence"));
  try {
    // When/Then: a lexical descendant cannot cross the symlink boundary.
    await assert.rejects(
      validateEvidenceDir(join(repoRoot, ".omo", "evidence", "run-1"), repoRoot),
      /unsafe evidence directory/,
    );
  } finally {
    await rm(repoRoot, { recursive: true, force: true });
    await rm(redirected, { recursive: true, force: true });
  }
});

test("safePtyEnvironment includes execution settings without credentials", () => {
  // Given: required toolchain values plus credentials and an injection hook.
  const host = { PATH: "/bin", HOME: "/home/tester", USER: "tester", CARGO_HOME: "/cargo",
    LANG: "en_US.UTF-8", CI: "1", AWS_SECRET_ACCESS_KEY: "secret", GITHUB_TOKEN: "token",
    NODE_OPTIONS: "--require credential-stealer" };
  // When: the PTY environment is assembled.
  const environment = safePtyEnvironment(host, { HARNESS_SEED: "42" });
  // Then: safe values remain, while credentials and injection hooks do not.
  assert.deepEqual(environment, { PATH: "/bin", HOME: "/home/tester", USER: "tester",
    CARGO_HOME: "/cargo", LANG: "en_US.UTF-8", CI: "1", HARNESS_SEED: "42" });
});

test("evidence persistence redacts terminal and structured secrets", async () => {
  // Given: terminal output and metadata containing each protected secret class.
  const evidenceDir = await mkdtemp(join(tmpdir(), "harness-xterm-redaction-"));
  await writeFile(join(evidenceDir, "terminal.png"), png);
  const secret = ["Authorization: Bearer abc.def.ghi", "Cookie: session=top-secret",
    "PASSWORD=hunter2", "sk-1234567890abcdefghijklmnop",
    "ghp_1234567890abcdefghijklmnop", "AKIA1234567890ABCDEF",
    "-----BEGIN PRIVATE KEY-----\nprivate-material\n-----END PRIVATE KEY-----"].join("\n");
  try {
    // When: passing evidence is durably serialized.
    await writePassEvidence(passSettings(evidenceDir, secret));
    const durable = await Promise.all(["terminal.ansi", "terminal-ansi.txt", "terminal.txt",
      "buffer.json", "interactions.json", "metadata.json"]
      .map((name) => readFile(join(evidenceDir, name), "utf8")));
    // Then: no original secret survives in raw, text, metadata, or interactions.
    for (const contents of durable) {
      assert.doesNotMatch(
        contents,
        /abc\.def\.ghi|top-secret|hunter2|private-material|sk-123|ghp_123|AKIA123/,
      );
    }
    assert.match(durable.join("\n"), /\[REDACTED\]/);
  } finally {
    await rm(evidenceDir, { recursive: true, force: true });
  }
});

test("evidence directories and artifacts use private modes", async () => {
  // Given: a dedicated evidence path with permissive modes.
  const evidenceDir = join(tmpdir(), `harness-xterm-modes-${process.pid}`);
  await mkdir(evidenceDir, { recursive: true, mode: 0o755 });
  await chmod(evidenceDir, 0o755);
  try {
    // When: the evidence lifecycle prepares and writes the directory.
    await prepareEvidence(evidenceDir);
    await writeFailureEvidence({ evidenceDir, error: new Error("fixture failure") });
    // Then: both directory and artifact are owner-only.
    assert.equal((await stat(evidenceDir)).mode & 0o777, 0o700);
    assert.equal((await stat(join(evidenceDir, "failure.json"))).mode & 0o777, 0o600);
  } finally {
    await rm(evidenceDir, { recursive: true, force: true });
  }
});

test("evidence preparation preserves its atomically allocated root", async () => {
  // Given: an atomically created private root containing stale evidence.
  const evidenceDir = await mkdtemp(join(tmpdir(), "harness-xterm-atomic-root-"));
  const before = await stat(evidenceDir);
  await writeFile(join(evidenceDir, "stale.json"), "{}");
  try {
    // When: evidence is prepared for a fresh capture.
    await prepareEvidence(evidenceDir);
    // Then: the owned root remains the same inode while stale contents are removed.
    const after = await stat(evidenceDir);
    assert.equal(after.ino, before.ino);
    await assert.rejects(readFile(join(evidenceDir, "stale.json")), /ENOENT/);
  } finally {
    await rm(evidenceDir, { recursive: true, force: true });
  }
});

test("writePassEvidence refuses PASS when any cleanup invariant failed", async () => {
  // Given: otherwise valid evidence with a process group still alive.
  const evidenceDir = await mkdtemp(join(tmpdir(), "harness-xterm-cleanup-refusal-"));
  await writeFile(join(evidenceDir, "terminal.png"), png);
  const settings = passSettings(evidenceDir, "safe output");
  settings.cleanup.pty.processGroupAlive = true;
  try {
    // When/Then: PASS persistence fails closed before creating PASS.json.
    await assert.rejects(writePassEvidence(settings), /cleanup invariants/);
    await assert.rejects(readFile(join(evidenceDir, "PASS.json")), /ENOENT/);
  } finally {
    await rm(evidenceDir, { recursive: true, force: true });
  }
});

test("signal cleanup owns each active resource exactly once", async () => {
  // Given: active PTY, browser, and temp resources with observable cleanup calls.
  const calls = [];
  const state = {};
  const owner = createCleanupOwner(state, {
    beforeTempRootRemoval: async () => calls.push("after-resources"),
    removeTempRoot: async (path) => {
      calls.push(`temp:${path}`);
      return true;
    },
  });
  owner.ownPty({ cleanup: async () => { calls.push("pty"); return { childExited: true }; } });
  owner.ownBrowser({ close: async () => { calls.push("browser"); return { contextClosed: true }; } });
  owner.ownTempRoot("/tmp/harness-xterm-owned");
  const target = { exitCode: 0 };
  // When: SIGINT cleanup and normal-finally cleanup race for ownership.
  await Promise.all([owner.handleSignal("SIGINT", target), owner.cleanup(), owner.cleanup()]);
  // Then: each cleanup executes once and the signal exit status is retained.
  assert.deepEqual(calls, ["pty", "browser", "after-resources", "temp:/tmp/harness-xterm-owned"]);
  assert.equal(target.exitCode, 130);
  assert.equal(state.tempRootRemoved, true);
});

test("resources acquired after signal cleanup are rejected and closed", async () => {
  // Given: signal cleanup has already completed before a PTY is acquired.
  const calls = [];
  const owner = createCleanupOwner({}, { removeTempRoot: async () => true });
  await owner.handleSignal("SIGTERM", { exitCode: 0 });

  // When: setup races ahead and attempts to register the late PTY.
  assert.throws(
    () => owner.ownPty({ cleanup: async () => {
      calls.push("late-pty");
      return { childExited: true };
    } }),
    /after xterm QA cleanup started/,
  );
  await owner.cleanup();

  // Then: the resource is still closed exactly once and setup cannot continue.
  assert.deepEqual(calls, ["late-pty"]);
});

test("secret scanning decodes buffers and blocks secret-bearing screenshots", () => {
  // Given: recognized credentials in raw PTY bytes and visible terminal text.
  const secret = "Authorization: Bearer visible-secret";

  // When/Then: neither durable bytes nor screenshot pixels can cross the evidence boundary.
  assert.throws(() => assertSecretFree(Buffer.from(secret)), /secret scan/);
  assert.throws(() => assertScreenshotSafe({ text: secret }), /secret scan/);
  for (const standalone of [
    "sk-1234567890abcdefghijklmnop",
    "ghp_1234567890abcdefghijklmnop",
    "AKIA1234567890ABCDEF",
  ]) {
    assert.throws(() => assertSecretFree(standalone), /secret scan/);
  }
});

function passSettings(evidenceDir, secret) {
  return {
    evidenceDir, raw: Buffer.from(secret),
    capture: { cols: 2, rows: 1, activeBuffer: "normal", cursor: {}, modes: {},
      text: `Harness runtime\n${secret}`, title: "Harness security fixture",
      renderCount: 1, parsedCount: 1 },
    captures: [], interactions: [{ action: { kind: "type", value: secret } }],
    contract: { name: "test", title: secret, command: `TOKEN=${secret}` },
    command: `PASSWORD=${secret}`, argv: [secret], repoRoot: "/repo", browser: "/browser",
    browserMetadata: { console: [{ text: secret }] }, sourceTree: { hash: "tree" },
    scriptSha256: "script", assertions: [],
    cleanup: {
      pty: { childExited: true, processGroupAlive: false, stdinClosed: true, temporarySockets: [] },
      browser: { pageClosed: true, contextClosed: true, browserConnectedAfterClose: false,
        profileRemoved: true, boundPorts: [] },
      tempRootRemoved: true,
    },
  };
}
