use super::*;

pub(crate) fn active_path(session: &CanonicalSession) -> Result<Vec<&SessionEntry>, SessionError> {
    let mut path = Vec::new();
    let mut visited = BTreeSet::<EntryId>::new();
    let mut current_id = session.active_leaf.as_ref();

    while let Some(entry_id) = current_id {
        if !visited.insert(entry_id.clone()) {
            return Err(SessionError::ParentCycle {
                entry_id: entry_id.clone(),
            });
        }
        let Some(entry) = session.entries.get(entry_id) else {
            return Err(SessionError::ActiveLeafMissing {
                entry_id: entry_id.clone(),
            });
        };
        path.push(entry);
        current_id = entry.parent_id.as_ref();
    }

    path.reverse();
    Ok(path)
}

pub(super) fn validate_selected_tool_pairs(
    session: &CanonicalSession,
    entry_id: &EntryId,
    tool_pairing: &ToolPairingState,
) -> Result<(), SessionError> {
    let mut ancestry = BTreeSet::new();
    let mut current_id = Some(entry_id);
    while let Some(current) = current_id {
        if !ancestry.insert(current.clone()) {
            return Err(SessionError::ParentCycle {
                entry_id: current.clone(),
            });
        }
        let Some(entry) = session.entries.get(current) else {
            return Err(SessionError::ActiveLeafMissing {
                entry_id: current.clone(),
            });
        };
        current_id = entry.parent_id.as_ref();
    }

    for (tool_call_id, assistant_entry_id) in &tool_pairing.calls {
        let call_selected = ancestry.contains(assistant_entry_id);
        let result_selected = tool_pairing
            .results
            .get(tool_call_id)
            .is_some_and(|result_entry_id| ancestry.contains(result_entry_id));
        if tool_pairing.settled.contains(tool_call_id) && call_selected != result_selected {
            return Err(SessionError::ToolResultOffActivePath {
                tool_call_id: tool_call_id.clone(),
            });
        }
    }

    Ok(())
}
