use crate::{
    cell::{epoch_ms, Cell},
    kernel::Kernel,
    session::Inner,
    Result,
};
use serde_json::{json, Value};
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::time::Instant;

pub(crate) async fn execute(session: Arc<Inner>, cell: Arc<Cell>, predecessor: Option<Arc<Cell>>) {
    let outcome = monitor(&session, &cell, predecessor).await;
    session.retire_calls(&cell).await;
    let result = {
        let mut state = cell.state.lock().await;
        state.completed = true;
        state.status = if state.status == "error" {
            "error"
        } else if cell.cancel.is_cancelled() {
            "cancelled"
        } else if outcome.is_err() {
            "error"
        } else {
            "complete"
        };
        if let Err(error) = outcome {
            // File failures are surfaced too; an output artifact is never reported as complete after a failed write.
            if state.output.push(&format!("{error}\n"), false).is_err() {
                state.status = "error";
            }
        }
        let images = state
            .images
            .iter()
            .enumerate()
            .map(|(index, image)| {
                format!(
                    "display image {}: [{}]\n",
                    index + 1,
                    image["mimeType"].as_str().unwrap_or("image")
                )
            })
            .collect::<Vec<_>>();
        for description in images {
            if state.output.push(&description, true).is_err() {
                state.status = "error";
            }
        }
        if state.images_elided > 0 || state.json_elided > 0 {
            let notice = format!(
                "[{} display image(s) and {} JSON output(s) elided beyond per-cell caps]\n",
                state.images_elided, state.json_elided
            );
            if state.output.push(&notice, true).is_err() {
                state.status = "error";
            }
        }
        let result = cell.snapshot(&state, false);
        state.result = Some(result.clone());
        if state.detached {
            session.detached.fetch_sub(1, Ordering::AcqRel);
        }
        result
    };
    cell.finished.notify_waiters();
    let execution = crate::metadata::execution(&cell, &*cell.state.lock().await);
    let rpc_execution = crate::metadata::rpc(execution.clone());
    let _ = cell
        .event(json!({"type":"settled","result":result,"execution":execution,"rpcExecution":rpc_execution}))
        .await;
    if !cell.state.lock().await.detached {
        let _ = cell.event(json!({"type":"result","result":result})).await;
    }
    session.retain(&cell.id).await;
}

async fn monitor(
    session: &Arc<Inner>,
    cell: &Arc<Cell>,
    predecessor: Option<Arc<Cell>>,
) -> Result<()> {
    let settings = &session.options.settings;
    let mut budget = cell.args["timeout"]
        .as_f64()
        .map(Duration::from_secs_f64)
        .unwrap_or_else(|| Duration::from_secs(settings.run_budget_seconds));
    if cell.args["isolate"] == true {
        budget = budget.min(Duration::from_secs(settings.sandbox.timeout_seconds));
    }
    let hard_limit = Duration::from_secs(settings.hard_limit_seconds).max(budget);
    let detach_after = Duration::from_secs(
        settings
            .cell_timeout_seconds
            .min(settings.foreground_window_seconds),
    );
    let foreground = Duration::from_secs(settings.foreground_window_seconds);
    let can_detach = cell.args["on_timeout"] == "detach"
        || cell.interactive && cell.args["on_timeout"] != "error";
    let drive = drive(session, cell, predecessor);
    tokio::pin!(drive);
    let mut ticker = tokio::time::interval(Duration::from_millis(100));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut timeout = None;
    loop {
        tokio::select! {
            result = &mut drive => return timeout.map_or(result, |reason: String| Err(reason.into())),
            _ = ticker.tick() => {
                let mut state = cell.state.lock().await;
                if let Some(start) = state.running_since { state.duration = start.elapsed(); }
                let own_time = state.own_time();
                let foreground_time = cell.created.elapsed().saturating_sub(state.blocked_time + state.blocked_since.map_or(Duration::ZERO, |start| start.elapsed()));
                if timeout.is_none() && (own_time >= budget || cell.created.elapsed() >= hard_limit) {
                    timeout = Some(if own_time >= budget { "eval execution budget exceeded" } else { "eval wall-clock limit exceeded" }.to_owned());
                    state.status = "error";
                    cell.cancel.cancel();
                }
                if !state.detached && !cell.cancel.is_cancelled() && can_detach
                    && (foreground_time >= detach_after || cell.created.elapsed() >= foreground || cell.steer.is_cancelled()) {
                    let claimed = session.detached.fetch_update(Ordering::AcqRel, Ordering::Acquire,
                        |count| (count < settings.max_detached_cells).then_some(count + 1));
                    if claimed.is_ok() {
                        state.detached = true;
                        if state.status == "running" { state.status = "detached"; }
                        let receipt = cell.snapshot(&state, true);
                        drop(state);
                        cell.event(json!({"type":"result","result":receipt})).await?;
                        continue;
                    }
                    if cell.created.elapsed() >= foreground {
                        timeout = Some("eval detached-cell capacity reached".to_owned()); state.status = "error"; cell.cancel.cancel();
                    }
                }
                let update = cell.snapshot(&state, true);
                drop(state);
                cell.event(json!({"type":"update","result":update})).await?;
            }
        }
    }
}

