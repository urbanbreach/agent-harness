//! Root-chain compaction settings a worker captures when it spawns.
//!
//! A root agent follows the live settings, including its own named agent override. A direct
//! child of a root follows them too unless it has an override of its own. A child of a child
//! pins the root policy as it was at spawn, so an ancestor's override never leaks down, and
//! an agent with its own override pins that override and ignores the per-model maps.
//! Every child carries a mid-turn flag (default on), which doubles as the marker that tells
//! a root overlay from a child overlay.
use super::threshold_override;
use crate::config::CompactionSettings;
use crate::coord::compaction::state::CompactionOverlay;
use crate::coord::runtime::Runtime;
use harness_compaction::policy::{self, ThresholdSettings};
use std::num::NonZeroU32;

/// The live global threshold pair with reserve and base window.
fn live_thresholds(live: &CompactionSettings) -> ThresholdSettings {
    ThresholdSettings {
        threshold_percent: live.threshold_percent.map(|percent| percent.get()),
        threshold_tokens: live.threshold_tokens.map(NonZeroU32::get),
        base_window_tokens: live.base_window_tokens,
        reserve_tokens: live.reserve_tokens,
    }
}

impl CompactionOverlay {
    /// Captures the overlay for an agent named `agent_name` being spawned.
    ///
    /// `parent` is the overlay of the spawning agent, or `None` for a root agent. A root
    /// agent ignores `spawn_mid_turn`; a child honors it, including an explicit `false`,
    /// and defaults to on.
    pub(in crate::coord) fn capture(
        live: &CompactionSettings,
        agent_name: &str,
        parent: Option<&CompactionOverlay>,
        spawn_mid_turn: Option<bool>,
    ) -> Self {
        let Some(parent) = parent else {
            return Self::default();
        };
        let mut overlay = Self {
            mid_turn_enabled: Some(spawn_mid_turn.unwrap_or(true)),
            ..Self::default()
        };
        let child_of_child = parent.mid_turn_enabled.is_some();
        if let Some(value) = live.agent_thresholds.get(agent_name) {
            let applied = policy::apply_agent_override(
                &live_thresholds(live),
                Some(threshold_override(*value, true)),
            );
            overlay.threshold_percent = applied.threshold_percent;
            overlay.threshold_tokens = applied.threshold_tokens;
            overlay.ignore_model_thresholds = true;
            overlay.pinned = true;
        } else if child_of_child {
            overlay.threshold_percent = live.threshold_percent.map(|percent| percent.get());
            overlay.threshold_tokens = live.threshold_tokens.map(NonZeroU32::get);
            overlay.ignore_model_thresholds = !live.model_thresholds_enabled;
            overlay.pinned = true;
        }
        overlay.enabled = child_of_child.then_some(live.enabled);
        overlay
    }

    /// The threshold settings for the model `provider`/`model` this agent runs on now.
    pub(in crate::coord) fn thresholds(
        &self,
        live: &CompactionSettings,
        agent_name: &str,
        provider: &str,
        model: &str,
    ) -> ThresholdSettings {
        let mut base = live_thresholds(live);
        if self.pinned {
            base.threshold_percent = self.threshold_percent;
            base.threshold_tokens = self.threshold_tokens;
        } else if let Some(value) = live.agent_thresholds.get(agent_name) {
            return policy::apply_agent_override(&base, Some(threshold_override(*value, true)));
        }
        let ignore_models = if self.pinned {
            self.ignore_model_thresholds
        } else {
            !live.model_thresholds_enabled
        };
        let over = (!ignore_models)
            .then(|| policy::match_model_key(&live.model_thresholds, provider, model))
            .flatten()
            .map(|value| threshold_override(*value, false));
        policy::apply_model_override(&base, over)
    }

    pub(in crate::coord) fn enabled(&self, live: &CompactionSettings) -> bool {
        self.enabled.unwrap_or(live.enabled)
    }

    /// Mid-turn maintenance: a child's captured flag, else the live setting.
    pub(in crate::coord) fn mid_turn_enabled(&self, live: &CompactionSettings) -> bool {
        self.mid_turn_enabled.unwrap_or(live.mid_turn_enabled)
    }
}

impl Runtime {
    /// The overlay for `agent_id`, folded down its parent chain from the root agent.
    /// Children carry a mid-turn flag of on; roots carry none.
    pub(in crate::coord) fn compaction_overlay(&self, agent_id: &str) -> CompactionOverlay {
        let mut chain = Vec::new();
        let mut next = Some(agent_id);
        while let Some(id) = next {
            let Some(agent) = self.agents.get(id) else {
                break;
            };
            if chain.len() >= self.agents.len() {
                break;
            }
            chain.push(agent.profile.name.as_str());
            next = agent.info.parent_agent_id.as_deref();
        }
        let live = &self.config.compaction;
        let mut overlay: Option<CompactionOverlay> = None;
        for name in chain.into_iter().rev() {
            overlay = Some(CompactionOverlay::capture(
                live,
                name,
                overlay.as_ref(),
                None,
            ));
        }
        overlay.unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CompactionThreshold, CompactionThresholdPercent};

    fn percent(value: f64) -> Option<CompactionThresholdPercent> {
        CompactionThresholdPercent::new(value).ok()
    }

