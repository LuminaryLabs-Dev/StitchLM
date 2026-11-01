#![cfg(target_arch = "wasm32")]
use stitch_platform::web;
use stitch_ui as ui;
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::{spawn_local, JsFuture};
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    ui::click("refresh", |_| {
        spawn_local(async {
            if let Err(e) = refresh().await {
                ui::error(e)
            }
        })
    })?;
    ui::click("generate", |_| {
        spawn_local(async {
            if let Err(e) = generate().await {
                ui::error(e)
            }
        })
    })?;
    ui::click("cancel", |_| {
        spawn_local(async {
            let result = async {
                let token = ui::value("access")?;
                web::text("/api/cancel", &token, "POST", None).await?;
                let current = ui::element("status")?.text_content().unwrap_or_default();
                if current.starts_with("Generating")
                    || current.starts_with("token |")
                    || current.starts_with("Scheduling")
                {
                    ui::text("status", "Cancellation requested");
                }
                Ok::<_, JsValue>(())
            }
            .await;
            if let Err(e) = result {
                ui::error(e)
            }
        })
    })?;
    ui::click("remove", |_| {
        spawn_local(async {
            let result = async {
                let token = ui::value("access")?;
                let id = ui::value("remove-id")?;
                if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(JsValue::from_str(
                        "copy the connector ID from device status",
                    ));
                }
                web::text(
                    &format!("/api/connectors/{id}/remove"),
                    &token,
                    "POST",
                    Some("{}"),
                )
                .await?;
                refresh().await
            }
            .await;
            if let Err(e) = result {
                ui::error(e)
            }
        })
    })?;
    ui::click("run-shards", |_| {
        spawn_local(async {
            if let Err(e) = project().await {
                ui::error(e)
            }
        })
    })?;
    ui::click("restore-shards", |_| {
        spawn_local(async {
            if let Err(e) = restore().await {
                ui::error(e)
            }
        })
    })?;
    ui::text(
        "status",
        "Enter the host access code printed by the native host.",
    );
    Ok(())
}
async fn refresh() -> Result<(), JsValue> {
    let token = ui::value("access")?;
    let value = web::text("/api/status", &token, "GET", None).await?;
    let status: stitch_core::HostStatus = serde_json::from_str(&value).map_err(js)?;
    let mut summary = format!(
        "{} | context {} | {} connectors\n",
        status.model.id,
        status.context_limit,
        status.connectors.len()
    );
    for c in status.connectors {
        summary.push_str(&format!(
            "{} ({}) blocks {}..{} | {} | {} jobs | ID:{}\n",
            c.name,
            c.kind,
            c.range.start,
            c.range.end,
            if c.online && c.ready {
                "ready"
            } else {
                "not ready"
            },
            c.completed_jobs,
            c.id
        ));
    }
    ui::text("devices", &summary);
    ui::text("status", "Host connected");
    Ok(())
}
async fn generate() -> Result<(), JsValue> {
    let token = ui::value("access")?;
    let request = stitch_core::GenerateRequest {
        prompt: ui::value("prompt")?,
        max_tokens: ui::value("tokens")?.parse().map_err(js)?,
        local_only: false,
        raw_prompt: ui::value("prompt-mode")? == "raw",
        split_axis: Some(ui::value("generation-axis")?),
        preferred_chunk: None,
        connector_count: None,
        working_memory_bytes: None,
    };
    ui::text("output", "");
    ui::text("status", "Generating…");
    let body = serde_json::to_string(&request).map_err(js)?;
    let response = web::request("/api/generate", &token, "POST", Some(&body)).await?;
    let reader = response
        .body()
        .ok_or_else(|| JsValue::from_str("stream unavailable"))?
        .get_reader()
        .dyn_into::<web_sys::ReadableStreamDefaultReader>()?;
    let mut pending = Vec::new();
    loop {
        let chunk = JsFuture::from(reader.read()).await?;
        if js_sys::Reflect::get(&chunk, &"done".into())?.as_bool() == Some(true) {
            break;
        }
        let bytes =
            js_sys::Uint8Array::new(&js_sys::Reflect::get(&chunk, &"value".into())?).to_vec();
        pending.extend(bytes);
        while let Some(end) = pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            let event: stitch_core::GenerationEvent = serde_json::from_slice(&line).map_err(js)?;
            ui::text("output", &event.text);
            ui::text(
                "status",
                &format!(
                    "{} | {} tokens | {:.2}s | {} {}",
                    event.kind,
                    event.tokens,
                    event.elapsed_ms / 1000.,
                    if event.remote_shards > 0 {
                        event.remote_shards
                    } else {
                        event.remote_blocks
                    },
                    if event.remote_shards > 0 {
                        "remote matrix shards"
                    } else {
                        "remote block executions"
                    }
                ),
            );
        }
    }
    if !pending.is_empty() {
        return Err(JsValue::from_str("incomplete generation stream"));
    }
    Ok(())
}
fn js(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

thread_local! {static LAST_CHECKPOINT:std::cell::RefCell<Option<String>>=const{std::cell::RefCell::new(None)};}
async fn project() -> Result<(), JsValue> {
    let token = ui::value("access")?;
    let resource = ui::value("shard-resource")?;
    let count: usize = ui::value("shard-count")?.parse().map_err(js)?;
    let budget: u64 = ui::value("shard-memory")?.parse().map_err(js)?;
    let axis = ui::value("shard-axis")?;
    let (target, input, chunk) = if resource == "dense-demo" {
        (
            serde_json::json!({"kind":"dense","rows":16,"columns":16,"weights":(0..256).map(|i|(i%19)as f32/16.).collect::<Vec<_>>()}),
            vec![1f32; 16],
            4u64,
        )
    } else {
        (
            serde_json::json!({"kind":"registered","resource_id":resource}),
            (0..5120)
                .map(|i| ((i % 31) as f32 - 15.) / 32.)
                .collect::<Vec<_>>(),
            if axis == "columns" { 1280 } else { 1024 },
        )
    };
    let request = serde_json::json!({"target":target,"input":input,"connector_count":count,"memory_limit_bytes":1000000000u64,"max_working_bytes":budget.checked_mul(1000000).ok_or_else(||JsValue::from_str("budget overflow"))?,"preferred_chunk":chunk,"split_axis":axis});
    ui::text("status", "Scheduling bounded shard commands…");
    SHARDS_ACTIVE.with(|v| v.set(true));
    spawn_local(poll_shards(token.clone()));
    let response = web::text(
        "/api/shards/project",
        &token,
        "POST",
        Some(&request.to_string()),
    )
    .await;
    SHARDS_ACTIVE.with(|v| v.set(false));
    let response = response?;
    let report: serde_json::Value = serde_json::from_str(&response).map_err(js)?;
    let commands = report["commands"]
        .as_array()
        .ok_or_else(|| JsValue::from_str("missing command report"))?;
    let definitions = report["definitions"]
        .as_array()
        .ok_or_else(|| JsValue::from_str("missing definitions"))?;
    let summary = serde_json::json!({"operation":report["operation"],"command_count":commands.len(),"maximum_reference_error":report["maximum_reference_error"],"elapsed_ms":report["elapsed_ms"],"full_model_inference":report["full_model_inference"],"first_outputs":report["output"].as_array().map(|v|v.iter().take(8).cloned().collect::<Vec<_>>()),"definitions":definitions.iter().map(|d|serde_json::json!({"id":d["id"],"parent":d["parent_id"],"working_bytes":d["budget"]["estimated_peak"],"merge":d["assembly"]["operation"]})).collect::<Vec<_>>(),"commands":commands.iter().map(|r|serde_json::json!({"id":r["command"]["id"],"device":r["command"]["device_id"],"generation":r["command"]["assignment_generation"],"status":r["status"],"started":r["started_ms"],"completed":r["completed_ms"]})).collect::<Vec<_>>()});
    ui::text(
        "shard-report",
        &serde_json::to_string_pretty(&summary).map_err(js)?,
    );
    LAST_CHECKPOINT.with(|c| *c.borrow_mut() = Some(report["checkpoint"].to_string()));
    ui::text(
        "status",
        "Shard execution passed reference comparison. Checkpoint retained in this page.",
    );
    Ok(())
}
async fn restore() -> Result<(), JsValue> {
    let checkpoint = LAST_CHECKPOINT
        .with(|c| c.borrow().clone())
        .ok_or_else(|| JsValue::from_str("run a shard request first"))?;
    let token = ui::value("access")?;
    let response = web::text("/api/shards/restore", &token, "POST", Some(&checkpoint)).await?;
    ui::text("shard-report", &response);
    ui::text(
        "status",
        "Checkpoint inspected and restored. Check replay_required before continuing.",
    );
    Ok(())
}

thread_local! {static SHARDS_ACTIVE:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}
async fn poll_shards(token: String) {
    while SHARDS_ACTIVE.with(|v| v.get()) {
        if let Ok(text) = web::text("/api/shards/status", &token, "GET", None).await {
            if !SHARDS_ACTIVE.with(|v| v.get()) {
                break;
            }
            ui::text("shard-report", &text);
        }
        if web::pause(500).await.is_err() {
            break;
        }
    }
}
