//! Speech: dictation with whisper.cpp and reading aloud with Piper, both
//! running locally. PipeWire records and plays the audio.

use anyhow::{Context, Result, bail};
use std::cell::RefCell;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use crate::{cmd, paths, prefs, window};

thread_local! {
    static RECORDER: RefCell<Option<(Child, PathBuf)>> = const { RefCell::new(None) };
    static SPEAKER: RefCell<Option<u32>> = const { RefCell::new(None) };
}

fn bin(configured: &str, fallbacks: &[&str]) -> Option<String> {
    let c = configured.trim();
    if !c.is_empty() && cmd::present(&paths::expand(c).to_string_lossy()) {
        return Some(paths::expand(c).to_string_lossy().into());
    }
    fallbacks.iter().find(|b| cmd::present(b)).map(|b| b.to_string())
}

/// Why dictation can't run, if it can't.
pub fn dictation_problem() -> Option<String> {
    let p = prefs::get();
    if !cmd::present("pw-record") {
        return Some("Recording needs PipeWire's pw-record.".into());
    }
    if bin(&p.whisper_bin, &["whisper-cli", "whisper-cpp", "whisper"]).is_none() {
        return Some("Dictation needs whisper.cpp (the whisper-cli program).".into());
    }
    if p.whisper_model.trim().is_empty() || !paths::expand(&p.whisper_model).exists() {
        return Some("Choose a whisper model file (ggml-*.bin) in Settings → Voice.".into());
    }
    None
}

/// Why reading aloud can't run, if it can't.
pub fn speech_problem() -> Option<String> {
    let p = prefs::get();
    if !cmd::present("pw-play") {
        return Some("Playback needs PipeWire's pw-play.".into());
    }
    if bin(&p.piper_bin, &["piper", "piper-tts"]).is_none() {
        return Some("Reading aloud needs Piper (the piper program).".into());
    }
    if p.piper_voice.trim().is_empty() || !paths::expand(&p.piper_voice).exists() {
        return Some("Choose a Piper voice (.onnx) in Settings → Voice.".into());
    }
    None
}

pub fn recording() -> bool {
    RECORDER.with(|r| r.borrow().is_some())
}

pub fn start_recording() -> Result<()> {
    if let Some(problem) = dictation_problem() {
        bail!("{problem}");
    }
    let dir = paths::cache_dir().join("voice");
    std::fs::create_dir_all(&dir)?;
    let wav = dir.join("dictation.wav");
    let child = Command::new("pw-record")
        .args(["--rate", "16000", "--channels", "1", "--format", "s16"])
        .arg(&wav)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("couldn't start pw-record")?;
    RECORDER.with(|r| *r.borrow_mut() = Some((child, wav)));
    Ok(())
}

pub fn stop_and_transcribe(done: impl FnOnce(Result<String>) + 'static) {
    let Some((mut child, wav)) = RECORDER.with(|r| r.borrow_mut().take()) else {
        done(Err(anyhow::anyhow!("not recording")));
        return;
    };
    let p = prefs::get();
    let whisper = bin(&p.whisper_bin, &["whisper-cli", "whisper-cpp", "whisper"]).unwrap_or_default();
    let model = paths::expand(&p.whisper_model);
    cmd::background(
        move || -> Result<String> {
            // SIGINT lets pw-record finish the WAV header.
            // SAFETY: plain syscall on our own child's pid.
            unsafe { libc::kill(child.id() as i32, libc::SIGINT) };
            let _ = child.wait();
            let out = Command::new(&whisper)
                .args(["-m"])
                .arg(&model)
                .args(["-f"])
                .arg(&wav)
                .args(["-nt", "-np", "-l", "auto"])
                .stdin(Stdio::null())
                .output()
                .context("couldn't run whisper")?;
            if !out.status.success() {
                bail!("whisper failed: {}", String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or(""));
            }
            Ok(clean_transcript(&String::from_utf8_lossy(&out.stdout)))
        },
        done,
    );
}

/// whisper marks silence and noise as `[BLANK_AUDIO]`, `(music)` and so on.
fn clean_transcript(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' if depth > 0 => depth -= 1,
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn speaking() -> bool {
    SPEAKER.with(|s| s.borrow().is_some())
}

pub fn stop_speaking() {
    if let Some(pid) = SPEAKER.with(|s| s.borrow_mut().take()) {
        // SAFETY: plain syscall; the group was created for this playback only.
        unsafe { libc::kill(-(pid as i32), libc::SIGTERM) };
    }
}

/// Read `markdown` aloud (code blocks are skipped). Speaking again stops the last.
pub fn speak(markdown: &str) {
    if speaking() {
        stop_speaking();
        return;
    }
    if let Some(problem) = speech_problem() {
        window::toast(&problem);
        return;
    }
    let text = crate::markdown::plain(markdown);
    if text.is_empty() {
        return;
    }
    let p = prefs::get();
    let piper = bin(&p.piper_bin, &["piper", "piper-tts"]).unwrap_or_default();
    let voice = paths::expand(&p.piper_voice);
    let dir = paths::cache_dir().join("voice");
    let _ = std::fs::create_dir_all(&dir);
    let wav = dir.join("speech.wav");
    // One process group runs Piper then pw-play, so Stop ends both.
    let script = format!(
        "{} --model {} --output_file {} && pw-play {}",
        shell_quote(&piper),
        shell_quote(&voice.to_string_lossy()),
        shell_quote(&wav.to_string_lossy()),
        shell_quote(&wav.to_string_lossy())
    );
    use std::os::unix::process::CommandExt;
    let child = Command::new("bash")
        .arg("-c")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn();
    let Ok(mut child) = child else {
        window::toast("Couldn't start Piper");
        return;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    let pid = child.id();
    SPEAKER.with(|s| *s.borrow_mut() = Some(pid));
    cmd::background(
        move || child.wait().map(|s| s.success()).unwrap_or(false),
        move |_| {
            SPEAKER.with(|s| {
                if *s.borrow() == Some(pid) {
                    *s.borrow_mut() = None;
                }
            });
        },
    );
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcripts_drop_noise_markers() {
        assert_eq!(clean_transcript(" [BLANK_AUDIO]\n Hello there. (music) How are you?\n"), "Hello there. How are you?");
    }

    #[test]
    fn quotes_for_the_shell() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }
}
