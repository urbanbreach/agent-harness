const SCENARIOS = [
  "smoke",
  "transcript-response-navigation",
  "transcript-active-block",
  "composer-multiline-actions",
  "slash-completion-happy",
  "slash-completion-edge",
  "disconnect-truth",
  "disconnect-duplicate",
  "p1-02-modal-chrome",
  "p1-04-responsive-feedback",
];

const VALUE_OPTIONS = new Set([
  "--scenario",
  "--evidence-dir",
  "--command",
  "--input",
  "--assert",
  "--browser",
  "--cols",
  "--rows",
  "--timeout-ms",
  "--title",
  "--capability-variant",
]);

export class CliError extends Error {
  constructor(message) {
    super(message);
    this.name = "CliError";
  }
}

export function parseArgs(argv) {
  const values = { inputs: [], assertions: [] };
  for (let index = 0; index < argv.length; index += 1) {
    const option = argv[index];
    if (option === "--help") {
      values.help = true;
      continue;
    }
    if (!VALUE_OPTIONS.has(option)) throw new CliError(`unknown option: ${option}`);
    const value = argv[index + 1];
    if (!value || value.startsWith("--")) throw new CliError(`${option} requires a value`);
    index += 1;
    if (option === "--input") values.inputs.push(value);
    else if (option === "--assert") values.assertions.push(value);
    else values[toProperty(option)] = value;
  }
  if (values.help) return values;
  if (!values.scenario) throw new CliError("--scenario is required");
  if (!SCENARIOS.includes(values.scenario)) throw new CliError(`unknown scenario: ${values.scenario}`);
  if (!values.evidenceDir) throw new CliError("--evidence-dir is required");
  values.cols = positiveInteger(values.cols ?? "100", "--cols");
  values.rows = positiveInteger(values.rows ?? "30", "--rows");
  values.timeoutMs = positiveInteger(values.timeoutMs ?? "20000", "--timeout-ms");
  values.browser ??= "/usr/bin/chromium";
  values.capabilityVariant ??= "unicode";
  if (!new Set(["unicode", "basic-ascii"]).has(values.capabilityVariant)) {
    throw new CliError(`unknown capability variant: ${values.capabilityVariant}`);
  }
  return values;
}

export function scenarioContract(options, environment) {
  if (options.scenario === "smoke") return smokeContract(options);
  if (options.scenario === "composer-multiline-actions") return composerMultilineActionsContract(options);
  if (options.scenario === "slash-completion-happy") return slashCompletionHappyContract(options);
  if (options.scenario === "slash-completion-edge") return slashCompletionEdgeContract(options);
  if (options.scenario === "disconnect-truth" || options.scenario === "disconnect-duplicate") {
    return disconnectContract(options);
  }
  if (options.scenario === "p1-02-modal-chrome") return p102ModalChromeContract(options);
  if (options.scenario === "p1-04-responsive-feedback") return p104ResponsiveFeedbackContract(options);
  const stem = options.scenario.replaceAll("-", "_").toUpperCase();
  const command = options.command ?? environment[`HARNESS_QA_${stem}_COMMAND`];
  if (!command) {
    throw new CliError(
      `${options.scenario} fixture command is not available; pass --command or HARNESS_QA_${stem}_COMMAND`,
    );
  }
  const envActions = parseJsonArray(environment[`HARNESS_QA_${stem}_ACTIONS`], `${stem}_ACTIONS`);
  const envAssertions = parseJsonArray(
    environment[`HARNESS_QA_${stem}_ASSERTIONS`],
    `${stem}_ASSERTIONS`,
  );
  const actions = options.inputs.length > 0 ? options.inputs.map(parseAction) : envActions;
  const assertions = options.assertions.length > 0 ? options.assertions : envAssertions;
  if (actions.length === 0 || assertions.length === 0
    || assertions.some((value) => typeof value !== "string" || value.length === 0)) {
    throw new CliError(
      `${options.scenario} requires actions and assertions from --input/--assert or its HARNESS_QA_* contract`,
    );
  }
  return {
    name: options.scenario,
    title: options.title ?? `Harness ${options.scenario}`,
    command,
    actions: actions.map(normalizeAction),
    assertions,
    expectNaturalExit: false,
  };
}

