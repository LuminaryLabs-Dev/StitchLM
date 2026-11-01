use anyhow::{ensure, Result};
use clap::Parser;
use stitch_core::*;
use stitch_worker::WorkerCore;
#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:8787")]
    host: String,
    #[arg(long, env = "STITCH_PAIRING_CODE", hide_env_values = true)]
    pairing_code: String,
    /// Optional PEM CA certificate for a private LAN certificate authority.
    #[arg(long)]
    ca_cert: Option<std::path::PathBuf>,
    #[arg(long, default_value = "native-worker")]
    name: String,
    #[arg(long, default_value_t = 512)]
    memory_mb: usize,
    #[arg(long, default_value_t = 10)]
    cpu_percent: u32,
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let startup = std::time::Instant::now();
    let a = Args::parse();
    ensure!(
        (1..=100).contains(&a.cpu_percent),
        "CPU duty target must be1..100"
    );
    let url = reqwest::Url::parse(&a.host)?;
    ensure!(
        url.scheme() == "https"
            || (url.scheme() == "http"
                && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))),
        "LAN connectors require trusted HTTPS"
    );
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "host URL must not contain credentials or query"
    );
    let mut client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none());
    if let Some(path) = &a.ca_cert {
        let pem = std::fs::read(path)?;
        client = client.add_root_certificate(reqwest::Certificate::from_pem(&pem)?);
    }
    let client = client.build()?;
    let base = a.host.trim_end_matches('/');
    let assignment: Assignment = client
        .post(format!("{base}/api/register"))
        .bearer_auth(&a.pairing_code)
        .json(&Registration {
            kernels: vec![
                "lfm-blocks/1".into(),
                "dense-f32-matvec/1".into(),
                "prism-group128-matvec/1".into(),
            ],
            capabilities: vec!["f32".into(), "little_endian".into()],
            protocol: 1,
            name: a.name,
            kind: "native".into(),
            memory_budget_mb: a.memory_mb,
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    ensure!(
        assignment.weights_path == format!("/api/connectors/{}/weights", assignment.connector_id),
        "unexpected shard URL"
    );
    ensure!(
        assignment.weights_bytes <= 256 * 1024 * 1024,
        "assigned shard exceeds connector transfer bound"
    );
    let mut response = client
        .get(format!("{base}{}", assignment.weights_path))
        .bearer_auth(&assignment.lease_token)
        .send()
        .await?
        .error_for_status()?;
    ensure!(
        response
            .content_length()
            .is_none_or(|n| n == assignment.weights_bytes as u64),
        "shard response length mismatch"
    );
    let mut bytes = Vec::with_capacity(assignment.weights_bytes);
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes
                .len()
                .checked_add(chunk.len())
                .is_some_and(|n| n <= assignment.weights_bytes),
            "oversize shard response"
        );
        bytes.extend_from_slice(&chunk);
    }
    ensure!(
        bytes.len() == assignment.weights_bytes,
        "truncated shard response"
    );
    let mut worker = WorkerCore::new(assignment.clone(), &bytes)?;
    drop(bytes);
    let endpoint = format!("{base}/api/connectors/{}", assignment.connector_id);
    tokio::time::sleep(
        startup
            .elapsed()
            .mul_f64(100.0 / a.cpu_percent as f64 - 1.0),
    )
    .await;
    if !wait_ready(&client, &endpoint, &assignment.lease_token).await? {
        return Ok(());
    }
    if assignment.range.is_empty() {
        println!("Ready: stateless Bonsai matrix shards, model state stays on host");
    } else {
        println!(
            "Ready: real model blocks {}..{}, memory admission {}MiB",
            assignment.range.start, assignment.range.end, assignment.memory_budget_mb
        );
    }
    loop {
        let cycle = std::time::Instant::now();
        let shard: Option<stitch_core::shards::WorkEnvelope> = client
            .get(format!("{endpoint}/shard-job"))
            .bearer_auth(&assignment.lease_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if let Some(envelope) = shard {
            envelope.validate_transfer(
                &assignment.connector_id,
                assignment.memory_budget_mb as u64 * 1024 * 1024,
            )?;
            ensure!(
                envelope.command.device_id == assignment.connector_id,
                "shard assignment belongs to another device"
            );
            let expected = format!(
                "/api/connectors/{}/shard-weights/{}",
                assignment.connector_id,
                hex::encode(envelope.command.id.as_bytes())
            );
            ensure!(
                envelope.weights_path == expected
                    && envelope.definition.tensor.byte_length <= 256 * 1024 * 1024,
                "invalid shard transfer"
            );
            let response = client
                .get(format!("{base}{}", envelope.weights_path))
                .bearer_auth(&assignment.lease_token)
                .send()
                .await?;
            if response.status() == reqwest::StatusCode::CONFLICT {
                continue;
            }
            let mut response = response.error_for_status()?;
            let mut weights = Vec::with_capacity(envelope.definition.tensor.byte_length as usize);
            while let Some(chunk) = response.chunk().await? {
                ensure!(
                    weights
                        .len()
                        .checked_add(chunk.len())
                        .is_some_and(|n| n as u64 <= envelope.definition.tensor.byte_length),
                    "oversize shard transfer"
                );
                weights.extend_from_slice(&chunk);
            }
            let start = std::time::Instant::now();
            let mut result = stitch_worker::execute_shard(
                &envelope.command,
                &envelope.definition,
                &weights,
                &envelope.input,
            )?;
            let compute = start.elapsed();
            result.execution_ms = Some(compute.as_secs_f64() * 1000.);
            let response = client
                .post(format!("{endpoint}/shard-result"))
                .bearer_auth(&assignment.lease_token)
                .json(&result)
                .send()
                .await?;
            if response.status() == reqwest::StatusCode::CONFLICT {
                continue;
            }
            response.error_for_status()?;
            tokio::time::sleep(compute.mul_f64(
                100.0 / a.cpu_percent.min(envelope.definition.budget.cpu_percent) as f64 - 1.0,
            ))
            .await;
            continue;
        }
        let request = client
            .get(format!("{endpoint}/job"))
            .bearer_auth(&assignment.lease_token)
            .send();
        let response = tokio::select! {_=tokio::signal::ctrl_c()=>break,r=request=>r?};
        if response.status() == reqwest::StatusCode::CONFLICT {
            worker.reset();
            if !wait_ready(&client, &endpoint, &assignment.lease_token).await? {
                return Ok(());
            }
            continue;
        }
        let job: Option<InferenceJob> = response.error_for_status()?.json().await?;
        if let Some(job) = job {
            let start = std::time::Instant::now();
            let mut result = worker.execute(&job)?;
            result.elapsed_ms = Some(start.elapsed().as_secs_f64() * 1000.);
            let response = client
                .post(format!("{endpoint}/result"))
                .bearer_auth(&assignment.lease_token)
                .json(&result)
                .send()
                .await?;
            if response.status() == reqwest::StatusCode::CONFLICT {
                worker.reset();
                if !wait_ready(&client, &endpoint, &assignment.lease_token).await? {
                    return Ok(());
                }
                continue;
            }
            response.error_for_status()?;
            let rest = cycle.elapsed().mul_f64(100.0 / a.cpu_percent as f64 - 1.0);
            tokio::time::sleep(rest).await;
        } else {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
    Ok(())
}

async fn wait_ready(client: &reqwest::Client, endpoint: &str, token: &str) -> Result<bool> {
    loop {
        let request = client
            .post(format!("{endpoint}/ready"))
            .bearer_auth(token)
            .send();
        let response = tokio::select! {_=tokio::signal::ctrl_c()=>return Ok(false),response=request=>response?};
        if response.status() == reqwest::StatusCode::CONFLICT {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            continue;
        }
        response.error_for_status()?;
        return Ok(true);
    }
}