    fn live() -> CompactionSettings {
        let mut live = CompactionSettings {
            threshold_percent: percent(80.0),
            threshold_tokens: None,
            base_window_tokens: None,
            model_thresholds_enabled: true,
            ..CompactionSettings::default()
        };
        if let (Some(base), Some(tokens)) = (NonZeroU32::new(400_000), NonZeroU32::new(50_000)) {
            live.model_thresholds
                .insert("p/m".into(), CompactionThreshold::Base { base });
            live.agent_thresholds
                .insert("tuned".into(), CompactionThreshold::Tokens { tokens });
        }
        live
    }

    /// (percent, tokens, base window) the overlay resolves for provider `p`, model `m`.
    fn resolved(
        overlay: &CompactionOverlay,
        live: &CompactionSettings,
        agent: &str,
    ) -> (Option<f64>, Option<u32>, Option<u32>) {
        let settings = overlay.thresholds(live, agent, "p", "m");
        (
            settings.threshold_percent,
            settings.threshold_tokens,
            settings.base_window_tokens,
        )
    }

    #[test]
    fn capture_follows_the_root_chain_rules() {
        let live = live();
        let root = CompactionOverlay::capture(&live, "main", None, Some(false));
        assert_eq!(
            root,
            CompactionOverlay::default(),
            "root ignores the spawn flag"
        );
        let child = CompactionOverlay::capture(&live, "worker", Some(&root), None);
        assert_eq!((child.pinned, child.enabled), (false, None));
        assert_eq!(child.mid_turn_enabled, Some(true));
        let child_off = CompactionOverlay::capture(&live, "worker", Some(&root), Some(false));
        assert_eq!(child_off.mid_turn_enabled, Some(false));
        let grandchild = CompactionOverlay::capture(&live, "worker", Some(&child), None);
        assert_eq!(
            (
                grandchild.pinned,
                grandchild.enabled,
                grandchild.threshold_percent
            ),
            (true, Some(true), Some(80.0))
        );
        let tuned = CompactionOverlay::capture(&live, "tuned", Some(&root), None);
        assert_eq!(
            (
                tuned.pinned,
                tuned.ignore_model_thresholds,
                tuned.threshold_tokens,
                tuned.threshold_percent
            ),
            (true, true, Some(50_000), None)
        );
        let below_tuned = CompactionOverlay::capture(&live, "worker", Some(&tuned), None);
        assert_eq!(
            below_tuned.threshold_tokens, None,
            "back to the root policy"
        );
        assert_eq!(below_tuned.threshold_percent, Some(80.0));
        assert!(!below_tuned.ignore_model_thresholds);
    }

    #[test]
    fn resolution_uses_pinned_pairs_and_live_values_where_documented() {
        let mut live = live();
        let root = CompactionOverlay::default();
        let child = CompactionOverlay::capture(&live, "worker", Some(&root), None);
        let grandchild = CompactionOverlay::capture(&live, "worker", Some(&child), None);
        let tuned = CompactionOverlay::capture(&live, "tuned", Some(&root), None);
        // (overlay, agent, expected) before and after the live pair changes.
        for (name, overlay, agent, before, after) in [
            (
                "root",
                &root,
                "main",
                (Some(80.0), None, Some(400_000)),
                (Some(60.0), None, Some(400_000)),
            ),
            (
                "root with override",
                &root,
                "tuned",
                (None, Some(50_000), None),
                (None, Some(50_000), None),
            ),
            (
                "direct child",
                &child,
                "worker",
                (Some(80.0), None, Some(400_000)),
                (Some(60.0), None, Some(400_000)),
            ),
            (
                "grandchild",
                &grandchild,
                "worker",
                (Some(80.0), None, Some(400_000)),
                (Some(80.0), None, Some(400_000)),
            ),
            (
                "named override",
                &tuned,
                "tuned",
                (None, Some(50_000), None),
                (None, Some(50_000), None),
            ),
        ] {
            assert_eq!(resolved(overlay, &live, agent), before, "{name}");
            let mut changed = live.clone();
            changed.threshold_percent = percent(60.0);
            assert_eq!(
                resolved(overlay, &changed, agent),
                after,
                "{name} after change"
            );
        }
        live.model_thresholds_enabled = false;
        assert_eq!(
            resolved(&child, &live, "worker").2,
            None,
            "live flag gates a direct child"
        );
        assert_eq!(
            resolved(&grandchild, &live, "worker").2,
            Some(400_000),
            "pinned flag"
        );
    }

    #[test]
    fn enabled_and_mid_turn_resolve_against_live_settings() {
        let mut live = live();
        live.enabled = true;
        live.mid_turn_enabled = true;
        let root = CompactionOverlay::default();
        let child = CompactionOverlay::capture(&live, "worker", Some(&root), None);
        let grandchild = CompactionOverlay::capture(&live, "worker", Some(&child), Some(false));
        live.enabled = false;
        live.mid_turn_enabled = false;
        assert_eq!(
            (root.enabled(&live), root.mid_turn_enabled(&live)),
            (false, false)
        );
        assert_eq!(
            (child.enabled(&live), child.mid_turn_enabled(&live)),
            (false, true)
        );
        assert_eq!(
            (
                grandchild.enabled(&live),
                grandchild.mid_turn_enabled(&live)
            ),
            (true, false)
        );
    }
}
