// allow: SIZE_OK — TUI app state (session projection + interaction)
use std::borrow::Cow;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::proj::{SessionCatalogEntry, SessionModeSource};
use harness_core::session_title::is_parent_default_title;

use super::{set_pending_live_prompt_draft, AppState, StartupLauncherAction, UiIntent};
use crate::time_format::short_time_or_trimmed;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHistoryEntry {
    pub run_dir: PathBuf,
    pub catalog: SessionCatalogEntry,
}

pub(crate) fn session_history_run_name(entry: &SessionHistoryEntry) -> &str {
    entry.catalog.run_name.as_deref().unwrap_or("<unavailable>")
}

pub(crate) fn session_history_display_title(entry: &SessionHistoryEntry) -> Cow<'_, str> {
    let run_name = session_history_run_name(entry);
    if is_parent_default_title(run_name) {
        Cow::Borrowed("New session")
    } else {
        Cow::Borrowed(run_name)
    }
}

pub(crate) fn session_history_current_marker(
    entry: &SessionHistoryEntry,
    current_session_id: Option<&str>,
) -> bool {
    current_session_id == Some(entry.catalog.run_id.as_str())
}

pub(crate) fn session_history_category_label(entry: &SessionHistoryEntry) -> String {
    let Some((year, month, day)) = entry
        .catalog
        .last_updated_at
        .as_deref()
        .and_then(session_history_date_parts)
    else {
        return "Unknown".to_string();
    };

    if current_utc_date() == Some((year, month, day)) {
        return "Today".to_string();
    }

    format!(
        "{} {} {:02} {}",
        weekday_name(year, month, day),
        month_name(month),
        day,
        year
    )
}

pub(crate) fn session_history_footer_label(entry: &SessionHistoryEntry) -> String {
    entry
        .catalog
        .last_updated_at
        .as_deref()
        .map(session_history_time_label)
        .unwrap_or_default()
}

pub(super) fn session_history_profile_label(entry: &SessionHistoryEntry) -> &str {
    entry
        .catalog
        .profile_preset
        .as_deref()
        .unwrap_or("<unavailable>")
}

fn session_history_entry_matches_action(
    entry: &SessionHistoryEntry,
    action: StartupLauncherAction,
) -> bool {
    if entry.catalog.parent_session_id.is_some() {
        return false;
    }

    match action {
        StartupLauncherAction::ContinueSession => matches!(
            entry.catalog.mode_source,
            SessionModeSource::InteractiveLive | SessionModeSource::InteractiveMock
        ),
        StartupLauncherAction::ReplaySession => true,
        StartupLauncherAction::NewSession => !matches!(
            entry.catalog.mode_source,
            SessionModeSource::ScenarioFixture | SessionModeSource::ReplayOnly
        ),
    }
}

const fn session_history_action_sort_bucket(
    entry: &SessionHistoryEntry,
    action: StartupLauncherAction,
) -> u8 {
    match action {
        StartupLauncherAction::ContinueSession if !entry.catalog.is_resumable => 1,
        _ => 0,
    }
}

fn session_history_filter_matches(entry: &SessionHistoryEntry, input: &str) -> bool {
    let input = input.trim();
    if input.is_empty() {
        return true;
    }

    let title = session_history_display_title(entry).to_lowercase();
    if title.contains(input) || fuzzy_subsequence_score(&title, input).is_some() {
        return true;
    }

    let run_id = entry.catalog.run_id.to_lowercase();
    if run_id.contains(input) || fuzzy_subsequence_score(&run_id, input).is_some() {
        return true;
    }

    session_history_search_fields(entry)
        .into_iter()
        .any(|field| field.to_lowercase().contains(input))
}

fn session_history_search_fields(entry: &SessionHistoryEntry) -> Vec<String> {
    let mut fields = vec![
        session_history_profile_label(entry).to_string(),
        session_history_category_label(entry),
        session_history_footer_label(entry),
        format!("{:?}", entry.catalog.mode_source),
    ];
    if let Some(provider_model) = entry.catalog.provider_model.as_ref() {
        fields.push(provider_model.clone());
    }
    if let Some(status) = entry.catalog.status {
        fields.push(format!("{status:?}"));
    }
    if entry.catalog.is_resumable {
        fields.push("continue ready".to_string());
        fields.push("replay ready".to_string());
    } else if let Some(reason) = entry.catalog.resume_disabled_reason.as_ref() {
        fields.push(reason.clone());
    }
    fields
}

