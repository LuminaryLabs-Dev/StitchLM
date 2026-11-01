use anyhow::{ensure, Result};
use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};
use stitch_core::*;
use stitch_host::{secret, Connector, Fleet, Runtime};
use tokio::sync::mpsc;
#[derive(Parser)]
struct Args {
    #[arg(
        long,
        required_unless_present = "model_file",
        conflicts_with = "model_file"
    )]
    model_dir: Option<PathBuf>,
    #[arg(long, conflicts_with = "model_dir")]
    model_file: Option<PathBuf>,
    #[arg(long, requires = "model_file")]
    tokenizer: Option<PathBuf>,
    #[arg(long)]
    gguf_resource: Option<PathBuf>,
    #[arg(long, requires = "tls_key")]
    tls_cert: Option<PathBuf>,
    #[arg(long, requires = "tls_cert")]
    tls_key: Option<PathBuf>,
    #[arg(long, default_value = "127.0.0.1:8787")]
    listen: SocketAddr,
    #[arg(long, default_value_t = 256)]
    context: usize,
    #[arg(long, default_value = "dist")]
    web_dir: PathBuf,
}
#[derive(Clone)]
struct App {
    shard_service: Arc<Mutex<stitch_host::shards::ShardService>>,
    matrix_sources: Arc<HashMap<String, stitch_host::shards::MatrixSource>>,
    generation_gate: Arc<tokio::sync::Semaphore>,
    runtime: Arc<Runtime>,
    fleet: Arc<Mutex<Fleet>>,
    shards: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    admin: String,
    pair: String,
}
type ApiResult<T> = std::result::Result<T, (StatusCode, String)>;
fn err(e: impl ToString) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, e.to_string())
}
fn bearer(h: &HeaderMap) -> &str {
    h.get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
}
fn admin(s: &App, h: &HeaderMap) -> ApiResult<()> {
    if bearer(h) == s.admin {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            "host authorization required".into(),
        ))
    }
}
fn connector(s: &App, h: &HeaderMap, id: &str) -> ApiResult<()> {
    let mut f = s.fleet.lock().unwrap();
    let c = f
        .connectors
        .get_mut(id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "unknown connector".into()))?;
    if bearer(h) != c.assignment.lease_token {
        return Err((
            StatusCode::UNAUTHORIZED,
            "connector authorization required".into(),
        ));
    }
    c.seen = Instant::now();
    Ok(())
}
async fn status(State(s): State<App>, h: HeaderMap) -> ApiResult<Json<HostStatus>> {
    admin(&s, &h)?;
    let f = s.fleet.lock().unwrap();
    Ok(Json(HostStatus {
        protocol: 1,
        version: env!("CARGO_PKG_VERSION").into(),
        model: s.runtime.identity().clone(),
        active_request: f.active.clone(),
        context_limit: s.runtime.context_limit(),
        max_tokens: 128,
        total_layers: s.runtime.layers(),
        connectors: f
            .connectors
            .iter()
            .map(|(id, c)| ConnectorStatus {
                id: id.clone(),
                name: c.name.clone(),
                kind: c.kind.clone(),
                range: c.assignment.range.clone(),
                ready: c.ready,
                online: c.seen.elapsed().as_secs() < 30,
                completed_jobs: c.completed,
                last_seen_ms: c.seen.elapsed().as_millis() as u64,
                memory_budget_mb: c.assignment.memory_budget_mb,
            })
            .collect(),
    }))
}
async fn register(
    State(s): State<App>,
    h: HeaderMap,
    Json(r): Json<Registration>,
) -> ApiResult<Json<Assignment>> {
    if bearer(&h) != s.pair {
        return Err((StatusCode::UNAUTHORIZED, "pairing code required".into()));
    }
    r.validate().map_err(err)?;
    let mut f = s.fleet.lock().unwrap();
    if f.active.is_some() {
        return Err((
            StatusCode::CONFLICT,
            "wait until generation completes".into(),
        ));
    }
    if f.connectors.len() >= 20 {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "twenty connector admission limit reached".into(),
        ));
    }
    // More devices than distinct ranges are replicas. One healthy replica per
    // range participates in a request; no duplicate execution or fake speedup.
    let start = (0..14)
        .step_by(2)
        .min_by_key(|start| {
            f.connectors
                .values()
                .filter(|c| c.assignment.range.start == *start)
                .count()
        })
        .unwrap();
    let id = secret();
    let (assignment, bytes) = s
        .runtime
        .assignment(
            &r,
            id.clone(),
            secret(),
            BlockRange {
                start,
                end: start + 2,
            },
        )
        .map_err(err)?;
    s.shards.lock().unwrap().insert(id.clone(), bytes);
    f.connectors.insert(
        id,
        Connector {
            kernels: r.kernels,
            capabilities: r.capabilities,
            assignment: assignment.clone(),
            name: r.name,
            kind: r.kind,
            ready: false,
            seen: Instant::now(),
            completed: 0,
            pending: None,
            result: None,
        },
    );
    Ok(Json(assignment))
}
async fn weights(
    State(s): State<App>,
    Path(id): Path<String>,
    h: HeaderMap,
) -> ApiResult<Response> {
    connector(&s, &h, &id)?;
    let bytes = s
        .shards
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| err("missing shard"))?;
    Ok((
        [
            ("content-type", "application/octet-stream"),
            ("cache-control", "no-store"),
        ],
        bytes,
    )
        .into_response())
}
async fn ready(
    State(s): State<App>,
    Path(id): Path<String>,
    h: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    connector(&s, &h, &id)?;
    if s.shard_service.lock().unwrap().active_request.is_some() {
        return Err((StatusCode::CONFLICT, "execution is still active".into()));
    }
    let mut fleet = s.fleet.lock().unwrap();
    if fleet.active.is_some() {
        return Err((StatusCode::CONFLICT, "generation is still active".into()));
    }
    fleet.connectors.get_mut(&id).unwrap().ready = true;
    Ok(Json(serde_json::json!({"ready":true})))
}
async fn job(
    State(s): State<App>,
    Path(id): Path<String>,
    h: HeaderMap,
) -> ApiResult<Json<Option<InferenceJob>>> {
    connector(&s, &h, &id)?;
    let fleet = s.fleet.lock().unwrap();
    let connector = fleet.connectors.get(&id).unwrap();
    if !connector.ready {
        return Err((
            StatusCode::CONFLICT,
            "connector cache reset required".into(),
        ));
    }
    Ok(Json(connector.pending.clone()))
}
async fn result(
    State(s): State<App>,
    Path(id): Path<String>,
    h: HeaderMap,
    Json(result): Json<JobResult>,
) -> ApiResult<Json<serde_json::Value>> {
    connector(&s, &h, &id)?;
    let mut f = s.fleet.lock().unwrap();
    let c = f.connectors.get_mut(&id).unwrap();
    let job = c
        .pending
        .as_ref()
        .ok_or_else(|| (StatusCode::CONFLICT, "no pending job".into()))?;
    result.matches(job).map_err(err)?;
    let output = result
        .output
        .as_ref()
        .ok_or_else(|| err("missing tensor"))?;
    output
        .validate(s.runtime.hidden_size(), s.runtime.context_limit())
        .map_err(err)?;
    if output.shape != job.input.shape {
        return Err(err("output shape mismatch"));
    }
    let tx = c
        .result
        .take()
        .ok_or_else(|| err("result already accepted"))?;
    tx.send(result)
        .map_err(|_| err("generation already ended"))?;
    c.pending = None;
    c.completed += 1;
    Ok(Json(serde_json::json!({"accepted":true})))
}
async fn remove(
    State(s): State<App>,
    Path(id): Path<String>,
    h: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    if s.shard_service.lock().unwrap().active_request.is_some() {
        return Err((StatusCode::CONFLICT, "cancel shard execution first".into()));
    }
    let mut f = s.fleet.lock().unwrap();
    if f.active.is_some() {
        return Err((StatusCode::CONFLICT, "cancel generation first".into()));
    }
    f.connectors.remove(&id);
    s.shards.lock().unwrap().remove(&id);
    Ok(Json(serde_json::json!({"removed":true})))
}
async fn cancel(State(s): State<App>, h: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    s.fleet.lock().unwrap().cancel = true;
    {
        let mut shards = s.shard_service.lock().unwrap();
        shards.active_request = None;
        let ids: Vec<_> = shards.pending.values().cloned().collect();
        for id in ids {
            let _ = shards.buffer.cancel(&id, "user cancelled");
        }
        shards.pending.clear();
    }
    Ok(Json(serde_json::json!({"cancel_requested":true})))
}
async fn generate(
    State(s): State<App>,
    h: HeaderMap,
    Json(request): Json<GenerateRequest>,
) -> ApiResult<Response> {
    admin(&s, &h)?;
    if s.fleet.lock().unwrap().active.is_some() {
        return Err((StatusCode::CONFLICT, "generation already active".into()));
    }
    if request.prompt.trim().is_empty()
        || request.prompt.len() > 8192
        || !(1..=128).contains(&request.max_tokens)
    {
        return Err(err("invalid prompt or token limit"));
    }
    let permit = s
        .generation_gate
        .clone()
        .try_acquire_owned()
        .map_err(|_| (StatusCode::CONFLICT, "generation already active".into()))?;
    let (tx, rx) = mpsc::channel::<GenerationEvent>(16);
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if let Err(e) = s.runtime.generate(request, s.fleet, tx, s.shard_service) {
            eprintln!("generation ended: {e}");
        }
    });
    use tokio_stream::StreamExt;
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(|event| {
        Ok::<_, std::convert::Infallible>(format!(
            "{}\n",
            serde_json::to_string(&event).expect("finite event")
        ))
    });
    Ok((
        [
            ("content-type", "application/x-ndjson"),
            ("cache-control", "no-store"),
        ],
        axum::body::Body::from_stream(stream),
    )
        .into_response())
}
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(
        args.listen.ip().is_loopback() || args.tls_cert.is_some(),
        "LAN listening requires --tls-cert and --tls-key with a certificate trusted by connector devices"
    );
    let runtime = Arc::new(if let Some(path) = &args.model_file {
        Runtime::load_bonsai(
            path,
            args.tokenizer.as_ref().ok_or_else(|| {
                anyhow::anyhow!("Bonsai requires --tokenizer with the pinned tokenizer.json")
            })?,
            args.context,
        )?
    } else {
        Runtime::load(
            args.model_dir
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("model directory required"))?,
            args.context,
        )?
    });
    let admin = secret();
    let pair = secret();
    let mut matrices = HashMap::new();
    if let Some(path) = &args.gguf_resource {
        matrices.insert(
            "bonsai-ffn-gate".into(),
            stitch_host::shards::MatrixSource::bonsai(path)?,
        );
    }
    let s = App {
        shard_service: Arc::new(Mutex::new(stitch_host::shards::ShardService::new()?)),
        matrix_sources: Arc::new(matrices),
        runtime,
        generation_gate: Arc::new(tokio::sync::Semaphore::new(1)),
        fleet: Arc::new(Mutex::new(Fleet {
            connectors: HashMap::new(),
            active: None,
            cancel: false,
        })),
        shards: Arc::new(Mutex::new(HashMap::new())),
        admin: admin.clone(),
        pair: pair.clone(),
    };
    let router = Router::new()
        .route("/api/shards/project",post(project))
        .route("/api/shards/restore",post(restore_shards))
        .route("/api/shards/status",get(shard_status))
        .route("/api/shards/checkpoint",get(shard_checkpoint))
        .route("/api/connectors/{id}/shard-job",get(shard_job))
        .route("/api/connectors/{id}/shard-weights/{command}",get(shard_weights))
        .route("/api/connectors/{id}/shard-result",post(shard_result))
        .route("/api/status", get(status))
        .route("/api/register", post(register))
        .route("/api/generate", post(generate))
        .route("/api/cancel", post(cancel))
        .route("/api/connectors/{id}/weights", get(weights))
        .route("/api/connectors/{id}/ready", post(ready))
        .route("/api/connectors/{id}/job", get(job))
        .route("/api/connectors/{id}/result", post(result))
        .route("/api/connectors/{id}/remove", post(remove))
        .layer(axum::middleware::map_response(|mut response:Response|async move{
            let h=response.headers_mut();
            h.insert("X-Content-Type-Options","nosniff".parse().unwrap());
            h.insert("Referrer-Policy","no-referrer".parse().unwrap());
            h.insert("Content-Security-Policy","default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; connect-src 'self'; style-src 'self'; frame-ancestors 'none'; base-uri 'none'".parse().unwrap());
            response
        }))
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        .with_state(s)
        .fallback_service(tower_http::services::ServeDir::new(args.web_dir));
    let scheme = if args.tls_cert.is_some() {
        "https"
    } else {
        "http"
    };
    println!("StitchLM v0.0.1 listening on {scheme}://{}\nHost console: {scheme}://{}/host-web/\nConnector: {scheme}://{}/connector-web/\nHost access code: {}\nPairing code: {}\nKeep these codes private. They expire on restart.",args.listen,args.listen,args.listen,admin,pair);
    if let (Some(cert), Some(key)) = (args.tls_cert, args.tls_key) {
        rustls::crypto::ring::default_provider()
            .install_default()
            .map_err(|_| anyhow::anyhow!("TLS provider initialization failed"))?;
        let config = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key).await?;
        let handle = axum_server::Handle::new();
        let shutdown = handle.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            shutdown.graceful_shutdown(Some(std::time::Duration::from_secs(5)));
        });
        axum_server::bind_rustls(args.listen, config)
            .handle(handle)
            .serve(router.into_make_service())
            .await?;
    } else {
        let listener = tokio::net::TcpListener::bind(args.listen).await?;
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await?;
    }
    Ok(())
}