export function helpText() {
  return `Harness browser-backed xterm.js visual QA\n\nUsage:\n  node scripts/qa/web-terminal-visual-qa.mjs --scenario smoke --evidence-dir <dir>\n  node scripts/qa/web-terminal-visual-qa.mjs --scenario p1-02-modal-chrome --evidence-dir <dir> --cols 120 --rows 40\n  node scripts/qa/web-terminal-visual-qa.mjs --scenario transcript-response-navigation --evidence-dir <dir>\n  node scripts/qa/web-terminal-visual-qa.mjs --scenario transcript-active-block --evidence-dir <dir>\n  node scripts/qa/web-terminal-visual-qa.mjs --scenario composer-multiline-actions --evidence-dir <dir>\n\nFixture override:\n  --command <shell-command> --input <action> --assert <visible-marker>\n\nActions: literal text, {Enter}, {Escape}, {PageUp}, {PageDown}, {Shift+J}, {Shift+K}, {Shift+Enter}, {Alt+Enter}, {Alt+M}, {Alt+S}, {Alt+I}, {Alt+R}, {Ctrl+Alt+Enter}, {Ctrl+P}, {Ctrl+Shift+Enter}, {Wait:text}, {WaitAbsent:text}, {WaitTitle:title}, {Click:text}, {Capture}.\nPlanned scenarios fail closed until a Harness fixture command, actions, and assertions are supplied.\n`;
}

function smokeContract(options) {
  return {
    name: "smoke",
    title: options.title ?? "Harness xterm.js PTY smoke",
    command: "harness tui --mock --deterministic --session-dir $HARNESS_QA_SESSION_DIR",
    actions: [
      { kind: "wait", value: "Demo mode" },
      { kind: "type", value: "P0-06 canonical" },
      { kind: "wait", value: "P0-06 canonical" },
      { kind: "key", value: "Control+P" },
      { kind: "wait", value: "Commands" },
      { kind: "capture" },
    ],
    assertions: ["Commands", "P0-06 canonical"],
    expectNaturalExit: false,
  };
}

function disconnectContract(options) {
  const mode = options.scenario === "disconnect-duplicate" ? "duplicate" : "truth";
  return {
    name: options.scenario,
    title: options.title ?? `Harness ${options.scenario}`,
    command: `env HARNESS_TUI_P0_05_SCENARIO=${mode} cargo test --manifest-path $HARNESS_QA_REPO_ROOT/Cargo.toml -p harness-tui --test runtime_wait_set_test -- --exact p0_05_disconnect_pty_helper --nocapture`,
    actions: [
      { kind: "wait", value: "Connection lost" },
      { kind: "wait", value: "reopen required" },
      { kind: "wait", value: "disconnect draft preserved" },
      { kind: "waitAbsent", value: "Reconnecting" },
      { kind: "waitAbsent", value: "[stop]" },
      { kind: "waitAbsent", value: "[send to bg]" },
      { kind: "assertCount", value: "Reconnecting", count: 0 },
      { kind: "assertCount", value: "[stop]", count: 0 },
      { kind: "assertCount", value: "[send to bg]", count: 0 },
      { kind: "assertCount", value: "reopen required", count: 1 },
      { kind: "capture" },
      { kind: "type", value: "attempted send" },
      { kind: "key", value: "Enter" },
      { kind: "waitAbsent", value: "attempted send" },
      { kind: "capture" },
      { kind: "key", value: "Control+Q" },
      { kind: "key", value: "Control+Q" },
    ],
    assertions: ["Connection lost", "reopen required", "disconnect draft preserved"],
    expectNaturalExit: true,
  };
}

