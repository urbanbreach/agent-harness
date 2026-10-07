//! Account renames; the label rules live with the pool (`accounts::display`).
use super::*;
use harness_providers::anthropic_subscription::accounts::display::{
    account_display_name, display_name_key, DISPLAY_NAME_MAX_COLUMNS,
};

/// `renameSlotDisplayName`: unique per provider; `None` clears the label.
pub fn rename_account(
    store: &CredentialStore,
    name: &str,
    value: Option<&str>,
) -> Result<(), String> {
    update_pool(store, |pool| {
        let target = pool
            .accounts
            .iter()
            .position(|slot| slot.name == name)
            .ok_or_else(|| format!("Stored provider account not found: {name}"))?;
        let display = match value {
            None => None,
            Some(value) => Some(account_display_name(Some(value)).ok_or_else(|| {
                format!(
                    "Display name must be 1-{DISPLAY_NAME_MAX_COLUMNS} terminal columns of visible text without control or formatting characters."
                )
            })?),
        };
        if let Some(display) = &display
            && pool.accounts.iter().any(|slot| {
                slot.name != name
                    && account_display_name(slot.display_name.as_deref())
                        .is_some_and(|other| display_name_key(&other) == display_name_key(display))
            })
        {
            return Err(
                "Display name is already used by another account for this provider.".into(),
            );
        }
        pool.accounts[target].display_name = display;
        Ok(())
    })
}
