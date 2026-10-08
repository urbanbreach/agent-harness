//! Populated, actionable journeys; expectations still come only from the pinned binary.
use super::{
    journey::{envelope, Journey, Result},
    Recorder, K, M,
};
use harness_core::{event::EventEnvelopeV1, memory::DurableMemoryStore, proj::SessionCatalogEntry};
use harness_tui::app::{AppState, LaunchMetadata, ModelOption, SessionHistoryEntry, UiIntent};
use serde_json::json;
use std::{fs, path::Path, sync::Arc};

impl Recorder {
    pub(super) fn mentions(&mut self) -> Result {
        fs::create_dir_all("src")?;
        fs::write("src/main.rs", "fn main() {}\n")?;
        for (name, query, expected) in [
            ("file", "@main", "src/main.rs"),
            ("subagent", "@expl", "explore"),
        ] {
            let mut j = Journey::new(false);
            j.app.set_file_mention_workspace_root_for_test(".".into());
            j.app.set_launch_metadata(
                LaunchMetadata::from_model_ref("worker", "mock:reference")
                    .with_available_models(vec![
                        ModelOption::from_model_ref("worker", "mock:reference"),
                        ModelOption::from_model_ref("explore", "mock:reference"),
                    ])
                    .with_switchable_profiles(vec!["worker".into()]),
            );
            j.text(query);
            self.frame(&format!("{name}-mention-picker"), &mut j)?;
            j.key(K::Enter, M::NONE);
            self.frame(&format!("{name}-mention-selected"), &mut j)?;
            j.key(K::Enter, M::NONE);
            self.frame(&format!("{name}-mention-submitted"), &mut j)?;
            let intents = j.intents.lock().unwrap_or_else(|error| error.into_inner());
            assert!(
                intents
                    .iter()
                    .any(|intent| intent.starts_with("SubmitPrompt") && intent.contains(expected)),
                "mention was not submitted: {intents:?}"
            );
        }
        Ok(())
    }

    pub(super) fn working_permissions(&mut self) -> Result {
        for (name, question, reject) in [
            ("allow", false, false),
            ("reject", false, true),
            ("answer", true, false),
            ("custom-answer", true, true),
        ] {
            let mut j = Journey::new(false);
            j.text("preserved draft");
            j.permission(question)?;
            if question && reject {
                j.key(K::Char('z'), M::NONE);
                j.text("Use a local file 界");
            } else if reject {
                // Last decision is Reject; ordinary text edits its optional feedback.
                for _ in 0..3 {
                    j.key(K::Down, M::NONE);
                }
                j.text("Read only; do not write 界");
            } else if !question {
                j.key(K::Down, M::NONE);
                j.key(K::Down, M::NONE);
            }
            self.frame(&format!("{name}-before-submit"), &mut j)?;
            j.key(K::Enter, M::NONE);
            self.frame(&format!("{name}-submitted"), &mut j)?;
            assert!(
                j.intents
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .iter()
                    .any(|intent| intent.starts_with("ResolvePermission")),
                "{name} did not submit"
            );
            j.event(
                "permission_resolved",
                json!({"permission_id": "permission",
                "decision": if reject && !question { "deny" } else { "allow" }, "reason": null}),
            )?;
            self.frame(&format!("{name}-acknowledged"), &mut j)?;
        }
        Ok(())
    }

