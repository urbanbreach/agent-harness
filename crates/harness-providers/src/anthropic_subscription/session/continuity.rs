//! Native continuity decisions.
//!
//! Resume-first: only
//! compaction, a missing transcript, an unrecoverable boundary, model drift, or account drift
//! on the config-dir lane reach `flatten`.
use super::observability::sanitize_reason;
use super::sync::sent_hash_prefix_digest;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct EntrySnapshot {
    pub sdk_session_id: String,
    pub account_name: String,
    pub model_id: String,
    pub system_prompt_hash: String,
    pub toolset_hash: String,
    pub sent_count: usize,
    pub sent_hashes: Vec<String>,
    pub last_assistant_uuid: Option<String>,
    pub assistant_uuid_by_index: BTreeMap<usize, String>,
    pub pending_fork_reason: Option<String>,
    pub tainted_reason: Option<String>,
    pub credential_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingSnapshot {
    pub sdk_session_id: String,
    pub sent_count: usize,
    pub sent_hashes: Vec<String>,
    pub sent_prefix_hash: Option<String>,
    pub last_assistant_uuid: Option<String>,
    pub assistant_uuid_by_index: Vec<(usize, String)>,
    pub account_name: String,
    pub model_id: String,
    pub system_prompt_hash: String,
    pub toolset_hash: String,
    pub unanswered_turn_digest: Option<String>,
    /// `false` until the SDK acknowledged the id; never resumed while unconfirmed.
    pub sdk_session_id_confirmed: Option<bool>,
}

pub struct DecisionInput<'a> {
    pub entry: Option<&'a EntrySnapshot>,
    pub binding: Option<&'a BindingSnapshot>,
    pub current_hashes: &'a [String],
    pub account_name: &'a str,
    pub model_id: &'a str,
    pub system_prompt_hash: &'a str,
    pub toolset_hash: &'a str,
    pub transcript_available: bool,
    /// False only on the config-dir lane, whose per-account roots cannot share a transcript.
    pub cross_account_resume_supported: bool,
    pub idle_expired: bool,
    pub invalidation_reason: Option<&'a str>,
    pub credential_digest: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Bootstrap {
        reason: Option<&'static str>,
    },
    Delta {
        from: usize,
    },
    Reattach {
        sdk_session_id: String,
        from: usize,
        reason: &'static str,
    },
    Fork {
        sdk_session_id: String,
        at_uuid: String,
        from: usize,
        reason: &'static str,
    },
    Flatten {
        reason: &'static str,
    },
}

fn invalidation_cause(reason: &str) -> &'static str {
    match reason {
        "compaction" => "tainted_compaction",
        "tree_changed" => "branch_diverged",
        "fork" => "tainted_fork",
        other => sanitize_reason(other),
    }
}

/// When the ledger recorded why the binding went away, a cold-seed names that cause.
fn with_recorded_invalidation(decision: Decision, input: &DecisionInput<'_>) -> Decision {
    let Some(reason) = input.invalidation_reason else {
        return decision;
    };
    let cause = invalidation_cause(reason);
    match decision {
        Decision::Bootstrap { .. } => Decision::Bootstrap {
            reason: Some(cause),
        },
        Decision::Flatten {
            reason: "registry_miss",
        } => Decision::Flatten { reason: cause },
        other => other,
    }
}

fn common_prefix_length(left: &[String], right: &[String]) -> usize {
    left.iter().zip(right).take_while(|(a, b)| a == b).count()
}

/// The last assistant boundary STRICTLY BEFORE the divergence.
fn boundary_before(entry: &EntrySnapshot, count: usize) -> Option<(usize, String)> {
    (1..count).rev().find_map(|candidate| {
        entry
            .assistant_uuid_by_index
            .get(&candidate)
            .map(|uuid| (candidate, uuid.clone()))
    })
}

