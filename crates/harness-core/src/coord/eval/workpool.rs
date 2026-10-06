//! Pool membership limits the existing native subagent admission queue.
use super::*;
use serde_json::json;
mod request;
type Submissions = Vec<(String, String, Value)>;
type Changes = (String, Submissions, Vec<String>);

pub(in crate::coord) struct Pool {
    owner: String,
    run: String,
    name: String,
    spec: Value,
    width: usize,
    closed: bool,
    cancelled: bool,
    notify: bool,
    notified: bool,
    children: Vec<String>,
    keys: BTreeMap<String, String>,
    errors: BTreeMap<String, String>,
    results: BTreeMap<String, Value>,
    pending: BTreeMap<String, String>,
    cancelling: Option<String>,
}

impl CoordinatorHandle {
    pub async fn eval_workpool(
        &self,
        parent: String,
        args: Value,
        cancel: CancellationToken,
    ) -> Result<Value, CoordinatorError> {
        let op = args["op"].as_str().unwrap_or_default().to_owned();
        let parent_for_state = parent.clone();
        let (id, submissions, cancellations) = self
            .call(move |s| s.prepare_eval_pool(&parent_for_state, args))
            .await?;
        for (child, _, spec) in submissions {
            let result = self
                .execute_nested_tool_with_id(
                    parent.clone(),
                    "spawn_subagent",
                    spec,
                    Some(format!("{parent}-pool-{child}")),
                    Some(cancel.clone()),
                )
                .await;
            let error = match result {
                Err(error) => Some(error.to_string()),
                Ok(result) if result.is_error() => Some(result.display_text),
                _ => None,
            };
            if let Some(message) = error {
                let pool_id = id.clone();
                self.call(move |s| {
                    s.eval_pools
                        .get_mut(&pool_id)
                        .ok_or_else(|| invalid("workpool is no longer available"))?
                        .errors
                        .insert(child, message);
                    Ok(())
                })
                .await?;
            }
        }
        let mut cancel_error = None;
        for child in cancellations {
            let result = self
                .execute_nested_tool_with_id(
                    parent.clone(),
                    "kill_command_or_subagent",
                    json!({"task_id":child}),
                    Some(format!("{parent}-cancel-{child}")),
                    Some(cancel.clone()),
                )
                .await;
            match result {
                Err(error) => {
                    cancel_error.get_or_insert(error);
                }
                Ok(result) if result.is_error() => {
                    cancel_error.get_or_insert_with(|| invalid(&result.display_text));
                }
                _ => {}
            }
        }
        if let Some(error) = cancel_error {
            let pool_id = id.clone();
            self.call(move |s| {
                if let Some(pool) = s.eval_pools.get_mut(&pool_id) {
                    pool.cancelled = false;
                }
                s.pump_native_subagents()
            })
            .await?;
            return Err(error);
        }
        self.call(move |s| {
            s.check_eval(&parent)?;
            if op == "cancel" { s.eval_pools.get_mut(&id).ok_or_else(|| invalid("workpool is no longer available"))?.cancelling = None; }
            s.publish_eval_pool_notifications()?;
            let pool = s.eval_pools.get(&id).ok_or_else(|| invalid("workpool is no longer available"))?;
            let items = pool.children.iter().map(|child| {
                let mut snapshot = if let Some(error) = pool.errors.get(child) { json!({"id":child,"status":"failed","terminal":true,"error":error}) }
                else if let Some(result) = pool.results.get(child) { result.clone() }
                else { s.native_subagents.get(child).map_or_else(|| json!({"id":child,"status":"queued","terminal":false}), |child| child.pool_snapshot()) };
                snapshot["key"] = json!(pool.keys.get(child));
                snapshot["run_epoch"] = json!(0);
                snapshot
            }).collect::<Vec<_>>();
            Ok(json!({"pool_id":id,"op":op,"name":pool.name,"width":pool.width,"closed":pool.closed,"cancelled":pool.cancelled,"items":items}))
        }).await
    }
}

