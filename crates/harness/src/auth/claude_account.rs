//! senpi `/claude-account`: list, remove, pin, unpin, rename and clear-name.
use super::*;

const CLAUDE_ACCOUNT_USAGE: &str =
    "Usage: /claude-account [add | remove <id> | pin <id> | unpin | rename <id> <display name...> | clear-name <id>]";

/// senpi `/claude-account`; returns the notice it would show.
pub(super) fn claude_account(
    args: &[String],
    store: &CredentialStore,
    settings: &harness_providers::anthropic_subscription::AnthropicSubscriptionSettings,
    deps: &CliDeps,
) -> Result<String, String> {
    use harness_core::auth::anthropic_subscription::{
        account_label, pin_account, read_pool, remove_named_account, rename_account, slot_status,
    };
    use harness_providers::anthropic_subscription::accounts::{
        list_accounts, select_account, AffinityOptions,
    };
    let env = |name: &str| deps.env_var_value(name);
    let action = args.first().map_or("list", String::as_str);
    let name = args.get(1).map(String::as_str);
    match (action, name) {
        ("rename", Some(name)) if args.len() > 2 => {
            rename_account(store, name, Some(&args[2..].join(" ")))?;
            let pool = read_pool(store)?;
            let label = pool
                .accounts
                .iter()
                .find(|a| a.name == name)
                .map_or_else(|| name.to_owned(), account_label);
            Ok(format!("Account display name updated: {label}."))
        }
        ("clear-name", Some(name)) if args.len() == 2 => {
            rename_account(store, name, None)?;
            Ok(format!("Account display name updated: {name}."))
        }
        ("rename" | "clear-name", _) => {
            Err("Usage: rename <id> <display name...> or clear-name <id>".into())
        }
        ("list", _) => {
            let pool = read_pool(store)?;
            let accounts = list_accounts(&pool, Some(&env));
            let pinned = settings.pinned_account.clone().or(pool.pinned.clone());
            let source = if settings.pinned_account.is_some() {
                "settings"
            } else {
                "stored"
            };
            let now = harness_core::auth::anthropic::now_epoch_ms();
            let pick = (!accounts.is_empty()).then(|| {
                select_account(
                    &accounts,
                    &AffinityOptions {
                        pinned_account: pinned.as_deref(),
                        now,
                        ..AffinityOptions::default()
                    },
                )
            });
            let label = |name: &str| {
                accounts
                    .iter()
                    .find(|a| a.name == name)
                    .map_or_else(|| name.to_owned(), account_label)
            };
            let mut lines = vec!["Anthropic Subscription accounts:".to_owned()];
            if accounts.is_empty() {
                lines.push("  (none)".into());
            }
            for account in &accounts {
                let mut states = vec![
                    account_label(account),
                    account.source.as_str().into(),
                    slot_status(account, now),
                ];
                if pinned.as_deref() == Some(account.name.as_str()) {
                    states.push("pinned".into());
                }
                if matches!(&pick, Some(Ok(p)) if p.name == account.name) {
                    states.push("affinity pick".into());
                }
                lines.push(format!("  {}", states.join(" | ")));
            }
            lines.push(format!(
                "Pinned account: {}",
                pinned
                    .as_deref()
                    .map_or_else(|| "none".to_owned(), |p| format!("{} ({source})", label(p)))
            ));
            lines.push(format!(
                "Affinity pick: {}",
                match &pick {
                    None => "none".to_owned(),
                    Some(Ok(p)) => label(&p.name),
                    Some(Err(e)) => format!("unavailable - {e}"),
                }
            ));
            Ok(lines.join("\n"))
        }
        ("remove", Some(name)) => {
            remove_named_account(store, name, &env)?;
            Ok(format!("Removed Anthropic Subscription account: {name}."))
        }
        ("remove", None) => Err("Usage: /claude-account remove <name>".into()),
        ("pin", Some("unpin")) | ("unpin", _) => {
            if read_pool(store)?.pinned.is_none() {
                return Ok("No stored Anthropic Subscription account pin is set.".into());
            }
            pin_account(store, None, &env)?;
            Ok("Unpinned Anthropic Subscription account.".into())
        }
        ("pin", Some(name)) => {
            if !list_accounts(&read_pool(store)?, Some(&env))
                .iter()
                .any(|a| a.name == name)
            {
                return Err(format!(
                    "Anthropic Subscription account '{name}' does not exist."
                ));
            }
            pin_account(store, Some(name), &env)?;
            Ok(format!("Pinned Anthropic Subscription account: {name}."))
        }
        ("pin", None) => Err("Usage: /claude-account pin <name>".into()),
        _ => Err(CLAUDE_ACCOUNT_USAGE.into()),
    }
}