function p102ModalChromeContract(options) {
  const popupWidth = Math.min(options.cols, 88);
  const popupHeight = Math.min(options.rows, 28);
  const popupColumn = Math.floor((options.cols - popupWidth) / 2) + 1;
  const popupRow = Math.floor((options.rows - popupHeight) / 2) + 1;
  return {
    name: "p1-02-modal-chrome",
    title: options.title ?? `Harness P1-02 modal chrome ${options.cols}x${options.rows}`,
    command: "harness tui --mock --deterministic --session-dir $HARNESS_QA_SESSION_DIR",
    actions: [
      { kind: "wait", value: "Demo mode" },
      { kind: "key", value: "Control+P" },
      { kind: "wait", value: "Commands" },
      { kind: "type", value: "settings" },
      { kind: "wait", value: "Settings" },
      { kind: "key", value: "Enter" },
      { kind: "wait", value: "Commands / Settings" },
      {
        kind: "waitFrame",
        marker: "[Runtime]  TUI",
        left: popupColumn,
        top: popupRow,
        right: popupColumn + popupWidth - 1,
        bottom: popupRow + popupHeight - 1,
      },
      { kind: "capture" },
      { kind: "key", value: "Tab" },
      { kind: "wait", value: "Runtime  [TUI]" },
      {
        kind: "waitFrame",
        marker: "Runtime  [TUI]",
        left: popupColumn,
        top: popupRow,
        right: popupColumn + popupWidth - 1,
        bottom: popupRow + popupHeight - 1,
      },
      { kind: "capture" },
      { kind: "key", value: "Shift+Tab" },
      { kind: "wait", value: "[Runtime]  TUI" },
      { kind: "key", value: "Escape" },
      { kind: "wait", value: "Commands" },
      { kind: "waitAbsent", value: "Commands / Settings" },
      { kind: "key", value: "Enter" },
      { kind: "wait", value: "Commands / Settings" },
      { kind: "mouseDown", column: 1, row: 1 },
      { kind: "mouseUp", column: popupColumn + 2, row: popupRow + 3 },
      { kind: "wait", value: "Commands / Settings" },
      { kind: "clickCell", column: popupColumn + popupWidth - 3, row: popupRow },
      { kind: "wait", value: "Commands" },
      { kind: "waitAbsent", value: "Commands / Settings" },
      { kind: "capture" },
      { kind: "key", value: "Escape" },
      { kind: "waitAbsent", value: "Commands" },
      { kind: "key", value: "Control+x" },
      { kind: "wait", value: "Keyboard Shortcuts" },
      { kind: "capture" },
      { kind: "key", value: "Escape" },
      { kind: "waitAbsent", value: "Keyboard Shortcuts" },
      { kind: "key", value: "Control+x" },
      { kind: "type", value: "m" },
      { kind: "wait", value: "Models" },
      { kind: "capture" },
      { kind: "key", value: "Escape" },
      { kind: "waitAbsent", value: "Models" },
    ],
    assertions: ["Models", "navigate", "Esc close"],
    expectNaturalExit: false,
  };
}

function p104ResponsiveFeedbackContract(options) {
  const basicAscii = options.capabilityVariant === "basic-ascii";
  const environment = basicAscii
    ? {
        TERM: "dumb",
        TERM_PROGRAM: "",
        COLORTERM: "",
        NO_COLOR: "1",
        HARNESS_TUI_REDUCED_MOTION: "1",
      }
    : {
        TERM: "xterm-256color",
        TERM_PROGRAM: "WezTerm",
        COLORTERM: "truecolor",
      };
  return {
    name: "p1-04-responsive-feedback",
    title: options.title ?? `Harness P1-04 responsive feedback ${options.capabilityVariant}`,
    command: "env HARNESS_TUI_P1_04_SCENARIO=1 cargo test --manifest-path $HARNESS_QA_REPO_ROOT/Cargo.toml -p harness-tui --test p1_04_pty_recorded -- --exact p1_04_pty_helper --nocapture",
    capabilityVariant: options.capabilityVariant,
    environment,
    classification: basicAscii
      ? { color: "no_color", glyphs: "ascii", width: "compact", motion: "reduced" }
      : { color: "true_color", glyphs: "preferred", width: "unicode11", motion: "full" },
    actions: [
      { kind: "wait", value: "P1-04 responsive ready" },
      { kind: "capture", state: "following" },
      { kind: "key", value: "PageUp" },
      { kind: "capture", state: "detached" },
      { kind: "resize", cols: 80, rows: 24 },
      { kind: "resize", cols: 160, rows: 50 },
      { kind: "resize", cols: 120, rows: 40 },
      { kind: "capture", state: "resize-final" },
      { kind: "capture", state: "reduced-motion" },
    ],
    assertions: ["Harness"],
    expectNaturalExit: false,
  };
}

