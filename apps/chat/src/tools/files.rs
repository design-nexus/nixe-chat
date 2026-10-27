//! Reading files inside allowed folders, and running approved shell commands.

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::Output;
use crate::paths;

/// Resolve `path` and check it's inside one of `allowed` (after following links).
pub fn permitted(path: &str, allowed: &[String]) -> Result<PathBuf> {
    if allowed.is_empty() {
        bail!("No folders are allowed yet. The user can add some in Settings → Tools.");
    }
    let p = paths::expand(path.trim());
    let real = std::fs::canonicalize(&p).map_err(|e| anyhow::anyhow!("{}: {e}", p.display()))?;
    for a in allowed {
        if let Ok(root) = std::fs::canonicalize(paths::expand(a))
            && real.starts_with(&root)
        {
            return Ok(real);
        }
    }
    bail!("{} is outside the folders the user allowed ({})", paths::pretty(&real), allowed.join(", "))
}

pub fn read(path: &str, allowed: &[String]) -> Result<Output> {
    let p = permitted(path, allowed)?;
    if p.is_dir() {
        return list(path, allowed);
    }
    let bytes = std::fs::read(&p)?;
    if bytes.iter().take(4096).any(|b| *b == 0) {
        bail!("{} looks like a binary file", paths::pretty(&p));
    }
    let text = String::from_utf8_lossy(&bytes);
    Ok(Output::text(crate::util::clip(&text, 60_000)))
}

pub fn list(path: &str, allowed: &[String]) -> Result<Output> {
    let p = permitted(path, allowed)?;
    let mut entries: Vec<String> = std::fs::read_dir(&p)?
        .flatten()
        .map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let meta = e.metadata().ok();
            if meta.as_ref().is_some_and(|m| m.is_dir()) {
                format!("{name}/")
            } else {
                format!("{name} ({})", crate::util::size(meta.map(|m| m.len()).unwrap_or(0)))
            }
        })
        .collect();
    entries.sort();
    entries.truncate(500);
    Ok(Output::text(format!("{}:\n{}", paths::pretty(&p), entries.join("\n"))))
}

pub async fn shell(command: &str) -> Result<Output> {
    let child = tokio::process::Command::new("bash")
        .arg("-lc")
        .arg(command)
        .current_dir(paths::home())
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = match tokio::time::timeout(Duration::from_secs(120), child).await {
        Ok(r) => r?,
        Err(_) => bail!("the command took longer than 2 minutes and was stopped"),
    };
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() {
        text.push_str("\n[stderr]\n");
        text.push_str(&err);
    }
    text.push_str(&format!("\n[exit status {}]", out.status.code().unwrap_or(-1)));
    Ok(Output::text(crate::util::clip(text.trim(), 20_000)))
}

/// Text from a file the user attached: plain text directly, PDFs via `pdftotext`.
pub fn extract_text(path: &Path) -> Result<String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ext == "pdf" {
        if !crate::cmd::present("pdftotext") {
            bail!("Reading PDFs needs pdftotext (the poppler package).");
        }
        let p = path.to_string_lossy();
        return crate::cmd::run(&["pdftotext", "-layout", &p, "-"]);
    }
    let bytes = std::fs::read(path)?;
    if bytes.iter().take(4096).any(|b| *b == 0) {
        bail!("{} isn't a text file", path.file_name().unwrap_or_default().to_string_lossy());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_allowed_folders() {
        let dir = std::env::temp_dir().join(format!("nyx-chat-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("ok")).unwrap();
        std::fs::write(dir.join("ok/a.txt"), "hello").unwrap();
        std::fs::write(dir.join("secret.txt"), "no").unwrap();
        let allowed = vec![dir.join("ok").to_string_lossy().to_string()];
        assert_eq!(read(&dir.join("ok/a.txt").to_string_lossy(), &allowed).unwrap().text, "hello");
        assert!(read(&dir.join("secret.txt").to_string_lossy(), &allowed).is_err());
        assert!(read(&dir.join("ok/../secret.txt").to_string_lossy(), &allowed).is_err());
        assert!(read(&dir.join("ok/a.txt").to_string_lossy(), &[]).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