/// The newest mapped boundary at or below `cap` (index >= 1).
fn newest_boundary_within(entries: &[(usize, String)], cap: usize) -> Option<(usize, String)> {
    entries
        .iter()
        .filter(|(index, _)| *index >= 1 && *index <= cap)
        .max_by_key(|(index, _)| *index)
        .cloned()
}

fn fork_or_flatten(entry: &EntrySnapshot, diverges_at: usize, reason: &'static str) -> Decision {
    match boundary_before(entry, diverges_at) {
        None => Decision::Flatten { reason },
        Some((from, at_uuid)) => Decision::Fork {
            sdk_session_id: entry.sdk_session_id.clone(),
            at_uuid,
            from,
            reason,
        },
    }
}

fn identity_drift(
    input: &DecisionInput<'_>,
    account: &str,
    model: &str,
    system_prompt_hash: &str,
    toolset_hash: &str,
) -> Option<&'static str> {
    if account != input.account_name {
        Some("account_changed")
    } else if model != input.model_id {
        Some("model_changed")
    } else if system_prompt_hash != input.system_prompt_hash {
        Some("system_prompt_changed")
    } else if toolset_hash != input.toolset_hash {
        Some("toolset_changed")
    } else {
        None
    }
}

/// Same-turn retry after a failed attempt: fork at the pre-turn boundary so the un-answered
/// user message is not appended to the lineage a second time.
fn retry_checkpoint_decision(
    input: &DecisionInput<'_>,
    binding: &BindingSnapshot,
) -> Option<Decision> {
    let unanswered = binding.unanswered_turn_digest.as_ref()?;
    let current = input.current_hashes;
    if sent_hash_prefix_digest(current, current.len()) != *unanswered
        || current.len() < binding.sent_count
    {
        return None;
    }
    let prefix_matches = match &binding.sent_prefix_hash {
        Some(prefix) => sent_hash_prefix_digest(current, binding.sent_count) == *prefix,
        None => common_prefix_length(&binding.sent_hashes, current) == binding.sent_count,
    };
    if !prefix_matches {
        return None;
    }
    let boundary = match &binding.last_assistant_uuid {
        Some(uuid) => Some((binding.sent_count, uuid.clone())),
        None => newest_boundary_within(&binding.assistant_uuid_by_index, binding.sent_count),
    };
    Some(match boundary {
        None => Decision::Flatten {
            reason: "timeout_retry",
        },
        Some((from, at_uuid)) => Decision::Fork {
            sdk_session_id: binding.sdk_session_id.clone(),
            at_uuid,
            from,
            reason: "timeout_retry",
        },
    })
}

fn without_unconfirmed_resume(decision: Decision, binding: &BindingSnapshot) -> Decision {
    if binding.sdk_session_id_confirmed != Some(false) {
        return decision;
    }
    match decision {
        Decision::Reattach { .. } | Decision::Fork { .. } => Decision::Flatten {
            reason: "session_unconfirmed",
        },
        other => other,
    }
}

