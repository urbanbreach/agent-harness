use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum TeamCreateOutcome {
    Created { team_id: String, name: String },
    Failed { name: String, reason: String },
}
impl TeamCreateOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Created { team_id, name } => {
                format!("team create: ok id=`{team_id}` name=`{name}`")
            }
            Self::Failed { name, reason } => {
                format!("team create: failed name=`{name}` ({reason})")
            }
        }
    }
}
pub fn create_team_outcome(
    registry: &mut TeamRegistry,
    name: impl Into<String>,
) -> TeamCreateOutcome {
    let name = name.into();
    match registry.create_team(&name) {
        Ok(t) => TeamCreateOutcome::Created {
            team_id: t.team_id,
            name: t.name,
        },
        Err(e) => TeamCreateOutcome::Failed {
            name,
            reason: e.to_string(),
        },
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum TeamSendOutcome {
    Sent {
        message_id: String,
        team_id: String,
        from_agent_id: String,
    },
    Failed {
        team_id: String,
        reason: String,
    },
}
impl TeamSendOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Sent {
                message_id,
                team_id,
                from_agent_id,
            } => {
                format!("team send: ok msg=`{message_id}` team=`{team_id}` from=`{from_agent_id}`")
            }
            Self::Failed { team_id, reason } => {
                format!("team send: failed team=`{team_id}` ({reason})")
            }
        }
    }
}
pub fn send_team_message_outcome(
    registry: &mut TeamRegistry,
    team_id: &str,
    from_agent_id: impl Into<String>,
    to_agent_id: Option<String>,
    body: impl Into<String>,
) -> TeamSendOutcome {
    match registry.send_message(team_id, from_agent_id, to_agent_id, body) {
        Ok(m) => TeamSendOutcome::Sent {
            message_id: m.message_id,
            team_id: m.team_id,
            from_agent_id: m.from_agent_id,
        },
        Err(e) => TeamSendOutcome::Failed {
            team_id: team_id.into(),
            reason: e.to_string(),
        },
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum TeamAddMemberOutcome {
    Added {
        team_id: String,
        agent_id: String,
        role: String,
        member_count: usize,
    },
    Failed {
        team_id: String,
        agent_id: String,
        reason: String,
    },
}
impl TeamAddMemberOutcome {
    pub fn one_line(&self) -> String {
        match self { Self::Added { team_id, agent_id, role, member_count } => format!("team add-member: ok team=`{team_id}` agent=`{agent_id}` role=`{role}` members={member_count}"), Self::Failed { team_id, agent_id, reason } => format!("team add-member: failed team=`{team_id}` agent=`{agent_id}` ({reason})") }
    }
}
pub fn add_team_member_outcome(
    registry: &mut TeamRegistry,
    team_id: &str,
    agent_id: impl Into<String>,
    role: impl Into<String>,
) -> TeamAddMemberOutcome {
    let agent_id = agent_id.into();
    let role = role.into();
    match registry.add_member(team_id, &agent_id, &role) {
        Ok(t) => TeamAddMemberOutcome::Added {
            team_id: t.team_id,
            agent_id,
            role,
            member_count: t.members.len(),
        },
        Err(e) => TeamAddMemberOutcome::Failed {
            team_id: team_id.into(),
            agent_id,
            reason: e.to_string(),
        },
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum TeamCancelOutcome {
    Cancelled { team_id: String, name: String },
    Failed { team_id: String, reason: String },
}
impl TeamCancelOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Cancelled { team_id, name } => {
                format!("team cancel: ok id=`{team_id}` name=`{name}`")
            }
            Self::Failed { team_id, reason } => {
                format!("team cancel: failed id=`{team_id}` ({reason})")
            }
        }
    }
}
pub fn cancel_team_outcome(registry: &mut TeamRegistry, team_id: &str) -> TeamCancelOutcome {
    match registry.cancel_team(team_id) {
        Ok(t) => TeamCancelOutcome::Cancelled {
            team_id: t.team_id,
            name: t.name,
        },
        Err(e) => TeamCancelOutcome::Failed {
            team_id: team_id.into(),
            reason: e.to_string(),
        },
    }
}