async fn drive(
    session: &Arc<Inner>,
    cell: &Arc<Cell>,
    predecessor: Option<Arc<Cell>>,
) -> Result<()> {
    if let Some(previous) = predecessor {
        tokio::select! {
            () = cell.cancel.cancelled() => return Err("queued eval cell cancelled".into()),
            () = previous.wait_terminal() => {},
        }
    }
    let slot = session
        .kernels
        .get(&cell.language)
        .ok_or("eval kernel is unavailable")?;
    let mut slot = tokio::select! {
        () = cell.cancel.cancelled() => return Err("queued eval cell cancelled".into()),
        slot = slot.lock() => slot,
    };
    if cell.args["isolate"] == true {
        started(cell, json!({"name":"QuickJS","isolated":true})).await;
        return crate::sandbox::run(Arc::clone(session), Arc::clone(cell)).await;
    }
    let kernel = prepare(&mut slot, session, cell).await?;
    session
        .controls
        .lock()
        .await
        .insert(cell.language.clone(), kernel.control.clone());
    started(cell, kernel.runtime.clone()).await;
    let mut result = run(kernel, session, cell).await;
    if cell.cancel.is_cancelled()
        && let Err(error) = interruption_notice(cell, result.is_ok()).await
    {
        result = Err(error);
    }
    let recycle = finish_memory(kernel, session, cell, result.is_err()).await;
    if let Some(mut kernel) = slot.take_if(|_| result.is_err() || recycle) {
        kernel.stop().await;
    }
    result
}

async fn interruption_notice(cell: &Cell, preserved: bool) -> Result<()> {
    let note = if preserved {
        "[Kernel interrupted; existing variables are preserved.]\n"
    } else {
        "[Kernel reset after interruption; earlier variables are lost.]\n"
    };
    cell.state.lock().await.output.push(note, false)
}

async fn started(cell: &Cell, runtime: Value) {
    let mut state = cell.state.lock().await;
    state.status = if state.detached {
        "detached"
    } else {
        "running"
    };
    state.started_at = Some(epoch_ms());
    state.running_since = Some(Instant::now());
    state.queued.clear();
    state.runtime = runtime;
}

async fn prepare<'a>(
    slot: &'a mut Option<Kernel>,
    session: &Inner,
    cell: &Cell,
) -> Result<&'a mut Kernel> {
    if let Some(mut kernel) =
        slot.take_if(|kernel| cell.args["reset"] == true || !kernel.is_alive())
    {
        kernel.stop().await;
        if let Some(policy) = session.memory.get(&cell.language) {
            *policy.lock().await = Default::default();
        }
    }
    if slot.is_none() {
        *slot = Some(tokio::select! {
            () = cell.cancel.cancelled() => return Err("eval startup cancelled".into()),
            kernel = Kernel::start(&session.options, &cell.language) => kernel?,
        });
    }
    slot.as_mut()
        .ok_or_else(|| "eval kernel did not start".into())
}

