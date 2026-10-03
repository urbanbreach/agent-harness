use super::*;

mod authorization;

enum Subscription {
    Command(Box<super::super::commands::CommandSubscription>),
    Subagent(watch::Receiver<NativeSnapshot>),
    Missing(String),
}
impl Subscription {
    fn result(&self) -> GetCommandOrSubagentOutputResult {
        match self {
            Self::Command(command) => command.updates.borrow().result.clone(),
            Self::Subagent(child) => child.borrow().result.clone(),
            Self::Missing(id) => GetCommandOrSubagentOutputResult {
                task_id: id.clone(),
                status: "not_found".into(),
                output: format!("Task {id} not found."),
                ..Default::default()
            },
        }
    }
    fn pending(&self) -> bool {
        match self {
            Self::Command(command) => !command.updates.borrow().is_terminal(),
            Self::Subagent(child) => !child.borrow().terminal,
            Self::Missing(_) => false,
        }
    }
    async fn wait_terminal(mut self) -> Result<(), CoordinatorError> {
        match &mut self {
            Self::Command(command) => loop {
                if command.updates.borrow_and_update().is_terminal() {
                    return Ok(());
                }
                command
                    .updates
                    .changed()
                    .await
                    .map_err(|_| CoordinatorError::Closed)?;
            },
            Self::Subagent(child) => loop {
                if child.borrow_and_update().terminal {
                    return Ok(());
                }
                child
                    .changed()
                    .await
                    .map_err(|_| CoordinatorError::Closed)?;
            },
            Self::Missing(_) => Ok(()),
        }
    }
    fn cloned(&self) -> Self {
        match self {
            Self::Command(command) => {
                Self::Command(Box::new(super::super::commands::CommandSubscription {
                    snapshot: command.updates.borrow().clone(),
                    updates: command.updates.clone(),
                }))
            }
            Self::Subagent(child) => Self::Subagent(child.clone()),
            Self::Missing(id) => Self::Missing(id.clone()),
        }
    }
}

impl From<WaitCommandsOrSubagentsResult> for GetCommandOrSubagentOutputValue {
    fn from(value: WaitCommandsOrSubagentsResult) -> Self {
        Self::MultiResult(value)
    }
}