impl Runtime {
    fn prepare_eval_pool(
        &mut self,
        parent: &str,
        args: Value,
    ) -> Result<Changes, CoordinatorError> {
        self.check_eval(parent)?;
        let owner = self.running[parent]
            .actor
            .agent_id
            .clone()
            .ok_or_else(|| invalid("workpools require an agent"))?;
        let run = self.info()?.run_id.to_string();
        let op = request::operation(&args)?;
        if op == "create" {
            let id = self.create_eval_pool(parent, owner, run, &args)?;
            return Ok((id, Vec::new(), Vec::new()));
        }
        let id = args["pool_id"]
            .as_str()
            .ok_or_else(|| invalid("pool_id is required"))?
            .to_owned();
        let pool = self
            .eval_pools
            .get_mut(&id)
            .filter(|p| p.owner == owner && p.run == run)
            .ok_or_else(|| invalid("workpool is not owned by this agent session"))?;
        let mut submissions = Vec::new();
        let mut cancellations = Vec::new();
        match op {
            "push" => {
                if pool.closed {
                    return Err(invalid("workpool is closed"));
                }
                submissions = request::items(pool, &args["items"])?;
                pool.pending.extend(
                    submissions
                        .iter()
                        .map(|(id, _, _)| (id.clone(), parent.into())),
                );
                pool.children
                    .extend(submissions.iter().map(|(id, _, _)| id.clone()));
                pool.keys.extend(
                    submissions
                        .iter()
                        .map(|(id, key, _)| (id.clone(), key.clone())),
                );
            }
            "close" => pool.closed = true,
            "cancel" => {
                pool.closed = true;
                pool.cancelled = true;
                pool.cancelling = Some(parent.into());
                cancellations = pool
                    .children
                    .iter()
                    .filter(|id| {
                        self.native_subagents
                            .get(*id)
                            .is_some_and(|child| !child.is_terminal())
                    })
                    .cloned()
                    .collect();
            }
            _ => {}
        }
        Ok((id, submissions, cancellations))
    }

