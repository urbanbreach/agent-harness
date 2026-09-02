import { lstat } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { basename, dirname, relative, resolve } from "node:path";

const SAFE_ENV_KEYS = [
  "PATH",
  "HOME",
  "USER",
  "LOGNAME",
  "SHELL",
  "TMPDIR",
  "TEMP",
  "TMP",
  "CARGO_HOME",
  "RUSTUP_HOME",
  "XDG_CACHE_HOME",
  "LANG",
  "LANGUAGE",
  "LC_ALL",
  "LC_CTYPE",
  "COLORTERM",
  "NO_COLOR",
  "FORCE_COLOR",
  "CI",
];

const PRIVATE_KEY = /-----BEGIN [^-]*(?:PRIVATE KEY|OPENSSH PRIVATE KEY)-----[\s\S]*?-----END [^-]*(?:PRIVATE KEY|OPENSSH PRIVATE KEY)-----/gi;
const AUTHORIZATION = /(authorization\s*:\s*bearer\s+)([^\s"',}]+)/gi;
const COOKIE_HEADER = /((?:set-)?cookie\s*:\s*)([^\r\n]+)/gi;
const CREDENTIAL = /\b([A-Z0-9_]*(?:API[_-]?KEY|TOKEN|SECRET|PASSWORD|PASSWD|COOKIE)[A-Z0-9_]*)\s*=\s*("[^"]*"|'[^']*'|[^\s,;]+)/gi;
const OPENAI_TOKEN = /\bsk-[A-Za-z0-9_-]{20,}\b/g;
const GITHUB_TOKEN = /\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})\b/g;
const AWS_ACCESS_KEY = /\b(?:AKIA|ASIA)[A-Z0-9]{16}\b/g;

export async function validateEvidenceDir(input, repoRoot) {
  const candidate = resolve(input);
  const repository = resolve(repoRoot);
  const repositoryEvidence = resolve(repository, ".omo", "evidence");
  const temporaryRoot = resolve(tmpdir());
  const insideRepositoryEvidence = isStrictDescendant(candidate, repositoryEvidence);
  const dedicatedTemporary = dirname(candidate) === temporaryRoot
    && basename(candidate).startsWith("harness-xterm-");
  const protectedPaths = new Set(["/", resolve(homedir()), repository, repositoryEvidence, temporaryRoot]);
  if (protectedPaths.has(candidate) || (!insideRepositoryEvidence && !dedicatedTemporary)) {
    throw new Error(`unsafe evidence directory: ${input}`);
  }
  await rejectSymlinkComponents(candidate, insideRepositoryEvidence ? repositoryEvidence : temporaryRoot);
  return candidate;
}

export function safePtyEnvironment(host, overrides = {}) {
  const environment = {};
  for (const key of SAFE_ENV_KEYS) {
    if (host[key] !== undefined) environment[key] = host[key];
  }
  return { ...environment, ...overrides };
}

export function redactEvidence(value) {
  if (Buffer.isBuffer(value)) return Buffer.from(redactText(value.toString("utf8")));
  if (typeof value === "string") return redactText(value);
  if (Array.isArray(value)) return value.map(redactEvidence);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, entry]) => [key, redactEvidence(entry)]));
  }
  return value;
}

export function assertSecretFree(value) {
  const text = Buffer.isBuffer(value)
    ? value.toString("utf8")
    : typeof value === "string"
      ? value
      : JSON.stringify(value);
  const residual = [
    PRIVATE_KEY,
    AUTHORIZATION,
    COOKIE_HEADER,
    CREDENTIAL,
    OPENAI_TOKEN,
    GITHUB_TOKEN,
    AWS_ACCESS_KEY,
  ].some((pattern) => {
    pattern.lastIndex = 0;
    return Array.from(text.matchAll(pattern)).some((match) => !match[0].includes("[REDACTED]"));
  });
  if (residual) throw new Error("secret scan rejected durable xterm QA evidence");
}

export function assertScreenshotSafe(snapshot) {
  assertSecretFree(snapshot?.text ?? "");
}

function redactText(value) {
  return value
    .replace(PRIVATE_KEY, "[REDACTED PRIVATE KEY]")
    .replace(AUTHORIZATION, "$1[REDACTED]")
    .replace(COOKIE_HEADER, "$1[REDACTED]")
    .replace(CREDENTIAL, "$1=[REDACTED]")
    .replace(OPENAI_TOKEN, "[REDACTED TOKEN]")
    .replace(GITHUB_TOKEN, "[REDACTED TOKEN]")
    .replace(AWS_ACCESS_KEY, "[REDACTED TOKEN]");
}

function isStrictDescendant(candidate, root) {
  const path = relative(root, candidate);
  return path !== "" && path !== ".." && !path.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`);
}

async function rejectSymlinkComponents(candidate, root) {
  let current = candidate;
  while (true) {
    try {
      if ((await lstat(current)).isSymbolicLink()) {
        throw new Error(`unsafe evidence directory: symlink component ${current}`);
      }
    } catch (error) {
      if (!(error instanceof Error && "code" in error && error.code === "ENOENT")) throw error;
    }
    if (current === root) break;
    const parent = dirname(current);
    if (parent === current) throw new Error(`unsafe evidence directory: ${candidate}`);
    current = parent;
  }
}
