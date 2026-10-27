//! Download GGUF files into the Hugging Face cache that llama.cpp already
//! scans. This keeps downloads independent of the router's loaded-model limit.

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use reqwest::header::RANGE;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use crate::ollama::api::Progress;
use crate::util::encode_path;

#[derive(Deserialize)]
struct Repo { sha: String }

#[derive(Clone, Deserialize)]
struct Lfs { oid: String }

#[derive(Clone, Deserialize)]
struct File { path: String, size: u64, lfs: Option<Lfs> }

fn cache_root() -> PathBuf {
    if let Some(path) = std::env::var_os("LLAMA_CACHE") { return path.into(); }
    if let Some(path) = std::env::var_os("HF_HUB_CACHE") { return path.into(); }
    if let Some(path) = std::env::var_os("HUGGINGFACE_HUB_CACHE") { return path.into(); }
    if let Some(path) = std::env::var_os("HF_HOME") { return PathBuf::from(path).join("hub"); }
    let base = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache"));
    base.join("huggingface/hub")
}

fn valid_repo(repo: &str) -> bool {
    let parts: Vec<_> = repo.split('/').collect();
    parts.len() == 2 && parts.iter().all(|part| !part.is_empty() && *part != "." && *part != ".."
        && part.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.')))
}

fn valid_file(path: &str) -> bool {
    !path.is_empty() && Path::new(path).components().all(|part| matches!(part, Component::Normal(_)))
}

fn selected_files(tree: &[File], quant: &str) -> Result<Vec<File>> {
    let first = tree.iter().filter(|f| f.path.to_ascii_lowercase().ends_with(".gguf")
        && !super::auxiliary_gguf(&f.path) && super::quant_tag(&f.path) == quant)
        .min_by_key(|f| f.path.len()).with_context(|| {
            if tree.iter().any(|f| super::auxiliary_gguf(&f.path) && super::quant_tag(&f.path) == quant) {
                format!("{quant} is only a companion drafter or projector in this repository; choose a main model quantization")
            } else { format!("{quant} is not in the Hugging Face repository") }
        })?;
    let mut files = if let Some((prefix, _)) = first.path.rsplit_once("-00001-of-") {
        tree.iter().filter(|f| f.path.starts_with(prefix) && f.path.contains("-of-")
            && super::quant_tag(&f.path) == quant).cloned().collect::<Vec<_>>()
    } else { vec![first.clone()] };
    if let Some(projector) = tree.iter().filter(|f| f.path.to_ascii_lowercase().ends_with(".gguf")
        && f.path.to_ascii_lowercase().contains("mmproj")
        && Path::new(&f.path).parent() == Path::new(&first.path).parent())
        .min_by_key(|f| (super::quant_tag(&f.path) != "Q8_0", f.size)) {
        files.push(projector.clone());
    }
    if files.iter().any(|f| !valid_file(&f.path) || f.lfs.as_ref().is_none_or(|l| !valid_oid(&l.oid))) {
        bail!("the repository has an invalid GGUF file path or checksum");
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn valid_oid(oid: &str) -> bool { oid.len() == 64 && oid.bytes().all(|c| c.is_ascii_hexdigit()) }

/// A cached quant can exist even when it contains only a speculative drafter.
pub fn cached_auxiliary_model(model: &str) -> bool {
    let Some((repo, quant)) = model.rsplit_once(':') else { return false };
    if !valid_repo(repo) { return false; }
    let base = cache_root().join(format!("models--{}", repo.replace('/', "--")));
    let Ok(revision) = std::fs::read_to_string(base.join("refs/main")) else { return false };
    let mut dirs = vec![base.join("snapshots").join(revision.trim())];
    let (mut auxiliary, mut main) = (false, false);
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() { dirs.push(path); continue; }
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if !name.to_ascii_lowercase().ends_with(".gguf") || super::quant_tag(&name) != quant { continue; }
            if super::auxiliary_gguf(&name) { auxiliary = true; } else { main = true; }
        }
    }
    auxiliary && !main
}

async fn progress(tx: &async_channel::Sender<Progress>, completed: u64, total: u64) {
    let _ = tx.send(Progress { status: "Downloading".into(), completed, total }).await;
}

async fn download_file(client: &reqwest::Client, repo: &str, revision: &str, file: &File,
    base: &Path, completed: &mut u64, total: u64, tx: &async_channel::Sender<Progress>) -> Result<()> {
    let oid = &file.lfs.as_ref().unwrap().oid;
    let blob = base.join("blobs").join(oid);
    let partial = base.join("blobs").join(format!("{oid}.incomplete"));
    tokio::fs::create_dir_all(base.join("blobs")).await?;
    if tokio::fs::metadata(&blob).await.is_ok_and(|m| m.len() == file.size) {
        *completed += file.size;
    } else {
        let mut offset = tokio::fs::metadata(&partial).await.map(|m| m.len()).unwrap_or(0);
        if offset > file.size { tokio::fs::remove_file(&partial).await?; offset = 0; }
        *completed += offset;
        progress(tx, *completed, total).await;
        if offset < file.size {
            let url = format!("https://huggingface.co/{repo}/resolve/{revision}/{}", encode_path(&file.path));
            let mut request = super::auth(client.get(url));
            if offset > 0 { request = request.header(RANGE, format!("bytes={offset}-")); }
            let response = request.send().await?.error_for_status().context("Hugging Face file download")?;
            if offset > 0 && response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                *completed -= offset;
                offset = 0;
            }
            let mut out = tokio::fs::OpenOptions::new().create(true).write(true).open(&partial).await?;
            if offset == 0 { out.set_len(0).await?; } else { out.seek(std::io::SeekFrom::Start(offset)).await?; }
            let mut stream = response.bytes_stream();
            let mut last = Instant::now();
            while let Some(chunk) = tokio::time::timeout(Duration::from_secs(60), stream.next())
                .await.context("download stalled; retry to resume")? {
                let chunk = chunk.context("Hugging Face download interrupted; retry to resume")?;
                out.write_all(&chunk).await?;
                *completed += chunk.len() as u64;
                if last.elapsed() >= Duration::from_millis(300) {
                    progress(tx, *completed, total).await;
                    last = Instant::now();
                }
            }
            out.flush().await?;
        }
        let actual = tokio::fs::metadata(&partial).await?.len();
        if actual != file.size { bail!("incomplete download of {}: {actual} of {} bytes", file.path, file.size); }
        let check_path = partial.clone();
        let digest = tokio::task::spawn_blocking(move || -> Result<String> {
            use std::io::Read;
            let mut input = std::fs::File::open(check_path)?;
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 1024 * 1024];
            loop {
                let count = input.read(&mut buffer)?;
                if count == 0 { break; }
                hash.update(&buffer[..count]);
            }
            Ok(format!("{:x}", hash.finalize()))
        }).await??;
        if digest != *oid {
            tokio::fs::remove_file(&partial).await?;
            bail!("checksum mismatch for {}", file.path);
        }
        tokio::fs::rename(&partial, &blob).await?;
    }
    let snapshot = base.join("snapshots").join(revision).join(&file.path);
    tokio::fs::create_dir_all(snapshot.parent().unwrap()).await?;
    if tokio::fs::symlink_metadata(&snapshot).await.is_err() {
        let depth = Path::new(&file.path).components().count();
        let link = PathBuf::from("../".repeat(depth + 1)).join("blobs").join(oid);
        std::os::unix::fs::symlink(link, snapshot)?;
    }
    progress(tx, *completed, total).await;
    Ok(())
}

