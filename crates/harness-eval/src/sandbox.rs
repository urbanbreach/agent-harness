//! Fresh QuickJS contexts expose only a JSON tool bridge and output events.
use crate::{cell::Cell, session::Inner, Result};
use rquickjs::{context::EvalOptions, CatchResultExt, Context, Function, Promise, Runtime};
use serde_json::{json, Value};
use std::sync::Arc;

pub(crate) async fn run(session: Arc<Inner>, cell: Arc<Cell>) -> Result<()> {
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let runtime = Runtime::new()?;
        runtime.set_memory_limit(session.options.settings.sandbox.memory_limit_mb * 1024 * 1024);
        runtime.set_max_stack_size(512 * 1024);
        let cancellation = cell.cancel.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || cancellation.is_cancelled())));
        let context = Context::full(&runtime)?;
        context.with(|ctx| -> Result<()> {
            let bridge_session = Arc::clone(&session);
            let bridge_cell = Arc::clone(&cell);
            let bridge_handle = handle.clone();
            ctx.globals().set(
                "__host",
                Function::new(
                    ctx.clone(),
                    move |operation: String, encoded: String| -> String {
                        let result = bridge(
                            &bridge_session,
                            &bridge_cell,
                            &bridge_handle,
                            &operation,
                            &encoded,
                        );
                        match result {
                            Ok(value) => json!({"value":value}).to_string(),
                            Err(error) => json!({"error":error.to_string()}).to_string(),
                        }
                    },
                )?,
            )?;
            let output_cell = Arc::clone(&cell);
            let output_handle = handle.clone();
            ctx.globals().set(
                "__emit",
                Function::new(
                    ctx.clone(),
                    move |encoded: String| -> rquickjs::Result<()> {
                        if encoded.len() > 32 * 1024 * 1024 {
                            return Err(rquickjs::Error::Unknown);
                        }
                        let event =
                            serde_json::from_str(&encoded).map_err(|_| rquickjs::Error::Unknown)?;
                        output_handle
                            .block_on(output_cell.accept(event, true))
                            .map_err(|_| rquickjs::Error::Unknown)
                    },
                )?,
            )?;
            ctx.eval::<(), _>(include_str!("sandbox.js"))
                .catch(&ctx)
                .map_err(|error| error.to_string())?;
            let mut options = EvalOptions::default();
            options.promise = true;
            options.filename = Some("<isolated-eval>".into());
            let promise: Promise = ctx
                .eval_with_options(
                    cell.args["code"].as_str().ok_or("missing isolated code")?,
                    options,
                )
                .catch(&ctx)
                .map_err(|error| error.to_string())?;
            let value: rquickjs::Value = promise
                .finish()
                .catch(&ctx)
                .map_err(|error| error.to_string())?;
            if !value.is_undefined()
                && let Some(encoded) = ctx.json_stringify(value)?
            {
                let text = encoded.to_string()?;
                handle.block_on(cell.accept(
                    json!({"type":"text","stream":"stdout","data":format!("{text}\n")}),
                    true,
                ))?;
            }
            Ok(())
        })
    })
    .await?
}

fn bridge(
    session: &Inner,
    cell: &Cell,
    handle: &tokio::runtime::Handle,
    operation: &str,
    encoded: &str,
) -> Result<Value> {
    if encoded.len() > 1024 * 1024 {
        return Err("isolated host arguments exceed 1 MiB".into());
    }
    if !matches!(operation, "tool" | "schema") {
        return Err("operation unavailable in isolated eval".into());
    }
    let args: Value = serde_json::from_str(encoded)?;
    handle.block_on(crate::helpers::dispatch(
        session,
        cell,
        &json!({"operation":operation,"args":args}),
    ))
}
