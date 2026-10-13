//! Wait card: the background jobs a `wait_commands_or_subagents` call blocks on.
use super::*;
use crate::app::{OrchestrationTaskState, ToolCallDisplayStatus, ToolCallEntry};
use harness_core::subagent::GetCommandOrSubagentOutputValue;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use serde::Deserialize;

pub(super) const WAIT_TOOL_ID: &str = "wait_commands_or_subagents";
const COLLAPSED_JOBS: usize = 8;
const LABEL_MAX_WIDTH: usize = 60;
const PREVIEW_LINE_WIDTH: usize = 80;

/// Declared in display order: pending work first, then problems, then successes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum JobStatus {
    Running,
    Failed,
    Cancelled,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui) struct WaitJob {
    id: String,
    kind: &'static str,
    status: JobStatus,
    label: String,
    duration_ms: u64,
    exit_code: Option<i32>,
    preview: String,
    /// Opens the job's subagent session when clicked.
    link: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui) struct WaitJobs {
    jobs: Vec<WaitJob>,
    /// Spinner frame for running rows while the wait still blocks.
    spinner: Option<&'static str>,
    /// Subagent row under the pointer.
    hovered: Option<String>,
}

/// Title the card by job counts and list one row per awaited job.
pub(super) fn refresh(row: &mut TranscriptToolCallSection, tool: &ToolCallEntry, app: &AppState) {
    if tool.effective_tool_id() != WAIT_TOOL_ID {
        return;
    }
    row.header.title = "Wait".into();
    if tool.status == ToolCallDisplayStatus::Failed {
        return;
    }
    let mut jobs = jobs(tool, app);
    // Replay keeps task rows read-only; links follow the same rule.
    if app.replay_mode {
        jobs.iter_mut().for_each(|job| job.link = false);
    }
    if jobs.is_empty() {
        if tool.output_json.is_some() {
            row.header.subtitle = Some("No jobs to process".into());
        }
        return;
    }
    jobs.sort_by_key(|job| (job.status, std::cmp::Reverse(job.duration_ms)));
    let count = |status| jobs.iter().filter(|job| job.status == status).count();
    let noun = if jobs.len() == 1 { "job" } else { "jobs" };
    row.header.title = match count(JobStatus::Running) {
        0 => format!("{} {noun} settled", jobs.len()),
        running if running == jobs.len() => format!("Waiting on {running} {noun}"),
        running => format!("Waiting on {running} of {} {noun}", jobs.len()),
    };
    // The title carries the running count; the meta lists only settled ones.
    let meta = [
        (JobStatus::Completed, "done"),
        (JobStatus::Failed, "failed"),
        (JobStatus::Cancelled, "cancelled"),
    ]
    .into_iter()
    .filter_map(|(status, word)| {
        let count = count(status);
        (count > 0).then(|| format!("{count} {word}"))
    })
    .collect::<Vec<_>>();
    row.header.subtitle = (!meta.is_empty()).then(|| meta.join(" · "));
    let live = tool.has_execution_motion() && app.presentation_is_live();
    row.detail_blocks
        .push(TranscriptToolCallDetailBlock::Jobs(WaitJobs {
            jobs,
            spinner: live.then(|| {
                glyph_routed_streaming_spinner_frame(
                    app.theme(),
                    app.transcript_animation_phase(),
                    app.transcript_motion_enabled(),
                )
            }),
            hovered: match app.hovered_transcript_target() {
                Some(TranscriptMouseTarget::SubagentLink { session_id }) => {
                    Some(session_id.clone())
                }
                _ => None,
            },
        }));
}

/// A later wait replaces a poll whose jobs were all still running, so repeated
/// polls read as one persistent wait.
pub(super) fn displaced(tool: &ToolCallEntry, next: Option<&ToolCallEntry>) -> bool {
    next.is_some_and(|next| next.effective_tool_id() == WAIT_TOOL_ID)
        && tool.effective_tool_id() == WAIT_TOOL_ID
        && tool.status == ToolCallDisplayStatus::Succeeded
        && results(tool).is_some_and(|results| {
            let mut statuses = results
                .iter()
                .filter_map(|result| status(&result.status))
                .peekable();
            statuses.peek().is_some() && statuses.all(|status| status == JobStatus::Running)
        })
}

fn results(
    tool: &ToolCallEntry,
) -> Option<Vec<harness_core::subagent::GetCommandOrSubagentOutputResult>> {
    match GetCommandOrSubagentOutputValue::deserialize(tool.output_json.as_ref()?).ok()? {
        GetCommandOrSubagentOutputValue::MultiResult(multi) => Some(multi.results),
        _ => None,
    }
}

