#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { safePtyEnvironment, validateEvidenceDir } from "./lib/security.mjs";
import { currentTree, fileReceipt } from "./lib/provenance.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const output = await validateEvidenceDir(process.argv[2], root);
// An optional standalone runner accepts: workload JSON, output JSON, conformance JSON.
const reference = process.argv[3] ? resolve(process.argv[3]) : undefined;
await mkdir(output, { recursive: true });
const cases = join(root, "scripts/qa/fixtures/eval-performance.json");
const conformance = join(root, "scripts/qa/fixtures/eval-conformance.json");
const sourceTree = await currentTree(root);
const run = (command, args, extra = {}) => execFileSync(command, args, {
  cwd: root, stdio: "inherit", env: safePtyEnvironment(process.env, { ...(process.env.GEM_PATH ? {GEM_PATH:process.env.GEM_PATH} : {}), ...extra }),
});
const capture = (command, args) => execFileSync(command, args, { encoding: "utf8" }).trim();
const measurements = new Map();
async function sample(name) {
  const destination = join(output, name + ".json");
  if (name === "harness-native") {
    run("cargo", ["nextest", "run", "--profile", "perf", "--release", "-p", "harness-eval", "--test", "performance", "--run-ignored", "all"],
      { HARNESS_EVAL_PERF_OUTPUT: destination });
  } else {
    run(name === "reference-bun" ? "bun" : "node", [reference, cases, destination, conformance]);
  }
  const rows = JSON.parse(await readFile(destination, "utf8"));
  const previous = measurements.get(name);
  if (previous) rows.forEach((row, i) => {
    if (row.name !== previous[i].name) throw new Error("benchmark cases changed between samples");
    row.ms.unshift(...previous[i].ms);
  });
  measurements.set(name, rows);
}
const listing = JSON.parse(capture("cargo", ["nextest", "list", "--release", "-p", "harness-eval", "--test", "performance", "--message-format", "json", "--list-type", "binaries-only"]));
const binary = Object.values(listing["rust-binaries"]).find(entry => entry["binary-name"] === "performance")?.["binary-path"];
if (!binary) throw new Error("eval benchmark binary missing");
const order = reference
  ? ["reference-node", "reference-bun", "harness-native", "harness-native", "reference-bun", "reference-node"]
  : ["harness-native", "harness-native"];
for (const name of order) await sample(name);
const report = {};
for (const [name, rows] of measurements) {
  await writeFile(join(output, name + ".json"), JSON.stringify(rows, null, 2));
  report[name] = rows.map(({name, ms}) => {
    if (!ms.length || ms.some(value => !Number.isFinite(value) || value < 0)) throw new Error("invalid benchmark sample");
    const sorted = [...ms].sort((a,b)=>a-b);
    return {name, samples:ms.length, median_ms:sorted[Math.floor(sorted.length/2)], p95_ms:sorted[Math.floor(sorted.length*0.95)]};
  });
}
if (reference) {
  report.comparison = report["harness-native"].map((row, i) => ({
    name: row.name, native_ms: row.median_ms,
    node_ratio: row.median_ms / report["reference-node"][i].median_ms,
    bun_ratio: row.median_ms / report["reference-bun"][i].median_ms,
  }));
  report.performance = Object.fromEntries(["node", "bun"].map(engine => {
    const ratio = Math.exp(report.comparison.reduce((sum, row) => sum + Math.log(row[engine + "_ratio"]), 0) / report.comparison.length);
    return [engine, {geometric_mean_ratio:ratio, pass:ratio <= 1}];
  }));
}
report.conformance = {};
for (const name of measurements.keys()) report.conformance[name] = JSON.parse(await readFile(join(output,name+".json.conformance.json"),"utf8"));
report.conformance.differences = report.conformance["harness-native"].flatMap((row,i)=>{
  const shouldError = /^(js|py|rb|jl)-error$/.test(row.name);
  if (row.error !== shouldError) return [{native:row,expected_error:shouldError}];
  return reference ? ["reference-node","reference-bun"].flatMap(engine=>{
    const expected = report.conformance[engine][i];
    return row.name === expected.name && row.error === expected.error && (row.error || row.text === expected.text) ? [] : [{engine,native:row,reference:expected}];
  }) : [];
});
report.provenance = {
  node:process.version, ...(reference ? {bun:capture("bun", ["--version"]), reference_runner:await fileReceipt(reference,root)} : {}), platform:process.platform, arch:process.arch,
  source:sourceTree, cases:await fileReceipt(cases,root),
  eval_binary:await fileReceipt(binary,root), node_binary:await fileReceipt(process.execPath,root),
  method:`${reference ? "ABC-CBA order against the supplied runner on Node and Bun." : "Two native runs; no reference comparison."} Each process takes 31 warm samples after 3 warmups per case (62 combined), plus five cold/reset samples per language (10 combined). The native Rust Session uses the installed Node.js worker. Timings include the public eval lifecycle and output retention with immediate host callbacks. Coordinator and full application overhead are outside this engine measurement.`,
};
const finalTree = await currentTree(root);
if (sourceTree.hash !== finalTree.hash) throw new Error("Source changed during the benchmark; rerun on the final tree");
report.provenance.source_unchanged = true;
await writeFile(join(output, "report.json"), JSON.stringify(report, null, 2));
process.stdout.write(JSON.stringify(report, null, 2)+"\n");
if (report.conformance.differences.length) throw new Error("Conformance differences; see report.json");
if (report.performance && Object.values(report.performance).some(result => !result.pass)) throw new Error("Native eval exceeds the reference suite's geometric mean latency; see report.json");