async fn run(kernel: &mut Kernel, session: &Arc<Inner>, cell: &Arc<Cell>) -> Result<()> {
    let preludes: Vec<_> = cell
        .tools
        .lock()
        .await
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            tool.get("kernelPrelude")
                .filter(|prelude| prelude.is_object())
                .cloned()
        })
        .collect();
    kernel
        .send(json!({"type":"run","id":cell.id,"code":cell.args["code"],"preludes":preludes,"tools":*cell.tools.lock().await}))
        .await?;
    let mut calls = tokio::task::JoinSet::new();
    let mut interrupted = false;
    let mut interrupt_deadline = Instant::now() + Duration::from_secs(86400);
    let result = async { loop {
        tokio::select! {
            () = cell.cancel.cancelled(), if !interrupted => {
                interrupted = true;
                kernel.interrupt(&cell.id).await?;
                interrupt_deadline = Instant::now() + Duration::from_secs(2);
            }
            () = tokio::time::sleep_until(interrupt_deadline), if interrupted => return Err("eval kernel did not acknowledge interruption; kernel reset".into()),
            event = kernel.receive() => {
                let event = event?;
                if event["cellId"] != cell.id { continue; }
                match event["type"].as_str() {
                    Some("transport-error") => return Err(event["error"].as_str().unwrap_or("eval transport failed").to_owned().into()),
                    Some("result") => {
                        let mut state = cell.state.lock().await;
                        if let Some(duration) = event["durationMs"].as_u64() { state.duration = Duration::from_millis(duration); }
                        if let Some(memory) = event.get("memory") { state.memory = Some(memory.clone()); }
                        if event["ok"] == true {
                            if let Some(value) = event["valueRepr"].as_str().filter(|value| !value.is_empty()) { state.output.push(&format!("{value}\n"), false)?; }
                        } else {
                            if !cell.cancel.is_cancelled() { state.status = "error"; }
                            state.output.push(&format!("{}\n", event["error"]["message"].as_str().unwrap_or("eval failed")), false)?;
                        }
                        // A user exception is a cell failure, not a transport failure; retain the kernel.
                        return Ok(());
                    }
                    Some("call") => {
                        let (session, cell) = (Arc::clone(session), Arc::clone(cell));
                        calls.spawn(async move { crate::helpers::reply(&session, &cell, &event).await });
                    }
                    _ => cell.accept(event, session.options.settings.status_events).await?,
                }
            }
            reply = calls.join_next(), if !calls.is_empty() => {
                kernel.send(reply.ok_or("eval helper stopped")??).await?;
            }
        }
    }}.await;
    calls.abort_all();
    while calls.join_next().await.is_some() {}
    result
}

async fn finish_memory(kernel: &Kernel, session: &Inner, cell: &Cell, failed: bool) -> bool {
    let footprint = if matches!(cell.language.as_str(), "rb" | "jl") {
        kernel.footprint().await
    } else {
        None
    };
    let mut recycle = false;
    if let Some(policy) = session.memory.get(&cell.language) {
        let mut policy = policy.lock().await;
        {
            let mut state = cell.state.lock().await;
            if footprint.is_some() {
                state.memory = footprint;
            }
            if let Some(report) = &mut state.memory {
                policy.annotate(&cell.language, &session.options.settings.memory, report);
            }
        }
        if policy.pending.is_some() {
            let cells: Vec<_> = session
                .cells
                .lock()
                .await
                .values()
                .filter(|other| other.language == cell.language && other.id != cell.id)
                .cloned()
                .collect();
            let mut queued = false;
            for other in cells {
                queued |= !other.state.lock().await.completed;
            }
            if !queued {
                policy.recycle();
                recycle = true;
            }
        }
        if failed {
            *policy = Default::default();
        }
    }
    recycle
}