fn decide_from_binding(input: &DecisionInput<'_>, binding: &BindingSnapshot) -> Decision {
    if !input.transcript_available {
        return Decision::Flatten {
            reason: "transcript_missing",
        };
    }
    let drift = identity_drift(
        input,
        &binding.account_name,
        &binding.model_id,
        &binding.system_prompt_hash,
        &binding.toolset_hash,
    );
    if drift == Some("model_changed") {
        return Decision::Flatten {
            reason: "model_changed",
        };
    }
    if drift == Some("account_changed") && !input.cross_account_resume_supported {
        return Decision::Flatten {
            reason: "cross_root_unsupported",
        };
    }
    if let Some(retry) = retry_checkpoint_decision(input, binding) {
        return retry;
    }
    let current = input.current_hashes;
    if let Some(prefix) = &binding.sent_prefix_hash {
        let matches = current.len() >= binding.sent_count
            && sent_hash_prefix_digest(current, binding.sent_count) == *prefix;
        if matches {
            return Decision::Reattach {
                sdk_session_id: binding.sdk_session_id.clone(),
                from: binding.sent_count,
                reason: drift.unwrap_or("registry_miss"),
            };
        }
        return Decision::Flatten {
            reason: if current.len() < binding.sent_count {
                "history_rolled_back"
            } else {
                "sent_stream_diverged"
            },
        };
    }
    let shared = common_prefix_length(&binding.sent_hashes, current);
    if shared == binding.sent_count {
        return Decision::Reattach {
            sdk_session_id: binding.sdk_session_id.clone(),
            from: binding.sent_count,
            reason: drift.unwrap_or("registry_miss"),
        };
    }
    let reason = if shared < binding.sent_count {
        "history_rolled_back"
    } else {
        "sent_stream_diverged"
    };
    match newest_boundary_within(&binding.assistant_uuid_by_index, shared) {
        None => Decision::Flatten {
            reason: if binding.last_assistant_uuid.is_some() {
                reason
            } else {
                "registry_miss"
            },
        },
        Some((from, at_uuid)) => Decision::Fork {
            sdk_session_id: binding.sdk_session_id.clone(),
            at_uuid,
            from,
            reason,
        },
    }
}

pub fn decide_native_continuity(input: &DecisionInput<'_>) -> Decision {
    with_recorded_invalidation(decide_from_state(input), input)
}

