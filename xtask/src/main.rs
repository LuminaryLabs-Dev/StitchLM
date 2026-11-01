use anyhow::{ensure, Context, Result};
use clap::{Parser, Subcommand};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};
#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Task,
}
#[derive(Subcommand)]
enum Task {
    FetchModel {
        #[arg(long)]
        directory: PathBuf,
    },
    FetchBonsai {
        #[arg(long)]
        directory: PathBuf,
    },
    InspectGguf {
        path: PathBuf,
    },
    BuildWeb,
    Check,
    Doctor,
}
fn run(program: &str, args: &[&str]) -> Result<()> {
    ensure!(
        Command::new(program).args(args).status()?.success(),
        "command failed: {program} {}",
        args.join(" ")
    );
    Ok(())
}
fn fetch(dir: &Path) -> Result<()> {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../models/manifests/lfm2.5-350m.json"))?;
    let _ = manifest;
    std::fs::create_dir_all(dir)?;
    let files = [
        (
            "config.json",
            "720b43d6ddc2ed25be23eed355aefcf342434a176dedad23dbe0a5e3ac24bbb8",
            1281u64,
        ),
        (
            "tokenizer.json",
            "df1d8d5ec5d091b460562ffd545e4a5e91d17d4a0db7ebe733be34ed374377bd",
            4733389,
        ),
        (
            "model.safetensors",
            "1c9c77a4471a7f590f85240f74ed1fc26df7fbde88c3006724e2f93ca993ea4e",
            708984464,
        ),
        (
            "LICENSE",
            "4d28ca14dedc0b3d0fcc2b3339f0e79931faa33874f3d24f522183a8fc70068c",
            10574,
        ),
    ];
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(1800))
        .build()?;
    for (name, digest, size) in files {
        let target = dir.join(name);
        if target.exists() {
            let (actual, hash) = file_digest(&target)?;
            ensure!(
                actual == size && hash == digest,
                "existing {name} failed integrity; move it aside before retry"
            );
            continue;
        }
        let url=format!("https://huggingface.co/LiquidAI/LFM2.5-350M/resolve/9e6c6ccf47cd318696e137d381a7ded8fe4df09f/{name}");
        let mut response = client.get(url).send()?.error_for_status()?;
        let partial = dir.join(format!("{name}.part"));
        let mut file = std::fs::File::create(&partial)?;
        let mut hash = Sha256::new();
        let mut total = 0u64;
        let mut buffer = [0u8; 65536];
        loop {
            let n = response.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            ensure!(total <= size, "download exceeds pinned size");
            hash.update(&buffer[..n]);
            file.write_all(&buffer[..n])?;
        }
        file.sync_all()?;
        ensure!(
            total == size && hex::encode(hash.finalize()) == digest,
            "download integrity failed for{name}"
        );
        std::fs::rename(partial, target)?;
        println!("Verified {name}");
    }
    std::fs::write(
        dir.join("manifest.json"),
        include_str!("../../models/manifests/lfm2.5-350m.json"),
    )?;
    Ok(())
}
fn web() -> Result<()> {
    run(
        "cargo",
        &[
            "build",
            "--locked",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "-p",
            "stitchlm-host-web",
            "-p",
            "stitchlm-connector-web",
        ],
    )?;
    let version = Command::new("wasm-bindgen")
        .arg("--version")
        .output()
        .context("install wasm-bindgen-cli0.2.130 first")?;
    ensure!(
        String::from_utf8_lossy(&version.stdout).trim() == "wasm-bindgen 0.2.130",
        "wasm-bindgen CLI must match0.2.130"
    );
    for (app, module) in [
        ("host-web", "stitchlm_host_web"),
        ("connector-web", "stitchlm_connector_web"),
    ] {
        let dir = format!("dist/{app}/pkg");
        std::fs::create_dir_all(&dir)?;
        run(
            "wasm-bindgen",
            &[
                &format!("target/wasm32-unknown-unknown/release/{module}.wasm"),
                "--target",
                "web",
                "--out-dir",
                &dir,
            ],
        )?;
        std::fs::copy(
            format!("apps/{app}/index.html"),
            format!("dist/{app}/index.html"),
        )?;
    }
    std::fs::copy("apps/host-web/page.js", "dist/host-web/page.js")?;
    for file in ["page.js", "worker.js"] {
        std::fs::copy(
            format!("apps/connector-web/{file}"),
            format!("dist/connector-web/{file}"),
        )?;
    }
    std::fs::copy("apps/web-assets/style.css", "dist/style.css")?;
    Ok(())
}
fn main() -> Result<()> {
    match Args::parse().command {
        Task::FetchModel { directory } => fetch(&directory)?,
        Task::FetchBonsai { directory } => fetch_bonsai(&directory)?,
        Task::InspectGguf { path } => println!(
            "{}",
            serde_json::to_string_pretty(&stitch_model::gguf::inspect(&mut std::fs::File::open(
                path
            )?)?)?
        ),
        Task::BuildWeb => web()?,
        Task::Check => {
            run("cargo", &["fmt", "--all", "--", "--check"])?;
            run("cargo", &["test", "--locked", "--workspace"])?;
            run(
                "cargo",
                &[
                    "check",
                    "--locked",
                    "--target",
                    "wasm32-unknown-unknown",
                    "-p",
                    "stitchlm-host-web",
                    "-p",
                    "stitchlm-connector-web",
                ],
            )?;
        }
        Task::Doctor => {
            run("rustc", &["--version"])?;
            run("cargo", &["--version"])?;
            println!("Run cargo xtask build-web, then the native host with a verified model directory. Browser and LAN capability require the acceptance tests; installed tooling alone is not proof.");
        }
    }
    Ok(())
}

fn file_digest(path: &Path) -> Result<(u64, String)> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut total = 0;
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        total += n as u64;
    }
    Ok((total, hex::encode(h.finalize())))
}
fn fetch_bonsai(dir: &Path) -> Result<()> {
    let m: serde_json::Value = serde_json::from_str(include_str!(
        "../../models/manifests/bonsai-27b-abliterated.json"
    ))?;
    let name = m["file"].as_str().context("file")?;
    let digest = m["sha256"].as_str().context("digest")?;
    let size = m["bytes"].as_u64().context("size")?;
    std::fs::create_dir_all(dir)?;
    let path = dir.join(name);
    if !path.exists() {
        let url = format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            m["repo"].as_str().unwrap(),
            m["revision"].as_str().unwrap(),
            name
        );
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(3600))
            .build()?;
        let mut response = client.get(url).send()?.error_for_status()?;
        let partial = dir.join(format!("{name}.part"));
        let mut out = std::fs::File::create(&partial)?;
        let mut total = 0u64;
        let mut h = Sha256::new();
        let mut buf = [0u8; 65536];
        loop {
            let n = response.read(&mut buf)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            ensure!(total <= size, "oversize download");
            h.update(&buf[..n]);
            out.write_all(&buf[..n])?;
        }
        out.sync_all()?;
        ensure!(
            total == size && hex::encode(h.finalize()) == digest,
            "Bonsai integrity failed"
        );
        std::fs::rename(partial, &path)?;
    }
    let (actual, hash) = file_digest(&path)?;
    ensure!(
        actual == size && hash == digest,
        "existing Bonsai file failed integrity"
    );
    println!(
        "Verified {}. This does not establish full-model runtime support.",
        path.display()
    );
    Ok(())
}
