use super::*;

impl Runtime {
    pub(in crate::coord) fn skill_permission(
        &self,
        actor: &EventActor,
        tool: &str,
        args: &Value,
    ) -> Result<crate::perm::PermissionAction, CoordinatorError> {
        use crate::perm::PermissionAction;
        if tool != "skill" {
            return Ok(PermissionAction::Allow);
        }
        let name = args.get("name").and_then(Value::as_str).unwrap_or_default();
        let entry = actor
            .agent_id
            .as_ref()
            .and_then(|id| self.agents.get(id))
            .and_then(|agent| agent.skill_startup.as_ref())
            .and_then(|snapshot| {
                snapshot
                    .catalog
                    .entries
                    .iter()
                    .find(|entry| entry.name == name || entry.stable_id == name)
            });
        let mut action = match entry {
            Some(entry) if !entry.loadable => PermissionAction::Deny,
            Some(entry) if entry.permission_mode == "ask" => PermissionAction::Ask,
            _ => PermissionAction::Allow,
        };
        // Recheck current shared policy even when the catalog came from a
        // finalized child created under an earlier configuration.
        for (pattern, mode) in self.config.skills.permissions.iter().rev() {
            let matcher = globset::Glob::new(pattern)
                .map_err(|error| CoordinatorError::Invalid(error.to_string()))?
                .compile_matcher();
            if matcher.is_match(name)
                || entry.is_some_and(|entry| {
                    matcher.is_match(&entry.name) || matcher.is_match(&entry.stable_id)
                })
            {
                action = match (action, mode) {
                    (PermissionAction::Deny, _) | (_, PermissionMode::Deny) => {
                        PermissionAction::Deny
                    }
                    (PermissionAction::Ask, _) | (_, PermissionMode::Ask) => PermissionAction::Ask,
                    _ => PermissionAction::Allow,
                };
                break;
            }
        }
        Ok(action)
    }

    fn discover_startup_skills(
        &self,
        cwd: &Path,
        config: SkillsConfig,
        discover: bool,
    ) -> Result<Option<Arc<SkillStartupSnapshot>>, CoordinatorError> {
        let Some(discovery) = &self.config.skill_catalog_discovery else {
            return Ok(None);
        };
        let catalog = if discover {
            discovery.discover(
                cwd,
                &config,
                self.config
                    .subagent_discovery
                    .as_ref()
                    .map(|context| context.project_trusted),
            )?
        } else {
            SkillCatalog::default()
        };
        Ok(Some(Arc::new(SkillStartupSnapshot { config, catalog })))
    }

    /// A restored registration is observable without filesystem discovery.
    /// Live execution can recover its catalog from the verified private state;
    /// replay never follows paths in the journal or rediscovers skill files.
    pub(in crate::coord) fn ensure_skill_startup(
        &mut self,
        agent: &str,
    ) -> Result<(), CoordinatorError> {
        let state = self
            .agents
            .get(agent)
            .ok_or_else(|| CoordinatorError::UnknownAgent(agent.into()))?;
        if self.config.skill_catalog_discovery.is_none() || state.skill_startup.is_some() {
            return Ok(());
        }
        if self.native_subagents.contains_key(agent) {
            if let FinalizedStateResult::Available { state } =
                self.resolve_agent_finalized_state(agent)?
            {
                if let Some(snapshot) = state.skill_startup {
                    self.agents
                        .get_mut(agent)
                        .ok_or_else(|| CoordinatorError::UnknownAgent(agent.into()))?
                        .skill_startup = Some(Arc::new(snapshot));
                    return Ok(());
                }
            }
            return Err(native_invalid(
                "Native skill startup snapshot is unavailable after restore; start a fresh child."
                    .into(),
            ));
        }
        let cwd = self.execution_cwd(&EventActor::new(ActorKind::Worker, Some(agent.into())))?;
        let snapshot = self.discover_startup_skills(&cwd, self.config.skills.clone(), true)?;
        if let Some(state) = self.agents.get_mut(agent) {
            state.skill_startup = snapshot;
        }
        Ok(())
    }

    pub(super) fn native_skill_startup(
        &mut self,
        agent: &str,
        cwd: &Path,
    ) -> Result<Option<Arc<SkillStartupSnapshot>>, CoordinatorError> {
        if self.config.skill_catalog_discovery.is_none() {
            return Ok(None);
        }
        let child = &self.native_subagents[agent];
        // Same-identity wake keeps its original startup source. A new-identity
        // resume applies the newly resolved definition and actual spawner below.
        if child.registration.source.as_deref() == Some(agent) {
            return self.agents[agent]
                .skill_startup
                .clone()
                .or_else(|| child.source_state.as_ref().and_then(|source| source.skill_startup.clone()).map(Arc::new))
                .map(Some)
                .ok_or_else(|| {
                    native_invalid(
                        "Source skill startup snapshot is unavailable after restore; start a fresh child."
                            .into(),
                    )
                });
        }
        let definition = child
            .resolved
            .as_ref()
            .ok_or_else(|| native_invalid("skill startup definition is unavailable".into()))?
            .definition
            .clone();
        if definition.inherit_skills {
            let spawner = child.registration.spawner.clone();
            self.ensure_skill_startup(&spawner)?;
            return Ok(self.agents[&spawner].skill_startup.clone());
        }
        // Discovery settings reset, but shared per-skill policy does not.
        let config = SkillsConfig {
            permissions: self.config.skills.permissions.clone(),
            ..Default::default()
        };
        self.discover_startup_skills(cwd, config, definition.discover_skills)
    }
}