pub async fn download(model: &str, tx: &async_channel::Sender<Progress>) -> Result<()> {
    let (repo, tag) = model.rsplit_once(':').context("use a Hugging Face model as user/repo:quant")?;
    if !valid_repo(repo) { bail!("invalid Hugging Face repository name"); }
    let quant = super::quant_tag(tag);
    let encoded = encode_path(repo);
    let metadata: Repo = super::auth(super::client().get(format!("https://huggingface.co/api/models/{encoded}")))
        .send().await?.error_for_status().context("Hugging Face model")?.json().await?;
    if metadata.sha.len() != 40 || !metadata.sha.bytes().all(|c| c.is_ascii_hexdigit()) {
        bail!("Hugging Face returned an invalid revision");
    }
    let tree: Vec<File> = super::auth(super::client().get(format!("https://huggingface.co/api/models/{encoded}/tree/{}?recursive=true", metadata.sha)))
        .send().await?.error_for_status().context("Hugging Face file list")?.json().await?;
    let files = selected_files(&tree, &quant)?;
    let total = files.iter().map(|f| f.size).sum();
    let base = cache_root().join(format!("models--{}", repo.replace('/', "--")));
    let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(10))
        .user_agent(concat!("nyx-chat/", env!("CARGO_PKG_VERSION"))).build()?;
    let mut completed = 0;
    for file in &files {
        download_file(&client, repo, &metadata.sha, file, &base, &mut completed, total, tx).await?;
    }
    tokio::fs::create_dir_all(base.join("refs")).await?;
    tokio::fs::write(base.join("refs/main"), &metadata.sha).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_one_quant_and_its_projector() {
        let tree = [
            ("model-PQ2_0.gguf", 10), ("model-PTQ1_0.gguf", 8),
            ("mmproj-BF16.gguf", 4), ("mmproj-Q8_0.gguf", 2),
        ].map(|(path, size)| File { path: path.into(), size,
            lfs: Some(Lfs { oid: "a".repeat(64) }) });
        let files = selected_files(&tree, "PTQ1_0").unwrap();
        assert_eq!(files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
            ["mmproj-Q8_0.gguf", "model-PTQ1_0.gguf"]);
    }

    #[test]
    fn rejects_drafter_as_standalone_model() {
        let tree = [File { path: "model-DFlash2-Q8_0.gguf".into(), size: 8,
            lfs: Some(Lfs { oid: "a".repeat(64) }) }];
        assert!(selected_files(&tree, "Q8_0").err().unwrap().to_string().contains("companion drafter"));
    }
}