/// `None` drops ids the coordinator could not resolve.
fn status(status: &str) -> Option<JobStatus> {
    Some(match status {
        "not_found" => return None,
        "running" | "initializing" => JobStatus::Running,
        "completed" => JobStatus::Completed,
        "cancelled" => JobStatus::Cancelled,
        _ => JobStatus::Failed,
    })
}

/// The returned snapshot once the wait finished, else each awaited id's live state.
fn jobs(tool: &ToolCallEntry, app: &AppState) -> Vec<WaitJob> {
    let tasks = app.background_task_rows();
    let task = |id: &str| tasks.iter().find(|task| task.id == id);
    if let Some(results) = results(tool) {
        return results
            .into_iter()
            .filter_map(|result| {
                let status = status(&result.status)?;
                let description = result
                    .command
                    .strip_prefix("[subagent:")
                    .and_then(|rest| rest.split_once("] "))
                    .map(|(_, description)| description);
                let known = task(&result.task_id);
                Some(WaitJob {
                    kind: if known.map_or(description.is_some(), |task| task.child) {
                        "subagent"
                    } else {
                        "bash"
                    },
                    label: known.map_or_else(
                        || description.unwrap_or(&result.command).to_owned(),
                        |task| task.title.clone(),
                    ),
                    status,
                    duration_ms: std::time::Duration::try_from_secs_f64(result.duration_secs)
                        .map_or(0, |duration| {
                            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
                        }),
                    exit_code: result.exit_code,
                    // Running rows carry wait guidance, not a result.
                    preview: if status == JobStatus::Running {
                        String::new()
                    } else {
                        result
                            .output
                            .split("\n\n<subagent_meta>")
                            .next()
                            .unwrap_or_default()
                            .to_owned()
                    },
                    link: known.is_some_and(|task| task.child),
                    id: result.task_id,
                })
            })
            .collect();
    }
    let args = serde_json::from_str::<serde_json::Value>(&tool.args_summary).unwrap_or_default();
    args["task_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|id| task(id.as_str()?))
        .map(|task| {
            let status = match task.state {
                OrchestrationTaskState::Queued
                | OrchestrationTaskState::Running
                | OrchestrationTaskState::Stale => JobStatus::Running,
                OrchestrationTaskState::Completed | OrchestrationTaskState::LateResult => {
                    JobStatus::Completed
                }
                OrchestrationTaskState::Cancelled => JobStatus::Cancelled,
                OrchestrationTaskState::Failed | OrchestrationTaskState::TimedOut => {
                    JobStatus::Failed
                }
            };
            WaitJob {
                id: task.id.clone(),
                kind: if task.child { "subagent" } else { "bash" },
                status,
                label: task.title.clone(),
                duration_ms: if status == JobStatus::Running {
                    app.live_elapsed_since(task.started_ms)
                } else {
                    task.elapsed_ms
                },
                exit_code: None,
                preview: String::new(),
                link: task.child,
            }
        })
        .collect()
}

impl WaitJobs {
    /// Tree rows, each with its click target: collapsed shows eight jobs with
    /// one preview line each.
    pub(super) fn lines(
        &self,
        theme: &Theme,
        width: u16,
        expanded: bool,
    ) -> Vec<(Line<'static>, Option<TranscriptMouseTarget>)> {
        let width = usize::from(width);
        let dim = Style::default().fg(theme.text.tertiary);
        let (branch, last, vertical) = if theme.glyph_mode() == crate::theme::GlyphMode::Ascii {
            ("|- ", "`- ", "|  ")
        } else {
            ("├─ ", "└─ ", "│  ")
        };
        let shown = if expanded {
            self.jobs.len()
        } else {
            self.jobs.len().min(COLLAPSED_JOBS)
        };
        let hidden = self.jobs.len() - shown;
        let roots = shown + usize::from(hidden > 0);
        let mut lines = Vec::new();
        for (index, job) in self.jobs[..shown].iter().enumerate() {
            let (first, rest) = if index + 1 == roots {
                (last, "   ")
            } else {
                (branch, vertical)
            };
            let target = job.link.then(|| TranscriptMouseTarget::SubagentLink {
                session_id: job.id.clone(),
            });
            let hovered = job.link && self.hovered.as_ref() == Some(&job.id);
            let rows = self.job_rows(job, theme, width.saturating_sub(3), expanded);
            for (row, spans) in rows.into_iter().enumerate() {
                let mut line = vec![Span::styled(if row == 0 { first } else { rest }, dim)];
                line.extend(spans);
                let mut line = Line::from(crate::ui::ui_tool_wrapping::clip(line, width));
                if hovered {
                    line.style = line
                        .style
                        .bg(super::ui_transcript_tool_render::hover_background(theme));
                }
                lines.push((line, target.clone()));
            }
        }
        if hidden > 0 {
            lines.push((
                Line::from(vec![
                    Span::styled(last, dim),
                    Span::styled(
                        format!(
                            "… {hidden} more {}",
                            if hidden == 1 { "job" } else { "jobs" }
                        ),
                        muted_meta_style(theme),
                    ),
                ]),
                None,
            ));
        }
        lines
    }

    /// Head row (status, kind badge, id, label, duration), then preview rows.
    fn job_rows(
        &self,
        job: &WaitJob,
        theme: &Theme,
        width: usize,
        expanded: bool,
    ) -> Vec<Vec<Span<'static>>> {
        let fg = |color| Style::default().fg(color);
        let dim = fg(theme.text.tertiary);
        let glyphs = &theme.live_shell.glyphs;
        let (icon, icon_color, tone) = match job.status {
            JobStatus::Running => (
                self.spinner.unwrap_or(glyphs.running),
                theme.text.accent,
                theme.text.accent,
            ),
            JobStatus::Completed => (
                theme.live_shell.transcript_glyphs.success_marker,
                theme.status.success,
                theme.status.success,
            ),
            JobStatus::Failed => (glyphs.failed, theme.status.error, theme.status.error),
            JobStatus::Cancelled => (glyphs.cancelled, theme.status.error, theme.status.warning),
        };
        let mut head = vec![Span::styled(icon, fg(icon_color))];
        if let Some(code) = job.exit_code.filter(|_| job.kind == "bash") {
            head.push(Span::styled(" · ", dim));
            head.push(Span::styled(
                format!("exit {code}"),
                fg(if code == 0 {
                    theme.text.secondary
                } else {
                    theme.status.error
                }),
            ));
        }
        let id = collapse_inline_whitespace(&crate::ui::ui_tool_output::safe_tool_text(&job.id));
        let label = truncate_plain_text(
            &collapse_inline_whitespace(&crate::ui::ui_tool_output::safe_tool_text(&job.label)),
            LABEL_MAX_WIDTH,
        );
        let distinct = !label.is_empty() && label != id;
        head.extend([
            Span::raw(" "),
            Span::styled(format!("[{}]", job.kind), fg(tone)),
            Span::raw(" "),
            Span::styled(
                id,
                fg(
                    if job.status == JobStatus::Running && self.spinner.is_some() {
                        theme.text.accent
                    } else {
                        theme.text.primary
                    },
                ),
            ),
        ]);
        let duration = [
            Span::styled(" · ", dim),
            Span::styled(format_duration_ms(job.duration_ms), dim),
        ];
        let fixed = head
            .iter()
            .chain(&duration)
            .map(|span| display_width(&span.content))
            .sum::<usize>();
        let inline = distinct && fixed + 1 + display_width(&label) <= width;
        if inline {
            head.push(Span::styled(format!(" {label}"), fg(theme.text.primary)));
        }
        head.extend(duration);
        let mut rows = vec![head];
        // Pretty-printed JSON would spend the preview on a lone brace; flatten
        // only the prefix the preview can show.
        let preview = job.preview.trim();
        let preview = if preview.starts_with(['{', '[']) {
            let end = preview
                .char_indices()
                .nth(PREVIEW_LINE_WIDTH * 8)
                .map_or(preview.len(), |(index, _)| index);
            collapse_inline_whitespace(&preview[..end])
        } else {
            preview.to_owned()
        };
        let tone = if matches!(job.status, JobStatus::Failed | JobStatus::Cancelled) {
            fg(theme.status.error)
        } else {
            dim
        };
        let preview_width = PREVIEW_LINE_WIDTH.min(width.saturating_sub(2));
        rows.extend(
            preview
                .lines()
                .filter(|line| !line.trim().is_empty())
                .take(if expanded { 4 } else { 1 })
                .map(|line| {
                    let line = crate::ui::ui_tool_output::safe_tool_text(line.trim());
                    vec![Span::styled(
                        format!("  {}", truncate_plain_text(&line, preview_width)),
                        tone,
                    )]
                }),
        );
        rows
    }
}
