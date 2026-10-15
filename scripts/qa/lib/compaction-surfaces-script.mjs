// Fixed-clock script for the compaction surfaces: one hand-authored conversation whose typed
// durable and live events, key presses and command outcomes drive the real AppState renderer.
// The numbers are visual fixtures, not measured sizes.
const AGENT = "fixture";
const WINDOW = 200_000;
const THRESHOLD = 160_000;
const SPECULATION = 140_000;
const SUMMARY = [
  "## Goal", "Ship the sync worker retry policy.", "## Done", "- [x] Backoff with full jitter and a thirty second cap.",
  "- [x] Tests for the cap and the jitter window.", "## Next steps", "1. Wire the counter, document the cap, run the soak test.",
].join("\n");

export function buildSurfaceScript() {
  const steps = [];
  const frames = [];
  let clock = 0;
  let seq = 0;
  let liveSeq = 0;
  const wait = ms => { clock += ms; };
  const envelope = (delivery, kind, data, { who = "worker", correlation = "setup" } = {}) => {
    const id = delivery === "durable" ? ++seq : ++liveSeq;
    const body = {
      event_id: `surface-${delivery}-${id}`, run_id: "surface-capture", mono_ms: clock,
      actor: { kind: who, agent_id: AGENT }, correlation_id: correlation, payload: { event_type: kind, data },
    };
    if (delivery === "durable") Object.assign(body, { schema_version: 1, seq: id });
    return { delivery, event: body };
  };
  const durable = (kind, data, options) => {
    const item = envelope("durable", kind, data, options);
    steps.push({ at_ms: clock, event: item });
    return item.event.seq;
  };
  const live = (kind, data) => steps.push({ at_ms: clock, event: envelope("live", kind, data) });
  const key = chord => steps.push({ at_ms: clock, key: chord });
  const type = text => steps.push({ at_ms: clock, text });
  const outcome = (kind, result, extra = {}) => steps.push({ at_ms: clock, outcome: { kind, result, ...extra } });
  const frame = (name, expect, absent = []) => {
    const frame_id = frames.length;
    frames.push({ name, at_ms: clock, expect, absent });
    steps.push({ at_ms: clock, frame_id });
  };
  const budget = used => ({
    status: "estimated", requested_output_tokens: null, reserved_output_tokens: null, maximum_input_tokens: WINDOW,
    safety_margin_tokens: 0, compaction_threshold_tokens: THRESHOLD,
    components: { system_tokens: 4000, tools_tokens: 6000, history_tokens: used - 10_000, attachments_tokens: 0, framing_tokens: 0, pending_prompt_tokens: 0 },
    occupied_input_tokens: used, remaining_input_tokens: Math.max(0, THRESHOLD - used), requires_compaction: used >= THRESHOLD,
    output_cap_disposition: "unspecified_unknown_limit",
  });
  const status = (state, extra = {}) => live("compaction_status", { agent_id: AGENT, auto: true, threshold_tokens: THRESHOLD, speculation_tokens: SPECULATION, speculation: state, ...extra });
  let turns = 0;
  const turn = (prompt, answer, used) => {
    const id = `turn-${++turns}`;
    const first = durable("user_message_submitted", { request_id: id, text: prompt }, { who: "user", correlation: id });
    durable("task_scheduled", { task_id: id, state: "queued", queue_key: AGENT, metadata: null }, { who: "user", correlation: id });
    durable("task_scheduled", { task_id: id, state: "started", queue_key: AGENT, metadata: null }, { correlation: id });
    wait(60);
    durable("provider_request_started", { request_id: `provider-${turns}`, provider_id: "mock", model_id: "default", prompt_summary: prompt, request_digest: `digest-${turns}`, metadata: { context_budget: budget(used) } }, { correlation: id });
    wait(60);
    durable("provider_request_finished", { request_id: `provider-${turns}`, finish_reason: "stop" }, { correlation: id });
    durable("assistant_message_finished", { request_id: `provider-${turns}`, parts: [{ kind: "text", text: answer }], tool_call_count: 0 }, { correlation: id });
    durable("task_completed", { task_id: id, result_summary: answer, result_digest: `result-${turns}`, metadata: { lineage: null, task_scope: "agent_turn", timing: null, hook_executions: [] } }, { correlation: id });
    wait(120);
    return first;
  };
  const stage = (reason, method, phase, before, extra = {}) => ({
    reason, method, phase, tokens_before: before, tokens_after: phase === "done" ? (before === 120_000 ? 24_000 : 31_000) : null, summarized_tokens: before - 31_000, kept_tokens: 20_000,
    fixed_tokens: 10_000, threshold_tokens: THRESHOLD, window_tokens: WINDOW, first_kept_seq: 100, windows_done: phase === "preparing" ? 0 : 1, windows_total: 2,
    frames_done: 0, frames_total: 0, thumbnails: [], message: null, ...extra,
  });
  const progress = (generation, reason, phase, preview, before = 184_000) => live("compaction_progress", { agent_id: AGENT, generation, trigger_reason: reason, preview, stage: stage(reason, "soft", phase, before) });
  const clearProgress = (generation, reason) => live("compaction_progress", { agent_id: AGENT, generation, trigger_reason: reason, preview: null, stage: null });
  const lifecycle = (operation_id, phase, reason, action, extra = {}) => live("compaction_lifecycle", {
    agent_id: AGENT, operation_id, phase, reason, action, will_retry: false, aborted: false, skipped: false, error: null, result: null, ...extra,
  });
  const result = (tokens_before, tokens_after, summary) => ({ method: "soft", tokens_before, tokens_after, short_summary: summary, warning: null, first_kept_seq: null });
  const compaction = (operation_id, firstKept, reason, extra = {}) => durable("session_compaction", {
    agent_id: AGENT, summary: SUMMARY, first_kept_event_seq: firstKept, first_kept_request_id: null, first_kept_entry_id: null, tokens_before: 184_000, tokens_after: 31_000,
    summary_usage: null, summary_provider_id: null, summary_model_id: null, read_files: ["src/sync/backoff.rs"], modified_files: ["src/sync/worker.rs"], task_intent: null,
    current_intent: null, trigger_reason: reason, from_hook: false, short_summary: "Retry policy shipped except metrics and soak test", method: "soft", warning: null,
    provider_replay_through_seq: null, native: null, archive: null, operation_id, ...extra,
  });

  // Footer context status at normal, warning, elevated and error severity, with the speculation lead.
  wait(100);
  status("idle");
  turn("Sketch the retry policy for the sync worker.", "First sketch: exponential backoff from 200 ms, doubling to a thirty second cap, three attempts per batch.", 60_000);
  frame("footer-normal", ["30.0%/200K", "Sketch the retry policy"]);
  turn("Add jitter and a cap to the backoff.", "Added full jitter and a thirty second ceiling: sleep is min(cap, random(0, base * 2^n)).", 110_000);
  frame("footer-warning", ["55.0%/200K"]);
  turn("Which tests cover the cap?", "Two tests cover it: backoff_respects_cap and jitter_stays_inside_window.", 150_000);
  status("armed");
  wait(100);
  frame("footer-armed", ["75.0%/200K"]);
  status("running");
  wait(100);
  frame("footer-speculation-running-a", ["75.0%/200K"]);
  wait(500);
  frame("footer-speculation-running-b", ["75.0%/200K"]);
  status("idle");
  const latest = turn("Summarize what is left before we ship.", "Remaining work: wire the metrics counter, document the cap, run the soak test.", 184_000);
  frame("footer-error", ["92.0%/200K", "Summarize what is left"]);

  // Automatic pass: lifecycle loader, stop control, and queued input in the pending band.
  wait(300);
  lifecycle("op-auto", "start", "threshold", "context-full");
  progress(1, "threshold", "preparing", "Preparing the older turns.");
  wait(400);
  progress(1, "threshold", "summarizing", "Keep the backoff decisions and the open tasks.");
  wait(600);
  frame("lifecycle-loader-and-stop", ["Auto context-full maintenance", "[stop]"]);
  progress(1, "threshold", "committing", null);
  wait(500);
  live("runtime_notice", { source: "compaction", severity: "info", message: "Summarizing 153,000 tokens of older history." });
  live("runtime_notice", { source: "compaction", severity: "info", message: "Kept the latest turn verbatim; summary is ready." });
  compaction("op-auto", latest, "threshold");
  progress(1, "threshold", "done", null);
  clearProgress(1, "threshold");
  lifecycle("op-auto", "end", "threshold", "context-full", { result: result(184_000, 31_000, "Retry policy shipped except metrics and soak test") });
  wait(1600);
  frame("end-notice-coalesced-status", ["Kept the latest turn verbatim", "15.5%/200K"], ["Summarizing 153,000 tokens"]);

  // Collapsed history by default, the full chronological transcript after Ctrl+Alt+K.
  frame("history-collapsed-default", ["soft-compacted · 184K→31K", "Summarize what is left"], ["Sketch the retry policy", "Add jitter and a cap"]);
  key("ctrl+alt+k");
  wait(200);
  frame("history-full-after-toggle", ["soft-compacted · 184K→31K", "Compacted history shown"]);
  key("pageup");
  wait(200);
  frame("history-full-scrolled-to-first-turns", ["Sketch the retry policy", "Add jitter and a cap"]);
  key("ctrl+alt+k");
  wait(200);
  frame("history-collapsed-again", ["Compacted history collapsed", "Summarize what is left"], ["Sketch the retry policy"]);
  key("ctrl+alt+o");
  wait(200);
  frame("summary-card-expanded", ["Compacted from 184,000 to 31,000 tokens", "Ship the sync worker retry policy.", "Next steps"]);
  key("ctrl+alt+o");

  // A failed automatic pass leaves a warning row, then a runtime warning from the summarizer.
  wait(300);
  lifecycle("op-fail", "start", "threshold", "context-full");
  wait(300);
  live("runtime_notice", { source: "compaction", severity: "warning", message: "Summary model returned a partial answer; keeping the previous summary." });
  lifecycle("op-fail", "end", "threshold", "context-full", { error: "Auto-compaction failed: summary rate limited" });
  wait(300);
  frame("runtime-notice-and-failed-pass", ["Summary model returned a partial answer", "summary rate limited"]);

  // Manual command: loader with stop, then done, warning and cancelled outcomes.
  wait(300);
  const second = turn("Add the metrics counter next.", "Added sync_retry_total with a reason label; the counter increments before each sleep.", 120_000);
  lifecycle("op-manual", "start", "manual", "context-full");
  progress(2, "manual", "summarizing", "Keep the counter work and the open tasks.", 120_000);
  wait(500);
  frame("manual-loader-and-stop", ["Compacting context", "[stop]"]);
  compaction("op-manual", second, "manual", { tokens_before: 120_000, tokens_after: 24_000, short_summary: "Counter added; docs and soak test remain" });
  progress(2, "manual", "done", null, 120_000);
  clearProgress(2, "manual");
  lifecycle("op-manual", "end", "manual", "context-full", { result: result(120_000, 24_000, "Counter added; docs and soak test remain") });
  outcome("compact", "done", { message: "manual compaction applied", tokens_after: 24_000 });
  wait(1600);
  frame("manual-outcome-done", ["manual compaction applied"]);
  key("ctrl+alt+k");
  wait(200);
  for (let page = 0; page < 2; page += 1) key("pageup");
  wait(200);
  key("ctrl+alt+o");
  key("pagedown");
  wait(200);
  frame("superseded-summary-card-in-full-history", ["[Superseded compaction summary elided after a newer compaction]", "Compacted history shown"]);
  key("ctrl+alt+o");
  key("ctrl+alt+k");
  wait(200);
  outcome("compact", "warning", { message: "Nothing to compact (session too small or already rolled over)" });
  frame("manual-outcome-warning", ["Warning: Nothing to compact"]);
  lifecycle("op-cancel", "start", "manual", "context-full");
  progress(3, "manual", "summarizing", "Keep the counter work and the open tasks.", 120_000);
  wait(300);
  frame("manual-stop-before-cancel", ["Compacting context", "[stop]"]);
  progress(3, "manual", "cancelled", null, 120_000);
  wait(100);
  clearProgress(3, "manual");
  lifecycle("op-cancel", "end", "manual", "context-full", { aborted: true });
  outcome("compact", "cancelled");
  wait(300);
  frame("manual-outcome-cancelled", ["Error: Compaction cancelled"]);

  // Notes-backed window rollover: the divider and card carry the rollover notice.
  wait(300);
  const third = turn("Document the cap in the runbook.", "Documented: the cap is thirty seconds and the jitter is full, so no two workers align.", 150_000);
  lifecycle("op-roll", "start", "threshold", "new-context");
  wait(300);
  compaction("op-roll", third, "threshold", { summary: "The context window rolled over.", details: { kind: "notes_rollover", version: 1 }, tokens_before: 150_000, tokens_after: 18_000, short_summary: null });
  live("runtime_notice", { source: "compaction", severity: "info", message: "Context window rolled over; the notebook carries the work forward." });
  lifecycle("op-roll", "end", "threshold", "new-context", { result: result(150_000, 18_000, null) });
  wait(1600);
  frame("notes-rollover-divider-and-notice", ["Context window rolled over; the notebook carries the work forward."]);
  key("ctrl+alt+o");
  wait(200);
  frame("notes-rollover-card-expanded", ["The context window rolled over, and the notes carry the work forward into the new window."]);
  key("ctrl+alt+o");

  // Threshold editor in the model switcher.
  wait(300);
  type("/model");
  key("enter");
  wait(200);
  frame("model-switcher", ["Models", "small", "Ctrl+T threshold"]);
  key("ctrl+t");
  wait(200);
  frame("threshold-editor-empty", ["Compaction threshold for", "Model window: 200K tokens"]);
  type("f150000");
  wait(200);
  frame("threshold-editor-fixed-tokens", ["f150000", "Compacts at"]);
  type("x");
  wait(200);
  frame("threshold-editor-invalid-input", ["f150000x"]);
  key("esc");
  key("esc");

  // Queued input while a second automatic pass runs: Steering first, After yield second.
  wait(300);
  turn("Run the soak test now.", "Soak test queued: twelve hours at the thirty second cap with injected failures.", 172_000);
  lifecycle("op-queue", "start", "threshold", "context-full");
  progress(4, "threshold", "summarizing", "Keep the soak test plan and the open tasks.", 172_000);
  wait(600);
  type("Also check the jitter bounds.");
  key("enter");
  type("Update the changelog after that.");
  key("alt+i");
  wait(100);
  frame("pending-band-steering-then-after-yield", ["Steering", "After yield", "Also check the jitter bounds.", "Update the changelog after that.", "[stop]"]);
  return { steps, frames };
}