fn decide_from_state(input: &DecisionInput<'_>) -> Decision {
    let Some(entry) = input.entry else {
        return match input.binding {
            None => Decision::Bootstrap { reason: None },
            Some(binding) => {
                without_unconfirmed_resume(decide_from_binding(input, binding), binding)
            }
        };
    };
    let divergence = entry
        .pending_fork_reason
        .as_deref()
        .or(entry.tainted_reason.as_deref());
    if divergence == Some("compaction") {
        return Decision::Flatten {
            reason: "tainted_compaction",
        };
    }
    if let Some(divergence) = divergence {
        let reason = if divergence == "assistant_rewritten" {
            "assistant_rewritten"
        } else {
            "other"
        };
        return fork_or_flatten(entry, entry.sent_count, reason);
    }
    let current = input.current_hashes;
    let shared = common_prefix_length(&entry.sent_hashes, current);
    if shared < entry.sent_count {
        let rolled_back = current.len() < entry.sent_count && shared == current.len();
        return if rolled_back {
            fork_or_flatten(entry, current.len(), "history_rolled_back")
        } else {
            fork_or_flatten(entry, shared + 1, "sent_stream_diverged")
        };
    }
    if input.idle_expired {
        return Decision::Reattach {
            sdk_session_id: entry.sdk_session_id.clone(),
            from: entry.sent_count,
            reason: "idle_ttl",
        };
    }
    if let Some(drift) = identity_drift(
        input,
        &entry.account_name,
        &entry.model_id,
        &entry.system_prompt_hash,
        &entry.toolset_hash,
    ) {
        return Decision::Reattach {
            sdk_session_id: entry.sdk_session_id.clone(),
            from: entry.sent_count,
            reason: drift,
        };
    }
    // A refresh revokes the access token the resident subprocess was spawned with.
    if let (Some(current), Some(spawned)) =
        (input.credential_digest, entry.credential_digest.as_deref())
        && current != spawned
    {
        return Decision::Reattach {
            sdk_session_id: entry.sdk_session_id.clone(),
            from: entry.sent_count,
            reason: "credential_refreshed",
        };
    }
    Decision::Delta {
        from: entry.sent_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hashes(n: usize, tag: &str) -> Vec<String> {
        (0..n).map(|i| format!("{tag}{i}")).collect()
    }

    fn entry(sent: Vec<String>) -> EntrySnapshot {
        EntrySnapshot {
            sdk_session_id: "sdk".into(),
            account_name: "a".into(),
            model_id: "m".into(),
            system_prompt_hash: "s".into(),
            toolset_hash: "t".into(),
            sent_count: sent.len(),
            sent_hashes: sent,
            last_assistant_uuid: None,
            assistant_uuid_by_index: BTreeMap::from([(1, "u1".into()), (2, "u2".into())]),
            pending_fork_reason: None,
            tainted_reason: None,
            credential_digest: Some("d".into()),
        }
    }

    fn input<'a>(
        entry: Option<&'a EntrySnapshot>,
        binding: Option<&'a BindingSnapshot>,
        current: &'a [String],
    ) -> DecisionInput<'a> {
        DecisionInput {
            entry,
            binding,
            current_hashes: current,
            account_name: "a",
            model_id: "m",
            system_prompt_hash: "s",
            toolset_hash: "t",
            transcript_available: true,
            cross_account_resume_supported: true,
            idle_expired: false,
            invalidation_reason: None,
            credential_digest: Some("d"),
        }
    }

    #[test]
    fn decision_table_covers_delta_rollback_divergence_drift_and_bindings() {
        let resident = entry(hashes(3, "h"));
        let mut current = hashes(3, "h");
        current.push("new".into());
        assert_eq!(
            decide_native_continuity(&input(Some(&resident), None, &current)),
            Decision::Delta { from: 3 }
        );
        let rolled = hashes(2, "h");
        assert!(
            matches!(decide_native_continuity(&input(Some(&resident), None, &rolled)),
            Decision::Fork { from: 1, ref at_uuid, reason: "history_rolled_back", .. } if at_uuid == "u1")
        );
        let diverged = vec!["h0".into(), "x".into(), "y".into()];
        assert!(matches!(
            decide_native_continuity(&input(Some(&resident), None, &diverged)),
            Decision::Fork {
                from: 1,
                reason: "sent_stream_diverged",
                ..
            }
        ));
        let mut tainted = resident.clone();
        tainted.pending_fork_reason = Some("compaction".into());
        assert_eq!(
            decide_native_continuity(&input(Some(&tainted), None, &current)),
            Decision::Flatten {
                reason: "tainted_compaction"
            }
        );
        let mut drift = input(Some(&resident), None, &current);
        drift.model_id = "other";
        assert!(matches!(
            decide_native_continuity(&drift),
            Decision::Reattach {
                reason: "model_changed",
                ..
            }
        ));
        let mut refreshed = input(Some(&resident), None, &current);
        refreshed.credential_digest = Some("rotated");
        assert!(matches!(
            decide_native_continuity(&refreshed),
            Decision::Reattach {
                reason: "credential_refreshed",
                ..
            }
        ));
        let binding = BindingSnapshot {
            sdk_session_id: "sdk".into(),
            sent_count: 3,
            sent_hashes: Vec::new(),
            sent_prefix_hash: Some(sent_hash_prefix_digest(&current, 3)),
            last_assistant_uuid: Some("u3".into()),
            assistant_uuid_by_index: vec![(3, "u3".into())],
            account_name: "a".into(),
            model_id: "m".into(),
            system_prompt_hash: "s".into(),
            toolset_hash: "changed".into(),
            unanswered_turn_digest: None,
            sdk_session_id_confirmed: None,
        };
        assert!(matches!(
            decide_native_continuity(&input(None, Some(&binding), &current)),
            Decision::Reattach {
                from: 3,
                reason: "toolset_changed",
                ..
            }
        ));
        let mut unconfirmed = binding.clone();
        unconfirmed.sdk_session_id_confirmed = Some(false);
        assert_eq!(
            decide_native_continuity(&input(None, Some(&unconfirmed), &current)),
            Decision::Flatten {
                reason: "session_unconfirmed"
            }
        );
        let mut invalidated = input(None, None, &current);
        invalidated.invalidation_reason = Some("compaction");
        assert_eq!(
            decide_native_continuity(&invalidated),
            Decision::Bootstrap {
                reason: Some("tainted_compaction")
            }
        );
    }
}
