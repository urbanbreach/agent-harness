import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { readFile, stat } from "node:fs/promises";
import { relative, resolve } from "node:path";

export function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

export async function currentTree(repoRoot) {
  const listed = execFileSync(
    "git",
    ["ls-files", "--cached", "--others", "--exclude-standard", "-z"],
    { cwd: repoRoot },
  ).toString("utf8");
  const paths = listed.split("\0").filter((path) => path && included(path)).sort();
  const digest = createHash("sha256");
  let files = 0;
  for (const path of paths) {
    const absolute = resolve(repoRoot, path);
    const details = await stat(absolute);
    if (!details.isFile()) continue;
    digest.update(path).update("\0").update(await readFile(absolute)).update("\0");
    files += 1;
  }
  return {
    algorithm: "sha256(path\\0content\\0)",
    hash: digest.digest("hex"),
    files,
    head: git(repoRoot, ["rev-parse", "HEAD"]),
    headTree: git(repoRoot, ["rev-parse", "HEAD^{tree}"]),
    dirty: git(repoRoot, ["status", "--porcelain=v1"]).length > 0,
  };
}

export async function fileReceipt(path, root) {
  const bytes = await readFile(path);
  return {
    path: relative(root, path),
    bytes: bytes.length,
    sha256: sha256(bytes),
  };
}

export function assertStableExecutable(before, after) {
  if (before.bytes !== after.bytes || before.sha256 !== after.sha256) {
    throw new Error(
      `executable changed during QA: before=${before.sha256}/${before.bytes} after=${after.sha256}/${after.bytes}`,
    );
  }
  return {
    before,
    after,
    unchanged: true,
  };
}

export function assertBuiltExecutable(settings) {
  if (settings.sourceTree.dirty) {
    throw new Error("refusing executable provenance from a dirty source tree");
  }
  return {
    build: {
      command: "cargo build -p harness",
      head: settings.sourceTree.head,
      headTree: settings.sourceTree.headTree,
    },
    source: {
      path: settings.sourceBefore.path,
      ...assertStableExecutable(settings.sourceBefore, settings.sourceAfter),
    },
    testedCopy: {
      path: settings.testedBefore.path,
      ...assertStableExecutable(settings.sourceBefore, settings.testedBefore),
      execution: assertStableExecutable(settings.testedBefore, settings.testedAfter),
    },
    unchanged: true,
  };
}

function git(cwd, args) {
  return execFileSync("git", args, { cwd, encoding: "utf8" }).trim();
}

function included(path) {
  return !path.startsWith(".omo/")
    && !path.startsWith("artifacts/")
    && !path.startsWith("target/")
    && !path.includes("/node_modules/")
    && path !== "scripts/qa/node_modules";
}