async fn shard_job(
    State(s): State<App>,
    Path(id): Path<String>,
    h: HeaderMap,
) -> ApiResult<Json<Option<stitch_core::shards::WorkEnvelope>>> {
    connector(&s, &h, &id)?;
    Ok(Json(
        s.shard_service.lock().unwrap().poll(&id).map_err(err)?,
    ))
}
async fn shard_weights(
    State(s): State<App>,
    Path((id, encoded)): Path<(String, String)>,
    h: HeaderMap,
) -> ApiResult<Response> {
    connector(&s, &h, &id)?;
    let command = String::from_utf8(hex::decode(encoded).map_err(err)?).map_err(err)?;
    let service = s.shard_service.lock().unwrap();
    if service.active_request.is_none()
        || service
            .pending
            .get(&id)
            .is_none_or(|pending| pending != &command)
        || service.buffer.records.get(&command).is_none_or(|record| {
            !matches!(
                record.status,
                stitch_core::shards::Status::Dispatched | stitch_core::shards::Status::Running
            ) || stitch_host::shards::now_ms() > record.command.lease_expires_ms
        })
    {
        return Err((
            StatusCode::CONFLICT,
            "shard weight request is no longer active".into(),
        ));
    }
    let bytes = service.weights(&id, &command).map_err(err)?;
    drop(service);
    Ok((
        [
            ("content-type", "application/octet-stream"),
            ("cache-control", "no-store"),
        ],
        bytes,
    )
        .into_response())
}
async fn shard_result(
    State(s): State<App>,
    Path(id): Path<String>,
    h: HeaderMap,
    Json(result): Json<stitch_core::shards::ResultPacket>,
) -> ApiResult<Json<serde_json::Value>> {
    connector(&s, &h, &id)?;
    if result.device_id != id {
        return Err(err("result device does not match authenticated lease"));
    }
    let mut service = s.shard_service.lock().unwrap();
    if service
        .buffer
        .records
        .get(&result.command_id)
        .is_none_or(|r| {
            r.command.assignment_generation != result.assignment_generation
                || (r.status != stitch_core::shards::Status::Completed
                    && stitch_host::shards::now_ms() > r.command.lease_expires_ms)
                || !matches!(
                    r.status,
                    stitch_core::shards::Status::Dispatched
                        | stitch_core::shards::Status::Running
                        | stitch_core::shards::Status::Completed
                )
        })
    {
        return Err((
            StatusCode::CONFLICT,
            "shard command is no longer active".into(),
        ));
    }
    let accepted = service.accept(result).map_err(err)?;
    Ok(Json(serde_json::json!({"accepted":accepted})))
}
async fn project(
    State(s): State<App>,
    h: HeaderMap,
    Json(request): Json<stitch_host::shards::ProjectionRequest>,
) -> ApiResult<Json<stitch_host::shards::ProjectionReport>> {
    admin(&s, &h)?;
    let permit = s
        .generation_gate
        .clone()
        .try_acquire_owned()
        .map_err(|_| (StatusCode::CONFLICT, "execution already active".into()))?;
    let source = match &request.target {
        stitch_host::shards::ProjectionTarget::Registered { resource_id } => s
            .matrix_sources
            .get(resource_id)
            .cloned()
            .ok_or_else(|| err("unknown registered matrix resource"))?,
        stitch_host::shards::ProjectionTarget::Dense {
            rows,
            columns,
            weights,
        } => {
            if *rows == 0
                || *columns == 0
                || rows.checked_mul(*columns) != Some(weights.len() as u64)
                || weights.len() > 1_000_000
                || !weights.iter().all(|v| v.is_finite())
            {
                return Err(err("invalid dense matrix dimensions"));
            }
            let bytes = stitch_core::shards::float_bytes(weights);
            let hash = stitch_core::shards::digest(&bytes);
            stitch_host::shards::MatrixSource {
                model: ModelIdentity {
                    id: "user-supplied-dense-matrix".into(),
                    revision: hash[..40].into(),
                    weights_sha256: hash,
                },
                tensor_id: "dense-matrix".into(),
                rows: *rows,
                columns: *columns,
                packed: false,
                bytes: Arc::new(bytes),
            }
        }
    };
    let required_kernel = if source.packed {
        "prism-group128-matvec/1"
    } else {
        "dense-f32-matvec/1"
    };
    let participants = {
        let f = s.fleet.lock().unwrap();
        let mut v: Vec<_> = f
            .connectors
            .iter()
            .filter(|(_, c)| {
                c.ready
                    && c.seen.elapsed().as_secs() < 30
                    && c.kernels.iter().any(|k| k == required_kernel)
                    && ["f32", "little_endian"]
                        .iter()
                        .all(|cap| c.capabilities.iter().any(|c| c == cap))
            })
            .map(|(id, c)| (id.clone(), c.kind.clone(), c.assignment.memory_budget_mb))
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    };
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        stitch_host::shards::run_projection(request, source, participants, s.shard_service)
    })
    .await
    .map_err(err)?
    .map_err(err)?;
    Ok(Json(result))
}

