use super::*;

impl Runtime {
    pub(super) fn start_native_disposal(&mut self, agent: &str) -> Result<(), CoordinatorError> {
        let child = &self.native_subagents[agent];
        let worktree = child
            .worktree
            .clone()
            .ok_or_else(|| native_invalid("native worktree disposal has no worktree".into()))?;
        let source = self.info()?.workspace_root.clone();
        let scratch = self.info()?.artifacts_dir.clone();
        let handle = self.handle()?;
        let owner = agent.to_owned();
        let ownership = child.ownership.clone();
        let prior_reference = child.snapshot_ref.clone();
        let id = self.id("subagent-dispose")?;
        let completion_id = id.clone();
        let cancellation = CancellationToken::new();
        let worker_cancellation = cancellation.clone();
        let actor = EventActor::new(ActorKind::Worker, Some(agent.into()));
        let join = self.jobs.spawn(async move {
            let result = async {
                if worker_cancellation.is_cancelled() {
                    return Ok(PreparedSubagent {
                        cwd: worktree.clone(),
                        worktree: Some(worktree),
                        snapshot_ref: prior_reference,
                        ownership,
                        failure: None,
                    });
                }
                // Cancellation is observed between ownership transitions,
                // never by dropping an async snapshot or removal future.
                let reference = workspace::snapshot(&owner, &worktree, &source, &scratch).await?;
                let recipient = owner.clone();
                let receipt = NativeWorkspaceReceipt {
                    payload_version: 1,
                    child_id: owner.clone(),
                    snapshot_ref: reference.clone(),
                    worktree_path: worktree.to_string_lossy().into_owned(),
                    removed: false,
                };
                handle
                    .call(move |s| s.record_native_workspace(receipt))
                    .await?;
                if ownership.is_some()
                    && !worker_cancellation.is_cancelled()
                    && workspace::remove(&worktree, &source).await.is_ok()
                {
                    let receipt = NativeWorkspaceReceipt {
                        payload_version: 1,
                        child_id: recipient,
                        snapshot_ref: reference.clone(),
                        worktree_path: worktree.to_string_lossy().into_owned(),
                        removed: true,
                    };
                    handle
                        .call(move |s| s.record_native_workspace(receipt))
                        .await?;
                    Ok(PreparedSubagent {
                        cwd: source,
                        worktree: None,
                        snapshot_ref: Some(reference),
                        ownership: None,
                        failure: None,
                    })
                } else {
                    Ok(PreparedSubagent {
                        cwd: worktree.clone(),
                        worktree: Some(worktree),
                        snapshot_ref: Some(reference),
                        ownership,
                        failure: None,
                    })
                }
            }
            .await;
            Completion::SubagentPrepared {
                id: completion_id,
                result,
            }
        });
        self.running.insert(
            id.clone(),
            Job {
                join_id: Some(join.id()),
                actor,
                kind: JobKind::SubagentPreparation {
                    agent: agent.into(),
                },
                parent: None,
                cancellation,
                reason: None,
                hooks: Vec::new(),
            },
        );
        if let Some(child) = self.native_subagents.get_mut(agent) {
            child.preparation = Some(id);
        }
        Ok(())
    }

    fn record_native_workspace(
        &mut self,
        receipt: NativeWorkspaceReceipt,
    ) -> Result<(), CoordinatorError> {
        let owner = receipt.child_id.clone();
        let applied_owner = owner.clone();
        let reference = receipt.snapshot_ref.clone();
        let removed = receipt.removed;
        self.emit_applied(
            EventActor::new(ActorKind::Worker, Some(owner.clone())),
            None,
            EventV1::NativeSubagentWorkspace(Box::new(receipt)),
            move |runtime, _| {
                if let Some(child) = runtime.native_subagents.get_mut(&applied_owner) {
                    child.snapshot_ref = Some(reference);
                    if removed {
                        child.worktree = None;
                    }
                }
            },
        )?;
        // This subordinate replacement must succeed before deletion is admitted.
        self.write_native_projection(&owner, true)
    }
}