impl CoordinatorHandle {
    async fn native_query_subscriptions(
        &self,
        actor: EventActor,
        tool: String,
        ids: &[String],
        waiting: bool,
    ) -> Result<
        (
            Vec<Subscription>,
            CancellationToken,
            Option<watch::Receiver<u64>>,
        ),
        CoordinatorError,
    > {
        let owner = actor.clone();
        let waiter = tool.clone();
        let (cancellation, interject) = self
            .call(move |s| {
                let job = s.authenticated_native_tool(&owner, &waiter, false)?;
                Ok((
                    job.cancellation.clone(),
                    owner
                        .agent_id
                        .as_ref()
                        .and_then(|agent| s.native_subagents.get(agent))
                        .map(|child| child.wait_interrupt.subscribe()),
                ))
            })
            .await?;
        let mut subscriptions = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(command) = self.subscribe_command(actor.clone(), id.clone()).await? {
                subscriptions.push(Subscription::Command(Box::new(command)));
                continue;
            }
            let id = id.clone();
            let owner = actor.clone();
            let waiter = tool.clone();
            let child = self
                .call(move |s| {
                    let reachable = s.native_reachable(&owner, &id);
                    if !reachable {
                        return Ok(None);
                    }
                    let Some(child) = s.native_subagents.get_mut(&id) else {
                        return Ok(None);
                    };
                    if waiting && child.phase != NativePhase::Terminal {
                        child.waiters.insert(waiter);
                    }
                    Ok(Some(child.updates.subscribe()))
                })
                .await?;
            subscriptions.push(child.map_or_else(
                || Subscription::Missing(ids[subscriptions.len()].clone()),
                Subscription::Subagent,
            ));
        }
        Ok((subscriptions, cancellation, interject))
    }

    pub async fn get_command_or_subagent_output(
        &self,
        actor: EventActor,
        tool_call_id: String,
        input: GetCommandOrSubagentOutputInput,
    ) -> Result<GetCommandOrSubagentOutputValue, CoordinatorError> {
        let mut ids = Vec::new();
        for id in input.task_ids {
            let id = id.trim();
            if !id.is_empty() && !ids.iter().any(|seen| seen == id) {
                ids.push(id.to_owned());
            }
        }
        if ids.is_empty() {
            return Err(native_invalid("Provide a non-empty task_ids list.".into()));
        }
        if ids.len() > 20 {
            return Err(native_invalid(
                "task_ids exceeds maximum of 20 entries.".into(),
            ));
        }
        let timeout = input.timeout_ms.unwrap_or(0);
        let (subscriptions, cancellation, interject) = self
            .native_query_subscriptions(actor.clone(), tool_call_id.clone(), &ids, timeout > 0)
            .await?;
        let wait =
            wait_subscriptions(&subscriptions, false, timeout, cancellation, interject).await;
        self.complete_native_wait(actor.clone(), &ids, &tool_call_id, wait.is_ok())
            .await?;
        wait?;
        // Watch observations must be refreshed after wait-any too; an admission
        // transition can replace the attempt before the query reply is built.
        let (latest, _, _) = self
            .native_query_subscriptions(actor.clone(), tool_call_id, &ids, false)
            .await?;
        if ids.len() == 1 {
            return match &latest[0] {
                Subscription::Missing(_) => {
                    let commands = self.list_commands(actor.clone()).await?;
                    let owner = actor;
                    let mut known = self
                        .call(move |s| {
                            Ok(s.native_subagents
                                .keys()
                                .filter(|id| s.native_reachable(&owner, id))
                                .cloned()
                                .collect::<Vec<_>>())
                        })
                        .await?;
                    known.extend(commands.into_iter().map(|command| command.result.task_id));
                    known.sort();
                    Ok(GetCommandOrSubagentOutputValue::TaskNotFound(
                        if known.is_empty() {
                            format!("Task {} not found. No background tasks or subagents exist in this session.", ids[0])
                        } else {
                            format!(
                                "Task {} not found. Known task IDs: [{}]",
                                ids[0],
                                known.join(", ")
                            )
                        },
                    ))
                }
                subscription => Ok(GetCommandOrSubagentOutputValue::Result(
                    subscription.result(),
                )),
            };
        }
        Ok(GetCommandOrSubagentOutputValue::MultiResult(aggregate(
            if timeout > 0 { "wait_all" } else { "poll" },
            &latest,
        )))
    }

    pub async fn wait_commands_or_subagents(
        &self,
        actor: EventActor,
        tool_call_id: String,
        input: WaitCommandsOrSubagentsInput,
    ) -> Result<WaitCommandsOrSubagentsResult, CoordinatorError> {
        if input.task_ids.is_empty() {
            return Err(native_invalid("task_ids must not be empty.".into()));
        }
        if input.task_ids.len() > 20 {
            return Err(native_invalid(
                "task_ids exceeds maximum of 20 entries.".into(),
            ));
        }
        let any = input.mode == WaitCommandsOrSubagentsMode::WaitAny;
        let timeout = if any {
            input.timeout_ms.unwrap_or(30_000)
        } else {
            input
                .timeout_ms
                .filter(|value| *value > 0)
                .unwrap_or(30_000)
        };
        let (subscriptions, cancellation, interject) = self
            .native_query_subscriptions(
                actor.clone(),
                tool_call_id.clone(),
                &input.task_ids,
                timeout > 0,
            )
            .await?;
        let wait = wait_subscriptions(&subscriptions, any, timeout, cancellation, interject).await;
        self.complete_native_wait(actor.clone(), &input.task_ids, &tool_call_id, wait.is_ok())
            .await?;
        wait?;
        let (latest, _, _) = self
            .native_query_subscriptions(actor, tool_call_id, &input.task_ids, false)
            .await?;
        Ok(aggregate(
            if any { "wait_any" } else { "wait_all" },
            &latest,
        ))
    }

    async fn complete_native_wait(
        &self,
        actor: EventActor,
        ids: &[String],
        tool: &str,
        observed: bool,
    ) -> Result<(), CoordinatorError> {
        let ids = ids.to_vec();
        let tool = tool.to_owned();
        self.call(move |s| {
            for id in ids {
                if !s.native_reachable(&actor, &id) {
                    continue;
                }
                let Some(child) = s.native_subagents.get_mut(&id) else {
                    continue;
                };
                child.waiters.remove(&tool);
                if observed && child.phase == NativePhase::Terminal {
                    child.consumed = true;
                    child.buffered_for = None;
                }
            }
            Ok(())
        })
        .await
    }

    pub async fn kill_command_or_subagent(
        &self,
        actor: EventActor,
        tool_call_id: String,
        input: KillCommandOrSubagentInput,
    ) -> Result<KillCommandOrSubagentValue, CoordinatorError> {
        let owner = actor.clone();
        self.call(move |s| {
            s.authenticated_native_tool(&owner, &tool_call_id, false)
                .map(|_| ())
        })
        .await?;
        if let Some(result) = self
            .kill_command(actor.clone(), input.task_id.clone())
            .await?
        {
            return Ok(KillCommandOrSubagentValue::Result(result));
        }
        self.call(move |s| {
            if !s.native_reachable(&actor, &input.task_id) {
                return Ok(KillCommandOrSubagentValue::TaskNotFound(format!(
                    "Task {} not found",
                    input.task_id
                )));
            }
            let Some(child) = s.native_subagents.get(&input.task_id) else {
                return Ok(KillCommandOrSubagentValue::TaskNotFound(format!(
                    "Task {} not found",
                    input.task_id
                )));
            };
            let terminal = child.phase == NativePhase::Terminal;
            let status = child.updates.borrow().result.status.clone();
            if !terminal {
                s.cancel_native_subagent(&input.task_id, "Subagent explicitly killed", true)?;
            }
            Ok(KillCommandOrSubagentValue::Result(
                KillCommandOrSubagentResult {
                    task_id: input.task_id,
                    outcome: if terminal { "already_exited" } else { "killed" }.into(),
                    message: if terminal {
                        format!("Subagent already {status}")
                    } else {
                        "Subagent cancellation initiated".into()
                    },
                },
            ))
        })
        .await
    }
}