fn session_history_time_label(timestamp: &str) -> String {
    if let Some((hour, minute)) = epoch_millis_time_parts(timestamp) {
        return format_twelve_hour_time(hour, minute);
    }

    let short = short_time_or_trimmed(timestamp);
    let Some((hour, minute)) = short.split_once(':').and_then(|(hour, minute)| {
        Some((
            hour.parse::<u8>().ok()?,
            minute.get(..2)?.parse::<u8>().ok()?,
        ))
    }) else {
        return short;
    };

    format_twelve_hour_time(hour, minute)
}

fn format_twelve_hour_time(hour: u8, minute: u8) -> String {
    let suffix = if hour < 12 { "AM" } else { "PM" };
    let display_hour = match hour % 12 {
        0 => 12,
        hour => hour,
    };
    format!("{display_hour}:{minute:02} {suffix}")
}

fn session_history_date_parts(timestamp: &str) -> Option<(i32, u8, u8)> {
    iso_date_parts(timestamp).or_else(|| epoch_millis_date_parts(timestamp))
}

fn epoch_millis_date_parts(timestamp: &str) -> Option<(i32, u8, u8)> {
    let seconds = epoch_millis_seconds(timestamp)?;
    civil_from_days(seconds.div_euclid(86_400))
}

fn epoch_millis_time_parts(timestamp: &str) -> Option<(u8, u8)> {
    let seconds = epoch_millis_seconds(timestamp)?;
    let seconds_of_day = seconds.rem_euclid(86_400);
    Some((
        u8::try_from(seconds_of_day / 3_600).ok()?,
        u8::try_from((seconds_of_day % 3_600) / 60).ok()?,
    ))
}

fn epoch_millis_seconds(timestamp: &str) -> Option<i64> {
    let trimmed = timestamp.trim();
    if trimmed.len() < 10 || !trimmed.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let millis = trimmed.parse::<i64>().ok()?;
    Some(millis / 1_000)
}

fn iso_date_parts(timestamp: &str) -> Option<(i32, u8, u8)> {
    let trimmed = timestamp.trim();
    if trimmed.len() < 10
        || trimmed.as_bytes().get(4) != Some(&b'-')
        || trimmed.as_bytes().get(7) != Some(&b'-')
    {
        return None;
    }
    let year = trimmed.get(0..4)?.parse::<i32>().ok()?;
    let month = trimmed.get(5..7)?.parse::<u8>().ok()?;
    let day = trimmed.get(8..10)?.parse::<u8>().ok()?;
    (1..=12).contains(&month).then_some(())?;
    (1..=31).contains(&day).then_some(())?;
    Some((year, month, day))
}

fn current_utc_date() -> Option<(i32, u8, u8)> {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    civil_from_days(i64::try_from(duration.as_secs() / 86_400).ok()?)
}

fn civil_from_days(days_since_unix_epoch: i64) -> Option<(i32, u8, u8)> {
    let z = days_since_unix_epoch.checked_add(719_468)?;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    Some((
        i32::try_from(year).ok()?,
        u8::try_from(month).ok()?,
        u8::try_from(day).ok()?,
    ))
}

fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let mut year = i64::from(year);
    let month = i64::from(month);
    let day = i64::from(day);
    year -= i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * month_prime + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn weekday_name(year: i32, month: u8, day: u8) -> &'static str {
    const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    let days = days_from_civil(year, month, day);
    let index = usize::try_from(days.rem_euclid(7)).unwrap_or(0);
    WEEKDAYS[index]
}

const fn month_name(month: u8) -> &'static str {
    match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "???",
    }
}

pub(super) fn fuzzy_subsequence_score(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if let Some(index) = haystack.find(needle) {
        return Some(index);
    }

    let mut needle_chars = needle.chars();
    let mut current = needle_chars.next()?;
    let mut matched = 0usize;
    let mut gap_score = 0usize;
    for (position, candidate) in haystack.chars().enumerate() {
        if candidate != current {
            gap_score = gap_score.saturating_add(1);
            continue;
        }
        matched = matched.saturating_add(1);
        gap_score = gap_score.saturating_add(position.saturating_sub(matched.saturating_sub(1)));
        match needle_chars.next() {
            Some(next) => current = next,
            None => {
                return Some(gap_score.saturating_add(haystack.len().saturating_sub(needle.len())));
            }
        }
    }
    None
}

