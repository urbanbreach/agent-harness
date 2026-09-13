import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { assertSecretFree, redactEvidence, safePtyEnvironment } from "./security.mjs";

export function commandRecorder({ cwd, output, timeout, env = {}, receipts = [], receiptFile = "commands.json" }) {
  let index = receipts.length;
  let receiptWrite = Promise.resolve();
  return async (label, command, args, overrides = {}) => {
    const started = new Date().toISOString();
    const logPath = `logs/${String(index++).padStart(3, "0")}-${label}.log`;
    const environment = safePtyEnvironment(process.env, {
      TERM: "xterm-256color", COLORTERM: "truecolor", ...env, ...overrides,
    });
    delete environment.NO_COLOR;
    const child = spawn(command, args, { cwd, env: environment, timeout, stdio: ["ignore", "pipe", "pipe"] });
    const chunks = [];
    child.stdout.on("data", chunk => chunks.push(chunk));
    child.stderr.on("data", chunk => chunks.push(chunk));
    const result = await new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("close", (code, signal) => resolve({ code, signal }));
    });
    const log = redactEvidence(Buffer.concat(chunks).toString("utf8"));
    const receipt = { label, command, args, overrides, started, ...result, logPath };
    assertSecretFree(log);
    assertSecretFree(receipt);
    await mkdir(join(output, "logs"), { recursive: true });
    await writeFile(join(output, logPath), log);
    receipts.push(receipt);
    // Body captures run commands concurrently; never overlap writes to the receipt file.
    receiptWrite = receiptWrite.then(() => writeFile(join(output, receiptFile), JSON.stringify(receipts, null, 2)));
    await receiptWrite;
    process.stdout.write(`${label}: exit ${result.code}\n${log.slice(-3000)}\n`);
    if (result.code !== 0) throw new Error(`${label} failed: exit ${result.code}, signal ${result.signal}; see ${join(output, logPath)}`);
  };
}