function composerMultilineActionsContract(options) {
  return {
    name: "composer-multiline-actions",
    title: options.title ?? "Harness multiline composer actions",
    command: "env HARNESS_TUI_P0_04_SCENARIO=1 cargo test --manifest-path $HARNESS_QA_REPO_ROOT/Cargo.toml -p harness-tui --test p0_04_pty_recorded -- --exact p0_04_pty_helper --nocapture",
    actions: [
      { kind: "wait", value: "P0-04 active streaming" },
      { kind: "key", value: "Alt+m" },
      { kind: "wait", value: "MULTILINE" },
      { kind: "type", value: "first line" },
      { kind: "key", value: "Enter" },
      { kind: "type", value: "second line" },
      { kind: "wait", value: "Enter:newline" },
      { kind: "wait", value: "Alt+s:send" },
      { kind: "wait", value: "Alt+i:interject" },
      { kind: "wait", value: "Alt+r:replace" },
      { kind: "capture" },
      { kind: "key", value: "Alt+s" },
      { kind: "waitCount", value: "QUEUED", count: 1 },
      { kind: "type", value: "interject text" },
      { kind: "key", value: "Alt+i" },
      { kind: "waitCount", value: "QUEUED", count: 2 },
      { kind: "type", value: "replacement text" },
      { kind: "key", value: "Alt+r" },
      { kind: "waitCount", value: "QUEUED", count: 3 },
      { kind: "capture" },
      { kind: "key", value: "Control+Q" },
      { kind: "key", value: "Control+Q" },
    ],
    assertions: ["MULTILINE", "QUEUED", "second line", "interject text", "replacement text"],
    expectNaturalExit: true,
  };
}

function slashCompletionCommand() {
  return "env HARNESS_TUI_P1_01_SCENARIO=1 cargo test --manifest-path $HARNESS_QA_REPO_ROOT/Cargo.toml -p harness-tui --test p1_01_pty_recorded -- --exact p1_01_pty_helper --nocapture";
}

function slashCompletionHappyContract(options) {
  const renamedTitle = "P1-01 renamed:Parity Title";
  return {
    name: "slash-completion-happy",
    title: options.title ?? "Harness slash completion happy path",
    command: slashCompletionCommand(),
    actions: [
      { kind: "wait", value: "P1-01 slash ready" },
      { kind: "type", value: "draft /ren" },
      { kind: "wait", value: "/rename" },
      { kind: "capture" },
      { kind: "key", value: "Tab" },
      { kind: "wait", value: "argument required" },
      { kind: "capture" },
      { kind: "type", value: "Parity Title" },
      { kind: "key", value: "Enter" },
      { kind: "waitTitle", value: renamedTitle },
      { kind: "assertTitleCount", value: renamedTitle, count: 1 },
      { kind: "wait", value: "draft" },
      { kind: "capture" },
    ],
    assertions: ["P1-01 slash ready", "draft"],
    expectNaturalExit: false,
  };
}

function slashCompletionEdgeContract(options) {
  return {
    name: "slash-completion-edge",
    title: options.title ?? "Harness slash completion edge path",
    command: slashCompletionCommand(),
    actions: [
      { kind: "wait", value: "P1-01 slash ready" },
      { kind: "type", value: "/rename" },
      { kind: "key", value: "Tab" },
      { kind: "wait", value: "argument required" },
      { kind: "key", value: "Enter" },
      { kind: "wait", value: "argument required" },
      { kind: "waitAbsent", value: "session title cannot be empty" },
      { kind: "capture" },
      { kind: "key", value: "Escape" },
      { kind: "type", value: "https://example.com \\/help /名" },
      { kind: "wait", value: "No matching items" },
      { kind: "capture" },
    ],
    assertions: ["P1-01 slash ready", "https://example.com", "\\/help", "No matching items"],
    expectNaturalExit: false,
  };
}