    pub(super) fn populated_dialogs(&mut self, root: &Path) -> Result {
        let data_dir = root.join("../data");
        let runtime_dir =
            harness_core::storage_paths::ProjectPaths::new(&data_dir, root)?.runtime_dir();
        fs::create_dir_all(runtime_dir.join("plans"))?;
        fs::write(
            runtime_dir.join("plans/fixture.md"),
            "# Fixture plan\n\n- Preserve the draft.\n- Verify terminal cleanup.\n",
        )?;
        DurableMemoryStore::for_runtime(&runtime_dir)
            .put("terminal", "Use graphemes and display cells.")?;
        fs::write(root.join("settings.json"), "{\"hashline_edit\":false}\n")?;
        for command in ["view-plan", "settings"] {
            let mut j = Journey::new(false);
            j.app.set_file_mention_workspace_root_for_test(root.into());
            j.app.set_storage_data_dir(data_dir.clone());
            j.app.bind_settings_project_config(
                root.join("settings.json"),
                false,
                true,
                false,
                false,
                false,
                false,
            );
            j.text(&format!("/{command}"));
            j.key(K::Enter, M::NONE);
            if command == "settings" {
                j.key(K::Char('/'), M::NONE);
                j.text("hashline_edit");
                j.key(K::Enter, M::NONE);
            }
            self.frame(&format!("populated-{command}"), &mut j)?;
            j.key(K::Enter, M::NONE);
            self.frame(&format!("activated-{command}"), &mut j)?;
            if command == "settings" {
                let written: serde_json::Value =
                    serde_json::from_str(&fs::read_to_string(root.join("settings.json"))?)?;
                assert_eq!(
                    written["hashline_edit"], true,
                    "settings edit did not persist"
                );
            }
            j.key(K::Esc, M::NONE);
        }
        let mut j = Journey::new(false);
        j.app.set_file_mention_workspace_root_for_test(root.into());
        j.app.set_storage_data_dir(data_dir);
        j.app.open_memory_browser();
        self.frame("memory-populated", &mut j)?;
        j.key(K::Char('/'), M::NONE);
        j.text("terminal");
        self.frame("memory-filtered", &mut j)?;
        j.key(K::Esc, M::NONE);
        j.key(K::Esc, M::NONE);
        j.text("/models");
        j.key(K::Enter, M::NONE);
        j.key(K::Down, M::NONE);
        self.frame("model-choice", &mut j)?;
        j.key(K::Enter, M::NONE);
        self.frame("model-committed", &mut j)?;
        assert!(j
            .intents
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|intent| intent.starts_with("SwitchModel")));
        let mut j = Journey::new(false);
        j.text("stash 界 this draft");
        palette(&mut j, "Stash prompt");
        self.frame("stash-saved", &mut j)?;
        palette(&mut j, "Stash list");
        self.frame("stash-populated", &mut j)?;
        j.key(K::Enter, M::NONE);
        self.frame("stash-restored", &mut j)?;
        Ok(())
    }

    pub(super) fn disk_sessions(&mut self, root: &Path) -> Result {
        let path = root.join("session");
        fs::create_dir_all(&path)?;
        let events: Vec<EventEnvelopeV1> = [
            ("run_started", json!({"run_name": "Fixture session", "workspace_root": "/workspace/agent-harness"})),
            ("user_message_submitted", json!({"request_id": "turn", "text": "Preserve this recorded prompt"})),
            ("provider_request_started", json!({"request_id": "turn", "provider_id": "mock", "model_id": "reference",
                "prompt_summary": "Recorded prompt", "request_digest": "fixture", "metadata": null})),
            ("assistant_message_finished", json!({"request_id": "turn", "tool_call_count": 0,
                "parts": [{"kind": "text", "text": "Recorded **answer** with 界 and e\u{301}."}], "provenance": null, "assistant_message": null})),
            ("provider_request_finished", json!({"request_id": "turn", "finish_reason": "stop", "output_digest": "fixture", "usage": null, "metadata": null})),
        ].into_iter().enumerate().map(|(i, (kind, data))| serde_json::from_value(envelope(i as u64 + 1, kind, data)))
            .collect::<std::result::Result<_, _>>()?;
        let body = events
            .iter()
            .map(serde_json::to_string)
            .collect::<std::result::Result<Vec<_>, _>>()?
            .join("\n")
            + "\n";
        fs::write(path.join("events.jsonl"), &body)?;
        let catalog: SessionCatalogEntry = serde_json::from_value(
            json!({"run_id": "reference", "run_name": "Fixture session",
            "status": null, "last_updated_at": null, "workspace_root": "/workspace/agent-harness", "profile_preset": "worker",
            "provider_model": "mock:reference", "mode_source": "interactive_mock", "is_resumable": true,
            "resume_disabled_reason": null, "artifact_count": 0, "child_session_count": 0, "parent_session_id": null}),
        )?;
        let mut j = Journey::new(true);
        // Session intent paths use the fixture's relative path, independent of checkout location.
        j.app.set_session_history_entries(vec![SessionHistoryEntry {
            run_dir: "session".into(),
            catalog,
        }]);
        j.text("/sessions");
        j.key(K::Enter, M::NONE);
        self.frame("sessions-populated", &mut j)?;
        j.key(K::Enter, M::NONE);
        self.frame("session-resume-selected", &mut j)?;
        assert!(j
            .intents
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|intent| intent.starts_with("ContinueSession")));
        let mut replay = Journey::new(false);
        replay.app = AppState::new_replay("session".into(), events.clone());
        replay.app.freeze_animation_clock();
        self.frame("replay-recorded", &mut replay)?;
        replay.text("must not be submitted");
        replay.key(K::Enter, M::NONE);
        replay.key(K::Char('r'), M::CONTROL);
        self.frame("replay-read-only", &mut replay)?;
        assert_eq!(fs::read_to_string(path.join("events.jsonl"))?, body);
        let intents = Arc::clone(&replay.intents);
        let sink = Arc::new(move |intent: UiIntent| {
            intents
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(format!("{intent:?}"));
        });
        replay.app = AppState::new_live(Some("session".into()), false, Some(sink));
        replay.app.replace_events(events);
        replay.app.freeze_animation_clock();
        self.frame("resumed-history", &mut replay)?;
        replay.text("/fork");
        replay.key(K::Enter, M::NONE);
        self.frame("fork-picker", &mut replay)?;
        replay.key(K::Enter, M::NONE);
        self.frame("fork-selected", &mut replay)?;
        Ok(())
    }
}

fn palette(journey: &mut Journey, query: &str) {
    journey.key(K::Char('p'), M::CONTROL);
    journey.text(query);
    journey.key(K::Enter, M::NONE);
}