async fn restore_shards(
    State(s): State<App>,
    h: HeaderMap,
    Json(checkpoint): Json<stitch_core::shards::Checkpoint>,
) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    let _permit = s.generation_gate.clone().try_acquire_owned().map_err(|_| {
        (
            StatusCode::CONFLICT,
            "cancel active execution before restoring".into(),
        )
    })?;
    let mut service = s.shard_service.lock().unwrap();
    let restored =
        stitch_core::shards::CommandBuffer::restore(&checkpoint, &service.resources, 256, 4)
            .map_err(err)?;
    let replay = restored
        .records
        .values()
        .any(|r| r.status == stitch_core::shards::Status::ReplayRequired)
        || restored.states.values().any(|s| s.replay_required);
    let completed = restored
        .records
        .values()
        .filter(|r| r.status == stitch_core::shards::Status::Completed)
        .count();
    let queued = restored
        .records
        .values()
        .filter(|r| r.status == stitch_core::shards::Status::Queued)
        .count();
    service.buffer = restored;
    service.pending.clear();
    service.envelopes.clear();
    service.weight_slices.clear();
    service.active_request = None;
    Ok(Json(
        serde_json::json!({"restored":true,"completed":completed,"queued":queued,"replay_required":replay,"next_action":if replay{"restart from retained inputs or full prompt"}else if queued>0{"replan queued work with fresh device leases"}else{"completed outputs retained"}}),
    ))
}

async fn shard_status(State(s): State<App>, h: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    admin(&s, &h)?;
    let service = s.shard_service.lock().unwrap();
    Ok(Json(
        serde_json::json!({"active_request":service.active_request,"commands":service.buffer.records.values().map(|r|serde_json::json!({"id":r.command.id,"device":r.command.device_id,"generation":r.command.assignment_generation,"sequence":r.command.sequence,"status":r.status,"placement":r.placement})).collect::<Vec<_>>(),"devices":service.devices.values().collect::<Vec<_>>()}),
    ))
}
async fn shard_checkpoint(
    State(s): State<App>,
    h: HeaderMap,
) -> ApiResult<Json<stitch_core::shards::Checkpoint>> {
    admin(&s, &h)?;
    let service = s.shard_service.lock().unwrap();
    let first = service
        .buffer
        .records
        .values()
        .next()
        .ok_or_else(|| err("no shard execution to checkpoint"))?;
    let def = &service.buffer.definitions[&first.command.shard_id];
    Ok(Json(
        service
            .buffer
            .checkpoint(
                &first.command.request_id,
                &def.weight_checksum,
                first.command.plan_version,
                first.committed_position,
                &service.resources,
            )
            .map_err(err)?,
    ))
}
