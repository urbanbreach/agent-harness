import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { parseArgs, scenarioContract } from "./lib/config.mjs";

const variants = [
  {
    name: "unicode",
    expectedEnvironment: {
      TERM: "xterm-256color",
      TERM_PROGRAM: "WezTerm",
      COLORTERM: "truecolor",
    },
    expectedClassification: {
      color: "true_color",
      glyphs: "preferred",
      width: "unicode11",
      motion: "full",
    },
  },
  {
    name: "basic-ascii",
    expectedEnvironment: {
      TERM: "dumb",
      TERM_PROGRAM: "",
      COLORTERM: "",
      NO_COLOR: "1",
      HARNESS_TUI_REDUCED_MOTION: "1",
    },
    expectedClassification: {
      color: "no_color",
      glyphs: "ascii",
      width: "compact",
      motion: "reduced",
    },
  },
];

test("P1-03 contract binds the startup reveal to the built owner with staged captures", () => {
  for (const variant of variants) {
    const options = parseArgs([
      "--scenario", "p1-03-startup-reveal",
      "--evidence-dir", "/tmp/evidence",
      "--capability-variant", variant.name,
      "--cols", "120",
      "--rows", "40",
    ]);
    const contract = scenarioContract(options, {});

    assert.deepEqual(contract.binaryTarget, {
      package: "harness-tui",
      test: "p1_03_pty_recorded",
    });
    assert.match(contract.command, /HARNESS_QA_TEST_BINARY/);
    assert.match(contract.command, /HARNESS_TUI_P1_03_SCENARIO=1/);
    assert.match(contract.command, /--exact p1_03_pty_helper/);
    assert.equal(contract.capabilityVariant, variant.name);
    assert.deepEqual(contract.environment, variant.expectedEnvironment);
    assert.deepEqual(contract.classification, variant.expectedClassification);
    assert.deepEqual(
      contract.actions.filter(({ kind }) => kind === "capture").map(({ state }) => state),
      ["welcome-complete", "after-input"],
    );
    assert.ok(contract.actions.some(({ kind, value }) => kind === "wait" && value === "Subagent spawning"));
    assert.ok(
      contract.actions.some(({ kind, value }) => kind === "type" && value === "draft during reveal"),
    );
    assert.ok(
      contract.actions.some(({ kind, value }) => kind === "waitAbsent" && value === "New worktree"),
    );
    assert.deepEqual(contract.assertions, ["draft during reveal", "Enter:send"]);
    assert.equal(contract.expectNaturalExit, false);
  }
});