impl AppState {
    pub fn set_session_history_entries(&mut self, entries: Vec<SessionHistoryEntry>) {
        self.session_history_entries = entries;
        self.update_session_history_filter();
        self.rebuild_model_options();
        if self.lineage_browser_visible {
            let current_run_id = self.current_session_id().map(str::to_string);
            let entries = self
                .session_history_entries
                .iter()
                .map(|entry| entry.catalog.clone())
                .collect::<Vec<_>>();
            self.lineage_browser
                .rebuild(entries, current_run_id, &self.palette_input);
        }
        self.session_history_selected = self
            .session_history_selected
            .min(self.session_history_filtered.len().saturating_sub(1));
    }

    pub fn selected_session_history_entry(&self) -> Option<&SessionHistoryEntry> {
        self.session_history_filtered
            .get(self.session_history_selected)
            .and_then(|index| self.session_history_entries.get(*index))
    }

    pub(in crate::app) fn handle_session_history_key(&mut self, key: &KeyEvent) -> bool {
        if self.session_rename_visible {
            return self.handle_session_rename_key(key);
        }
        let ctrl_only = key.modifiers == KeyModifiers::CONTROL;
        match key.code {
            KeyCode::Esc => {
                self.close_session_history();
                true
            }
            KeyCode::Enter => {
                self.execute_selected_session_launcher_action();
                true
            }
            KeyCode::PageUp => {
                self.session_delete_armed_run_id = None;
                self.move_session_history_selection(-10);
                true
            }
            KeyCode::PageDown => {
                self.session_delete_armed_run_id = None;
                self.move_session_history_selection(10);
                true
            }
            KeyCode::Home => {
                self.session_delete_armed_run_id = None;
                self.session_history_selected = 0;
                true
            }
            KeyCode::End => {
                self.session_delete_armed_run_id = None;
                self.session_history_selected =
                    self.session_history_filtered.len().saturating_sub(1);
                true
            }
            KeyCode::Up => {
                self.session_delete_armed_run_id = None;
                self.move_session_history_selection(-1);
                true
            }
            KeyCode::Down => {
                self.session_delete_armed_run_id = None;
                self.move_session_history_selection(1);
                true
            }
            KeyCode::Backspace => {
                self.overlay_backspace(Self::update_session_history_filter);
                true
            }
            KeyCode::Delete => {
                self.overlay_delete(Self::update_session_history_filter);
                true
            }
            KeyCode::Char('p') if ctrl_only => {
                self.move_session_history_selection(-1);
                true
            }
            KeyCode::Char('n') if ctrl_only => {
                self.move_session_history_selection(1);
                true
            }
            KeyCode::Char('f') if ctrl_only => {
                self.toggle_session_pin();
                true
            }
            KeyCode::Char('d') if ctrl_only => {
                self.handle_session_delete_press();
                true
            }
            KeyCode::Char('r') if ctrl_only => {
                self.open_session_rename_dialog();
                true
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::ALT)
                {
                    return false;
                }
                self.session_delete_armed_run_id = None;
                self.overlay_insert_char(c, Self::update_session_history_filter);
                true
            }
            _ => false,
        }
    }

    fn move_session_history_selection(&mut self, delta: isize) {
        let len = self.session_history_filtered.len();
        if len == 0 {
            self.session_history_selected = 0;
            return;
        }

        if delta == -1 {
            self.session_history_selected = if self.session_history_selected == 0 {
                len - 1
            } else {
                self.session_history_selected - 1
            };
            return;
        }

        if delta == 1 {
            self.session_history_selected = (self.session_history_selected + 1) % len;
            return;
        }

        let current = isize::try_from(self.session_history_selected.min(len.saturating_sub(1)))
            .unwrap_or(isize::MAX);
        let next = (current + delta).clamp(
            0,
            isize::try_from(len.saturating_sub(1)).unwrap_or(isize::MAX),
        );
        self.session_history_selected = usize::try_from(next).unwrap_or(0);
    }

    fn update_session_history_filter(&mut self) {
        let input = self.palette_input.to_lowercase();
        let mut filtered = self
            .session_history_entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                session_history_entry_matches_action(entry, self.startup_launcher_action)
            })
            .filter(|(_, entry)| session_history_filter_matches(entry, &input))
            .map(|(index, entry)| {
                let is_pinned = self.session_pins.contains(&entry.catalog.run_id);
                (
                    index,
                    session_history_action_sort_bucket(entry, self.startup_launcher_action),
                    is_pinned,
                )
            })
            .collect::<Vec<_>>();
        filtered.sort_by(
            |(left_index, left_bucket, left_pinned), (right_index, right_bucket, right_pinned)| {
                let left_entry = &self.session_history_entries[*left_index];
                let right_entry = &self.session_history_entries[*right_index];
                right_pinned
                    .cmp(left_pinned)
                    .then_with(|| left_bucket.cmp(right_bucket))
                    .then_with(|| {
                        right_entry
                            .catalog
                            .last_updated_at
                            .as_deref()
                            .unwrap_or("")
                            .cmp(left_entry.catalog.last_updated_at.as_deref().unwrap_or(""))
                    })
                    .then_with(|| {
                        session_history_display_title(left_entry)
                            .cmp(&session_history_display_title(right_entry))
                    })
                    .then_with(|| left_entry.catalog.run_id.cmp(&right_entry.catalog.run_id))
            },
        );
        self.session_history_filtered = filtered.into_iter().map(|(index, _, _)| index).collect();
        self.session_history_selected = 0;
    }

    pub(crate) fn session_history_visual_row_count(&self) -> usize {
        let mut rows = 0usize;
        let mut previous_category: Option<String> = None;
        for entry_index in &self.session_history_filtered {
            let Some(entry) = self.session_history_entries.get(*entry_index) else {
                continue;
            };
            let category = session_history_category_label(entry);
            if previous_category.as_deref() != Some(category.as_str()) {
                if previous_category.is_some() {
                    rows = rows.saturating_add(1);
                }
                rows = rows.saturating_add(1);
                previous_category = Some(category);
            }
            rows = rows.saturating_add(1);
        }
        rows
    }

    pub(in crate::app) fn begin_session_history_picker(&mut self, action: StartupLauncherAction) {
        self.startup_launcher_action = action;
        self.continue_disabled_banner = None;
        self.palette_visible = true;
        self.model_switcher_visible = false;
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.update_session_history_filter();
        self.open_session_history();
    }

    fn open_session_history(&mut self) {
        if !self.session_history_visible {
            self.palette_focus_return.get_or_insert(self.focus);
        }
        self.palette_visible = true;
        self.session_history_selected = self
            .session_history_selected
            .min(self.session_history_filtered.len().saturating_sub(1));
        self.session_history_visible = true;
        self.sync_slash_overlay();
    }

    pub(in crate::app) fn close_session_history(&mut self) {
        self.close_palette();
    }

    fn execute_selected_session_launcher_action(&mut self) {
        if self.session_history_entries.is_empty() {
            if matches!(
                self.startup_launcher_action,
                StartupLauncherAction::ContinueSession
            ) {
                self.continue_disabled_banner =
                    Some("continue unavailable: no session history entries".to_string());
            } else {
                self.continue_disabled_banner =
                    Some("replay unavailable: no session history entries".to_string());
            }
            self.open_session_history();
            return;
        }

        if self.session_history_filtered.is_empty() {
            self.continue_disabled_banner =
                Some("no sessions match the current filter".to_string());
            self.open_session_history();
            return;
        }

        let Some(selected) = self.selected_session_history_entry() else {
            return;
        };
        let selected_run_id = selected.catalog.run_id.clone();
        let selected_run_dir = selected.run_dir.clone();
        let selected_resumable = selected.catalog.is_resumable;
        let selected_resume_disabled_reason = selected.catalog.resume_disabled_reason.clone();

        match self.startup_launcher_action {
            StartupLauncherAction::NewSession => {
                self.apply_new_session_launcher_selection();
            }
            StartupLauncherAction::ContinueSession => {
                if !selected_resumable {
                    self.continue_disabled_banner = selected_resume_disabled_reason
                        .map(|reason| format!("continue unavailable: {reason}"))
                        .or_else(|| {
                            Some("continue unavailable for the selected session".to_string())
                        });
                    return;
                }

                self.continue_disabled_banner = None;
                self.replay_mode = false;
                set_pending_live_prompt_draft(Some(self.composer.prompt_buffer.clone()));
                self.emit_ui_intent(UiIntent::ContinueSession {
                    run_id: selected_run_id,
                    run_dir: selected_run_dir,
                });
                self.should_quit = true;
                self.close_session_history();
            }
            StartupLauncherAction::ReplaySession => {
                self.continue_disabled_banner = None;
                self.replay_mode = true;
                self.emit_ui_intent(UiIntent::ReplaySession {
                    run_id: selected_run_id,
                    run_dir: selected_run_dir,
                });
                self.should_quit = true;
                self.close_session_history();
            }
        }
    }

    fn toggle_session_pin(&mut self) {
        let Some(entry) = self.selected_session_history_entry() else {
            return;
        };
        let run_id = entry.catalog.run_id.clone();
        if !self.session_pins.insert(run_id.clone()) {
            self.session_pins.remove(&run_id);
        }
        self.persist_session_pins();
        self.update_session_history_filter();
    }

    fn handle_session_delete_press(&mut self) {
        let Some(entry) = self.selected_session_history_entry() else {
            return;
        };
        let run_id = entry.catalog.run_id.clone();
        let run_dir = entry.run_dir.clone();
        if self.session_delete_armed_run_id.as_deref() == Some(run_id.as_str()) {
            self.session_delete_armed_run_id = None;
            self.emit_ui_intent(UiIntent::DeleteSession { run_id, run_dir });
            self.close_session_history();
        } else {
            self.session_delete_armed_run_id = Some(run_id);
        }
    }

    pub(in crate::app) fn open_session_rename_dialog(&mut self) {
        if let Some(entry) = self.selected_session_history_entry() {
            let run_id = entry.catalog.run_id.clone();
            let title = session_history_display_title(entry).to_string();
            self.session_rename_target_run_id = Some(run_id);
            self.session_rename_input = title;
            self.session_rename_cursor = self.session_rename_input.chars().count();
            self.session_rename_visible = true;
            return;
        }
        if let Some(run_id) = self.run_id().map(str::to_string) {
            self.session_rename_target_run_id = Some(run_id.clone());
            self.session_rename_input = run_id;
            self.session_rename_cursor = self.session_rename_input.chars().count();
            self.session_rename_visible = true;
        }
    }

    fn handle_session_rename_key(&mut self, key: &KeyEvent) -> bool {
        let ctrl_only = key.modifiers == KeyModifiers::CONTROL;
        match key.code {
            KeyCode::Esc => {
                self.close_session_rename_dialog();
                true
            }
            KeyCode::Enter => {
                self.submit_session_rename();
                true
            }
            KeyCode::Left => {
                if self.session_rename_cursor > 0 {
                    self.session_rename_cursor -= 1;
                }
                true
            }
            KeyCode::Right => {
                if self.session_rename_cursor < self.session_rename_input.chars().count() {
                    self.session_rename_cursor += 1;
                }
                true
            }
            KeyCode::Home => {
                self.session_rename_cursor = 0;
                true
            }
            KeyCode::End => {
                self.session_rename_cursor = self.session_rename_input.chars().count();
                true
            }
            KeyCode::Backspace => {
                if self.session_rename_cursor > 0 {
                    self.session_rename_cursor -= 1;
                    let byte_idx = self
                        .session_rename_input
                        .char_indices()
                        .nth(self.session_rename_cursor)
                        .map(|(index, _)| index)
                        .unwrap_or(self.session_rename_input.len());
                    self.session_rename_input.remove(byte_idx);
                }
                true
            }
            KeyCode::Delete => {
                if self.session_rename_cursor < self.session_rename_input.chars().count() {
                    let byte_idx = self
                        .session_rename_input
                        .char_indices()
                        .nth(self.session_rename_cursor)
                        .map(|(index, _)| index)
                        .unwrap_or(self.session_rename_input.len());
                    self.session_rename_input.remove(byte_idx);
                }
                true
            }
            KeyCode::Char('a') if ctrl_only => {
                self.session_rename_cursor = 0;
                true
            }
            KeyCode::Char('e') if ctrl_only => {
                self.session_rename_cursor = self.session_rename_input.chars().count();
                true
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::ALT)
                {
                    return false;
                }
                let byte_idx = self
                    .session_rename_input
                    .char_indices()
                    .nth(self.session_rename_cursor)
                    .map(|(index, _)| index)
                    .unwrap_or(self.session_rename_input.len());
                self.session_rename_input.insert(byte_idx, c);
                self.session_rename_cursor += 1;
                true
            }
            _ => false,
        }
    }

    fn submit_session_rename(&mut self) {
        let Some(run_id) = self.session_rename_target_run_id.take() else {
            self.close_session_rename_dialog();
            return;
        };
        let title = self.session_rename_input.trim().to_string();
        if !title.is_empty() {
            self.emit_ui_intent(UiIntent::UpdateSessionTitle { title });
        }
        let _ = run_id;
        self.close_session_rename_dialog();
    }

    fn close_session_rename_dialog(&mut self) {
        self.session_rename_visible = false;
        self.session_rename_input.clear();
        self.session_rename_cursor = 0;
        self.session_rename_target_run_id = None;
    }

    fn persist_session_pins(&mut self) {
        let Some(path) = self.session_pins_path.as_deref() else {
            return;
        };
        if let Err(err) = super::session_pins::save_session_pins(path, &self.session_pins) {
            self.status_banner = Some(err);
        }
    }
}
