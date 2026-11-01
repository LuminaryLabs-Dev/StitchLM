//! WASM wrapper around the identical native worker, with real LFM tensor operations.
use stitch_worker::WorkerCore;
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub struct WebConnector {
    worker: WorkerCore,
}
#[wasm_bindgen]
impl WebConnector {
    #[wasm_bindgen(constructor)]
    pub fn new(assignment_json: &str, weights: &[u8]) -> Result<WebConnector, JsValue> {
        let assignment = serde_json::from_str(assignment_json).map_err(js_error)?;
        let worker = WorkerCore::new(assignment, weights).map_err(js_error)?;
        Ok(Self { worker })
    }
    pub fn execute(&mut self, job_json: &str) -> Result<String, JsValue> {
        let job = serde_json::from_str(job_json).map_err(js_error)?;
        let result = self.worker.execute(&job).map_err(js_error)?;
        serde_json::to_string(&result).map_err(js_error)
    }
    pub fn reset(&mut self) {
        self.worker.reset();
    }
}
fn js_error(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// Runs in a dedicated Web Worker. Secrets remain in memory, not URLs or storage.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn connect(pairing_code: String, name: String, memory_mb: usize) -> Result<(), JsValue> {
    use stitch_platform::web;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    let scope = js_sys::global()
        .dyn_into::<web_sys::DedicatedWorkerGlobalScope>()
        .map_err(|_| JsValue::from_str("connector must run in a Web Worker"))?;
    let registration = stitch_core::Registration {
        kernels: vec![
            "lfm-blocks/1".into(),
            "dense-f32-matvec/1".into(),
            "prism-group128-matvec/1".into(),
        ],
        capabilities: vec!["f32".into(), "little_endian".into()],
        protocol: 1,
        name,
        kind: "web".into(),
        memory_budget_mb: memory_mb,
    };
    registration.validate().map_err(js_error)?;
    let json = web::text(
        "/api/register",
        &pairing_code,
        "POST",
        Some(&serde_json::to_string(&registration).map_err(js_error)?),
    )
    .await?;
    let assignment: stitch_core::Assignment = serde_json::from_str(&json).map_err(js_error)?;
    if assignment.weights_path != format!("/api/connectors/{}/weights", assignment.connector_id) {
        return Err(JsValue::from_str("unexpected shard path"));
    }
    scope.post_message(&JsValue::from_str("Downloading assigned model blocks…"))?;
    let response = web::request(
        &assignment.weights_path,
        &assignment.lease_token,
        "GET",
        None,
    )
    .await?;
    if assignment.weights_bytes > 256 * 1024 * 1024 {
        return Err(JsValue::from_str("shard transfer bound exceeded"));
    }
    let advertised = response
        .headers()
        .get("Content-Length")?
        .ok_or_else(|| JsValue::from_str("shard Content-Length required"))?
        .parse::<usize>()
        .map_err(js_error)?;
    if advertised != assignment.weights_bytes {
        return Err(JsValue::from_str("shard length mismatch"));
    }
    let array = JsFuture::from(response.array_buffer()?).await?;
    let bytes = js_sys::Uint8Array::new(&array).to_vec();
    let mut worker = WebConnector::new(&json, &bytes)?;
    drop(bytes);
    let endpoint = format!("/api/connectors/{}", assignment.connector_id);
    wait_ready_web(&endpoint, &assignment.lease_token).await?;
    let ready_message = if assignment.range.is_empty() {
        "Ready. Computing stateless Bonsai matrix shards; model state stays on host".to_string()
    } else {
        format!(
            "Ready. Computing real model blocks {}..{}",
            assignment.range.start, assignment.range.end
        )
    };
    scope.post_message(&JsValue::from_str(&ready_message))?;
    loop {
        let shard_json = web::text(
            &format!("{endpoint}/shard-job"),
            &assignment.lease_token,
            "GET",
            None,
        )
        .await?;
        if shard_json.trim() != "null" {
            let envelope: stitch_core::shards::WorkEnvelope =
                serde_json::from_str(&shard_json).map_err(js_error)?;
            envelope
                .validate_transfer(
                    &assignment.connector_id,
                    assignment.memory_budget_mb as u64 * 1024 * 1024,
                )
                .map_err(js_error)?;
            let expected = format!(
                "/api/connectors/{}/shard-weights/{}",
                assignment.connector_id,
                hex::encode(envelope.command.id.as_bytes())
            );
            if envelope.command.device_id != assignment.connector_id
                || envelope.weights_path != expected
                || envelope.definition.tensor.byte_length > 256 * 1024 * 1024
            {
                return Err(JsValue::from_str("invalid shard transfer"));
            }
            let response =
                web::request_allow_conflict(&expected, &assignment.lease_token, "GET", None)
                    .await?;
            if response.status() == 409 {
                continue;
            }
            let length = response
                .headers()
                .get("Content-Length")?
                .ok_or_else(|| JsValue::from_str("shard length required"))?
                .parse::<u64>()
                .map_err(js_error)?;
            if length != envelope.definition.tensor.byte_length {
                return Err(JsValue::from_str("shard transfer length mismatch"));
            }
            let bytes =
                js_sys::Uint8Array::new(&JsFuture::from(response.array_buffer()?).await?).to_vec();
            let start = js_sys::Date::now();
            let mut result = stitch_worker::execute_shard(
                &envelope.command,
                &envelope.definition,
                &bytes,
                &envelope.input,
            )
            .map_err(js_error)?;
            let elapsed = (js_sys::Date::now() - start).max(0.);
            result.execution_ms = Some(elapsed);
            let response = web::request_allow_conflict(
                &format!("{endpoint}/shard-result"),
                &assignment.lease_token,
                "POST",
                Some(&serde_json::to_string(&result).map_err(js_error)?),
            )
            .await?;
            if response.status() == 409 {
                continue;
            }
            scope.post_message(&JsValue::from_str(&format!(
                "Completed shard {}",
                envelope.definition.id
            )))?;
            web::pause(
                (elapsed * (100.0 / envelope.definition.budget.cpu_percent as f64 - 1.0))
                    .min(i32::MAX as f64) as i32,
            )
            .await?;
            continue;
        }
        let response = web::request_allow_conflict(
            &format!("{endpoint}/job"),
            &assignment.lease_token,
            "GET",
            None,
        )
        .await?;
        if response.status() == 409 {
            worker.reset();
            wait_ready_web(&endpoint, &assignment.lease_token).await?;
            continue;
        }
        let job = JsFuture::from(response.text()?)
            .await?
            .as_string()
            .unwrap_or_default();
        if job.trim() != "null" {
            let start = js_sys::Date::now();
            let result = worker.execute(&job)?;
            let mut result: stitch_core::JobResult =
                serde_json::from_str(&result).map_err(js_error)?;
            result.elapsed_ms = Some((js_sys::Date::now() - start).max(0.));
            let response = web::request_allow_conflict(
                &format!("{endpoint}/result"),
                &assignment.lease_token,
                "POST",
                Some(&serde_json::to_string(&result).map_err(js_error)?),
            )
            .await?;
            if response.status() == 409 {
                worker.reset();
                wait_ready_web(&endpoint, &assignment.lease_token).await?;
                continue;
            }
            scope.post_message(&JsValue::from_str(&format!(
                "Completed block job at token position {}",
                result.position
            )))?;
        } else {
            web::pause(100).await?;
        }
    }
}

/// Explicit low-level projection test, not a claim of complete Bonsai execution.
#[wasm_bindgen]
pub fn legacy_prism_group128_projection(packed: &[u8], input: &[f32]) -> Result<Vec<f32>, JsValue> {
    stitch_model::ternary::matvec_rows(packed, input).map_err(js_error)
}

#[wasm_bindgen]
pub fn execute_shard_json(envelope_json: &str, weights: &[u8]) -> Result<String, JsValue> {
    let envelope: stitch_core::shards::WorkEnvelope =
        serde_json::from_str(envelope_json).map_err(js_error)?;
    let result = stitch_worker::execute_shard(
        &envelope.command,
        &envelope.definition,
        weights,
        &envelope.input,
    )
    .map_err(js_error)?;
    serde_json::to_string(&result).map_err(js_error)
}

#[cfg(target_arch = "wasm32")]
async fn wait_ready_web(endpoint: &str, token: &str) -> Result<(), JsValue> {
    loop {
        let response = stitch_platform::web::request_allow_conflict(
            &format!("{endpoint}/ready"),
            token,
            "POST",
            None,
        )
        .await?;
        if response.status() == 409 {
            stitch_platform::web::pause(100).await?;
            continue;
        }
        return Ok(());
    }
}