    fn create_eval_pool(
        &mut self,
        parent: &str,
        owner: String,
        run: String,
        args: &Value,
    ) -> Result<String, CoordinatorError> {
        let name = args["name"]
            .as_str()
            .filter(|name| !name.trim().is_empty() && name.len() <= 128)
            .ok_or_else(|| invalid("workpool name must contain 1-128 bytes"))?;
        let spec = request::spec(args)?;
        let width = match args.get("width") {
            Some(value) => value
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| invalid("workpool width must be an integer"))?,
            None => 4.min(self.config.subagents.max_concurrent).max(1),
        };
        if !(1..=256).contains(&width) {
            return Err(invalid("workpool width must be 1-256"));
        }
        if let Some((id, pool)) = self
            .eval_pools
            .iter()
            .find(|(_, p)| p.owner == owner && p.run == run && p.name == name)
        {
            if pool.spec != spec || pool.width != width {
                return Err(invalid("workpool name already belongs to a different pool"));
            }
            return Ok(id.clone());
        }
        if self
            .eval_pools
            .values()
            .filter(|pool| pool.owner == owner && pool.run == run)
            .count()
            >= 64
        {
            return Err(invalid("workpool capacity reached (64 per agent session)"));
        }
        let notify = self.running[parent].parent.is_some();
        if notify {
            self.check_turn_capacity(&owner)?;
        }
        let id = format!("wp_{}", uuid::Uuid::now_v7().simple());
        self.eval_pools.insert(
            id.clone(),
            Pool {
                owner,
                run,
                name: name.into(),
                spec,
                width,
                closed: false,
                cancelled: false,
                notify,
                notified: false,
                children: Vec::new(),
                keys: BTreeMap::new(),
                errors: BTreeMap::new(),
                results: BTreeMap::new(),
                pending: BTreeMap::new(),
                cancelling: None,
            },
        );
        Ok(id)
    }

    pub(in crate::coord) fn finish_eval_pool_submission(
        &mut self,
        parent: &str,
    ) -> Result<(), CoordinatorError> {
        for pool in self.eval_pools.values_mut() {
            let abandoned = pool
                .pending
                .iter()
                .filter(|(_, job)| *job == parent)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for id in abandoned {
                pool.pending.remove(&id);
                if !self.native_subagents.contains_key(&id) && !pool.results.contains_key(&id) {
                    pool.errors
                        .entry(id)
                        .or_insert_with(|| "submission interrupted before admission".into());
                }
            }
            if pool.cancelling.as_deref() == Some(parent) {
                pool.cancelling = None;
                pool.cancelled = false;
            }
        }
        self.pump_native_subagents()?;
        self.publish_eval_pool_notifications()
    }

    pub(in crate::coord) fn eval_pool_notification_reservations(&self, owner: &str) -> usize {
        self.eval_pools
            .values()
            .filter(|p| p.owner == owner && p.notify && !p.notified)
            .count()
    }

    pub(in crate::coord) fn is_eval_pool_child(&self, id: &str) -> bool {
        self.eval_pools
            .values()
            .any(|p| p.children.iter().any(|child| child == id))
    }

    pub(in crate::coord) fn eval_pool_completed_child(
        &mut self,
        id: &str,
    ) -> Result<bool, CoordinatorError> {
        let Some((pool_id, _)) = self
            .eval_pools
            .iter()
            .find(|(_, pool)| pool.children.iter().any(|child| child == id))
        else {
            return Ok(false);
        };
        let pool_id = pool_id.clone();
        let child = self
            .native_subagents
            .get(id)
            .ok_or_else(|| invalid("pool child is missing"))?;
        let mut snapshot = child.pool_snapshot();
        snapshot["output"] = json!(self
            .redactor
            .redact_text(snapshot["output"].as_str().unwrap_or_default())
            .chars()
            .take(1024)
            .collect::<String>());
        let agent = &self.agents[id];
        self.emit(
            EventActor::new(ActorKind::Worker, Some(id.into())),
            agent.attempt.clone(),
            EventV1::NativeSubagentReceipt(Box::new(
                super::super::public_subagents::NativeSubagentReceipt {
                    payload_version: 1,
                    child_id: id.into(),
                    attempt_id: agent.attempt.clone(),
                    generation: agent.generation,
                    kind: "notification_consumed".into(),
                    waiter_id: Some(pool_id.clone()),
                },
            )),
        )?;
        if let Some(pool) = self.eval_pools.get_mut(&pool_id) {
            pool.results.insert(id.into(), snapshot);
        }
        Ok(true)
    }

    pub(in crate::coord) fn publish_eval_pool_notifications(
        &mut self,
    ) -> Result<(), CoordinatorError> {
        if self.stopping.is_some() || self.rewind.is_some() || self.fault.is_some() {
            return Ok(());
        }
        let ready = self
            .eval_pools
            .iter()
            .filter(|(_, pool)| {
                pool.notify
                    && !pool.notified
                    && pool.closed
                    && pool.children.iter().all(|child| {
                        pool.results.contains_key(child) || pool.errors.contains_key(child)
                    })
            })
            .map(|(id, pool)| (id.clone(), pool.owner.clone()))
            .collect::<Vec<_>>();
        for (id, owner) in ready {
            if self.killed_agents.contains(&owner) || self.stopped_sessions.contains(&owner) {
                continue;
            }
            let pool = &self.eval_pools[&id];
            let outcomes = pool
                .children
                .iter()
                .map(|child| {
                    json!({"key":pool.keys.get(child),"id":child,
                "result":pool.results.get(child),"error":pool.errors.get(child)})
                })
                .collect::<Vec<_>>();
            let prompt = super::super::prompt::Prompt::from(format!("Workpool {} ({id}) has settled. Output previews are limited to 1024 characters per item; use output() for full results.\n{}", pool.name, json!(outcomes)));
            self.eval_pools
                .get_mut(&id)
                .ok_or_else(|| invalid("workpool is no longer available"))?
                .notified = true;
            if let Err(error) = self.queue_turn(
                EventActor::new(ActorKind::Worker, Some(owner.clone())),
                &owner,
                prompt,
                None,
                None,
                None,
            ) {
                self.eval_pools
                    .get_mut(&id)
                    .ok_or_else(|| invalid("workpool is no longer available"))?
                    .notified = false;
                return Err(error);
            }
        }
        Ok(())
    }

    pub(in crate::coord) fn eval_pool_has_capacity(&self, id: &str) -> bool {
        self.eval_pools
            .values()
            .find(|pool| pool.children.iter().any(|child| child == id))
            .is_none_or(|pool| {
                !pool.cancelled
                    && pool
                        .children
                        .iter()
                        .filter(|child| {
                            self.native_subagents
                                .get(*child)
                                .is_some_and(|child| child.occupies_slot())
                        })
                        .count()
                        < pool.width
            })
    }

    pub(in crate::coord) fn check_eval_pool_child(
        &self,
        id: &str,
        owner: &str,
    ) -> Result<(), CoordinatorError> {
        if let Some(pool) = self
            .eval_pools
            .values()
            .find(|pool| pool.children.iter().any(|child| child == id))
            && (pool.owner != owner || pool.cancelled)
        {
            return Err(CoordinatorError::PermissionDenied(
                "workpool child belongs to another owner or was cancelled".into(),
            ));
        }
        Ok(())
    }
}

fn invalid(message: &str) -> CoordinatorError {
    CoordinatorError::Invalid(message.into())
}