function parseAction(value) {
  const structured = /^\{(WaitTitle|WaitAbsent|Wait|Click):(.+)\}$/.exec(value);
  if (structured) {
    const kind = structured[1] === "WaitTitle"
      ? "waitTitle"
      : structured[1] === "WaitAbsent" ? "waitAbsent" : structured[1].toLowerCase();
    return { kind, value: structured[2] };
  }
  if (value === "{Capture}") return { kind: "capture" };
  const keys = new Map([
    ["{Enter}", "Enter"], ["{Escape}", "Escape"], ["{Tab}", "Tab"],
    ["{PageUp}", "PageUp"], ["{PageDown}", "PageDown"],
    ["{Shift+J}", "Shift+J"], ["{Shift+K}", "Shift+K"],
    ["{Shift+Enter}", "Shift+Enter"], ["{Alt+Enter}", "Alt+Enter"],
    ["{Alt+M}", "Alt+m"], ["{Alt+S}", "Alt+s"],
    ["{Alt+I}", "Alt+i"], ["{Alt+R}", "Alt+r"],
    ["{Ctrl+Alt+Enter}", "Control+Alt+Enter"],
    ["{Ctrl+Shift+Enter}", "Control+Shift+Enter"],
    ["{Up}", "ArrowUp"], ["{Down}", "ArrowDown"],
    ["{Left}", "ArrowLeft"], ["{Right}", "ArrowRight"],
  ]);
  const named = keys.get(value);
  if (named) return { kind: "key", value: named };
  const control = /^\{Ctrl\+([A-Za-z])\}$/.exec(value);
  if (control) return { kind: "key", value: `Control+${control[1].toUpperCase()}` };
  if (value.startsWith("{") && value.endsWith("}")) throw new CliError(`unknown action: ${value}`);
  return { kind: "type", value };
}

function normalizeAction(value) {
  if (typeof value === "string") return parseAction(value);
  if (!value || typeof value !== "object" || !["wait", "waitAbsent", "waitTitle", "waitCount", "assertCount", "assertTitleCount", "type", "key", "click", "clickCell", "mouseDown", "mouseUp", "resize", "capture"].includes(value.kind)) {
    throw new CliError("invalid fixture action");
  }
  if (value.kind === "resize") {
    if (!Number.isSafeInteger(value.cols) || value.cols <= 0
      || !Number.isSafeInteger(value.rows) || value.rows <= 0) {
      throw new CliError("fixture action resize requires positive cols and rows");
    }
  } else if (["clickCell", "mouseDown", "mouseUp"].includes(value.kind)) {
    if (!Number.isSafeInteger(value.column) || value.column <= 0
      || !Number.isSafeInteger(value.row) || value.row <= 0) {
      throw new CliError(`fixture action ${value.kind} requires positive cell coordinates`);
    }
  } else if (value.kind !== "capture" && typeof value.value !== "string") {
    throw new CliError(`fixture action ${value.kind} requires a string value`);
  }
  if ((value.kind === "waitCount" || value.kind === "assertCount" || value.kind === "assertTitleCount")
    && (!Number.isSafeInteger(value.count) || value.count < 0)) {
    throw new CliError(`fixture action ${value.kind} requires a non-negative integer count`);
  }
  return value;
}

function parseJsonArray(value, name) {
  if (!value) return [];
  try {
    const parsed = JSON.parse(value);
    if (!Array.isArray(parsed)) throw new CliError(`${name} must be a JSON array`);
    return parsed;
  } catch (error) {
    if (error instanceof CliError) throw error;
    throw new CliError(`${name} is not valid JSON`);
  }
}

function positiveInteger(value, option) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new CliError(`${option} must be positive`);
  return parsed;
}

function toProperty(option) {
  return option.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
}
