//! Small formatting and encoding helpers.

pub fn encode_query(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn encode_path(s: &str) -> String {
    s.split('/')
        .map(|seg| {
            let mut out = String::new();
            for b in seg.bytes() {
                match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
                    _ => out.push_str(&format!("%{b:02X}")),
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// `4.7 GB`, `812 MB`.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else if v >= 100.0 {
        format!("{v:.0} {}", UNITS[i])
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// `1.2M`, `34.5K`.
pub fn count(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1}K", n as f64 / 1e3).replace(".0K", "K"),
        1_000_000..=999_999_999 => format!("{:.1}M", n as f64 / 1e6).replace(".0M", "M"),
        _ => format!("{:.1}B", n as f64 / 1e9).replace(".0B", "B"),
    }
}

/// Seconds since the epoch.
pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// "just now", "5 min ago", "3 h ago", "2 days ago", or a date.
pub fn ago(ts: i64) -> String {
    let d = (now() - ts).max(0);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", d / 60),
        3600..=86_399 => format!("{} h ago", d / 3600),
        86_400..=604_799 => {
            let days = d / 86_400;
            if days == 1 { "yesterday".into() } else { format!("{days} days ago") }
        }
        _ => chrono::DateTime::from_timestamp(ts, 0)
            .map(|t| t.with_timezone(&chrono::Local).format("%b %-d, %Y").to_string())
            .unwrap_or_default(),
    }
}

/// Parse an RFC 3339 time (Ollama and Hugging Face dates) to epoch seconds.
pub fn parse_time(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|t| t.timestamp())
}

/// Escape text for Pango markup.
pub fn esc(s: &str) -> String {
    gtk::glib::markup_escape_text(s).to_string()
}

/// Cut to at most `max` characters, adding an ellipsis.
pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { format!("{}…", s.chars().take(max).collect::<String>().trim_end()) }
}

/// Shorten from the middle, keeping both ends: `hf.co/unsl…Q4_K_M`.
pub fn clip_middle(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max || max < 5 {
        return s.to_string();
    }
    let head = (max - 1) / 2;
    let tail = max - 1 - head;
    let a: String = s.chars().take(head).collect();
    let b: String = s.chars().skip(n - tail).collect();
    format!("{a}…{b}")
}

/// A model name as shown in pickers: Hugging Face prefixes and `.gguf` dropped.
pub fn model_label(name: &str) -> String {
    let n = name.strip_prefix("hf.co/").unwrap_or(name);
    let n = n.strip_suffix(".gguf").unwrap_or(n);
    let n = n.strip_suffix(":latest").unwrap_or(n);
    clip_middle(n, 40)
}

/// Split a command line on whitespace, honouring single and double quotes.
pub fn shell_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut any = false;
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                any = true;
            }
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() || any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if !cur.is_empty() || any {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_counts() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(4_700_000_000), "4.7 GB");
        assert_eq!(size(812_000_000), "812 MB");
        assert_eq!(count(999), "999");
        assert_eq!(count(34_500), "34.5K");
        assert_eq!(count(2_000_000), "2M");
        assert_eq!(count(8_190_735_360), "8.2B");
    }

    #[test]
    fn encodes() {
        assert_eq!(encode_query("qwen 3/a"), "qwen+3%2Fa");
        assert_eq!(encode_path("unsloth/Qwen3 8B"), "unsloth/Qwen3%208B");
    }

    #[test]
    fn splits_words() {
        assert_eq!(shell_words(r#"npx -y "@scope/server x" '' a"#), vec!["npx", "-y", "@scope/server x", "", "a"]);
    }

    #[test]
    fn model_labels() {
        assert_eq!(model_label("llama3.2:latest"), "llama3.2");
        assert_eq!(clip_middle("abcdefghij", 5), "ab…ij");
        assert!(
            model_label("hf.co/HauhauCS/Gemma-4-E4B-Uncensored-HauhauCS-Aggressive:Gemma-4-E4B-Q4_K_M.gguf").chars().count()
                <= 40
        );
    }

    #[test]
    fn clips() {
        assert_eq!(clip("hello world", 5), "hello…");
        assert_eq!(clip("hi", 5), "hi");
    }
}
