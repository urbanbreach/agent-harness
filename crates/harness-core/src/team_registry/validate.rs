use super::*;
use std::collections::BTreeSet;

impl TeamRegistry {
    pub(crate) fn validate(&self) -> Result<(), TeamRegistryError> {
        let summary = self.summary();
        if summary.teams > 256 || summary.mailbox_messages > 4096 {
            return Err(TeamRegistryError::Capacity);
        }
        for (id, team) in &self.parts.teams {
            if id != &team.team_id
                || !id
                    .strip_prefix("team_")
                    .is_some_and(|s| s.parse::<u64>().is_ok_and(|n| n > 0))
                || team.name.trim().is_empty()
            {
                return Err(TeamRegistryError::InvalidText);
            }
            check_label(&team.name)?;
            if team.members.len() > 256 {
                return Err(TeamRegistryError::Capacity);
            }
            let mut seen = BTreeSet::new();
            for m in &team.members {
                check_label(&m.agent_id)?;
                check_label(&m.role)?;
                if m.agent_id.trim().is_empty()
                    || m.agent_id != m.agent_id.trim()
                    || !seen.insert(&m.agent_id)
                {
                    return Err(TeamRegistryError::InvalidText);
                }
            }
        }
        let mut bytes = 0;
        for (id, messages) in &self.parts.mailboxes {
            let team = self.team(id)?;
            let mut seen = BTreeSet::new();
            for m in messages {
                check_label(&m.from_agent_id)?;
                if m.team_id != *id
                    || m.seq == 0
                    || m.message_id != format!("msg_{}", m.seq)
                    || m.from_agent_id.trim().is_empty()
                    || m.body.trim().is_empty()
                    || m.body.contains('\0')
                    || m.body.len() > 64 * 1024
                    || !seen.insert((&m.message_id, &m.to_agent_id))
                {
                    return Err(TeamRegistryError::InvalidText);
                }
                if m.to_agent_id
                    .as_ref()
                    .is_some_and(|to| !team.members.iter().any(|member| &member.agent_id == to))
                {
                    return Err(TeamRegistryError::InvalidText);
                }
                bytes += m.body.len();
            }
        }
        if bytes > 4 * 1024 * 1024 {
            return Err(TeamRegistryError::Capacity);
        }
        Ok(())
    }
}
