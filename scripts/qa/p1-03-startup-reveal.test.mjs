import assert from "node:assert/strict";
import { parseArgs, scenarioContract } from "./lib/config.mjs";
import test from "node:test";

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
    expectedCaptures: ["welcome-complete", "after-input"],
    expectedDraft: "draft 川山 during reveal",
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
    expectedCaptures: ["early-input"],
    expectedDraft: "draft during reveal",
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
      variant.expectedCaptures,
    );
    assert.ok(contract.actions.some(({ kind, value }) => kind === "wait" && value === "Beta"));
    assert.ok(
      contract.actions.some(({ kind, value }) => kind === "waitAbsent" && value === "New worktree"),
    );
    assert.ok(
      contract.actions.some(({ kind, value }) => kind === "type" && value === variant.expectedDraft),
    );
    assert.deepEqual(contract.assertions, [variant.expectedDraft, "Enter:send"]);
    assert.equal(contract.expectNaturalExit, false);

    if (variant.name === "unicode") {
      assert.ok(
        contract.actions.some(({ kind, value }) => kind === "wait" && value === "Subagent spawning"),
      );
    } else {
      const typeIndex = contract.actions.findIndex(({ kind }) => kind === "type");
      const waitIndex = contract.actions.findIndex(
        ({ kind, value }) => kind === "wait" && value === "Enter:send",
      );
      assert.ok(
        typeIndex > -1 && waitIndex > typeIndex,
        "basic-ascii must type the draft before waiting for the composer echo",
      );
    }
  }
});