async fn wait_subscriptions(
    subscriptions: &[Subscription],
    any: bool,
    timeout: u64,
    cancellation: CancellationToken,
    interject: Option<watch::Receiver<u64>>,
) -> Result<(), CoordinatorError> {
    if timeout == 0 {
        return Ok(());
    }
    if any
        && subscriptions.iter().any(|subscription| {
            !subscription.pending() && !matches!(subscription, Subscription::Missing(_))
        })
    {
        return Ok(());
    }
    let mut waits = tokio::task::JoinSet::new();
    for subscription in subscriptions
        .iter()
        .filter(|subscription| subscription.pending())
    {
        let subscription = subscription.cloned();
        waits.spawn(subscription.wait_terminal());
    }
    if waits.is_empty() {
        return Ok(());
    }
    let pending = async {
        while let Some(result) = waits.join_next().await {
            result.map_err(|_| CoordinatorError::Closed)??;
            if any {
                return Ok(());
            }
        }
        Ok::<(), CoordinatorError>(())
    };
    let interrupted = async {
        match interject {
            Some(mut interject) => {
                let _ = interject.changed().await;
            }
            None => std::future::pending::<()>().await,
        }
    };
    let timeout = timeout.min(milliseconds("GROK_MAX_WAIT_BLOCK_MS", 3_600_000));
    tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(CoordinatorError::Cancelled("wait cancelled".into())),
        () = interrupted => Err(CoordinatorError::Cancelled("wait interrupted by a subagent interject".into())),
        result = tokio::time::timeout(Duration::from_millis(timeout), pending) => {
            match result { Ok(result) => result, Err(_) => Ok(()) }
        }
    }
}

fn aggregate(mode: &str, subscriptions: &[Subscription]) -> GetCommandOrSubagentOutputResults {
    let results: Vec<_> = subscriptions.iter().map(Subscription::result).collect();
    let completed = subscriptions
        .iter()
        .filter(|subscription| {
            !subscription.pending() && !matches!(subscription, Subscription::Missing(_))
        })
        .count();
    GetCommandOrSubagentOutputResults {
        mode: mode.into(),
        summary: format!("{completed}/{} tasks completed ({mode})", results.len()),
        results,
    }
}
