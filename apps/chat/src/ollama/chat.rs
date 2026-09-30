//! The chat loop: stream a reply, run any tools the model calls, send the
//! results back, and repeat until the model answers in words.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::api::{self, Conn};
use crate::tools;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

/// One message as Ollama sees it.
#[derive(Clone, Debug, Default)]
pub struct Msg {
    pub role: String,
    pub content: String,
    /// Base64 images, for vision models.
    pub images: Vec<String>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_name: String,
}

impl Msg {
    pub fn new(role: &str, content: impl Into<String>) -> Msg {
        Msg { role: role.into(), content: content.into(), ..Default::default() }
    }

    fn to_json(&self) -> Value {
        let mut m = json!({ "role": self.role, "content": self.content });
        if !self.images.is_empty() {
            m["images"] = json!(self.images);
        }
        if !self.tool_calls.is_empty() {
            m["tool_calls"] =
                self.tool_calls.iter().map(|c| json!({ "function": { "name": c.name, "arguments": c.arguments } })).collect();
        }
        if !self.tool_name.is_empty() {
            m["tool_name"] = json!(self.tool_name);
        }
        m
    }
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub tokens: u64,
    pub tokens_per_sec: f64,
}

pub struct Request {
    pub conn: Conn,
    pub model: String,
    pub messages: Vec<Msg>,
    /// Tool definitions (empty when the model can't use tools).
    pub tools: Vec<Value>,
    /// `Some` only for models that can think.
    pub think: Option<bool>,
    /// `"json"` or a JSON schema.
    pub format: Option<Value>,
    pub options: Value,
    pub keep_alive: String,
    pub max_rounds: u32,
    pub tool_ctx: tools::Ctx,
}

pub enum Event {
    /// A new assistant message begins (once per tool round).
    RoundStart,
    Delta {
        content: String,
        thinking: String,
    },
    /// The assistant message is complete. `calls` are the tools it asked for.
    RoundEnd {
        content: String,
        thinking: String,
        calls: Vec<ToolCall>,
        stats: Stats,
    },
    ToolStart(ToolCall),
    ToolResult {
        call: ToolCall,
        output: tools::Output,
    },
    /// A tool needs the user's go-ahead.
    Approval {
        title: String,
        detail: String,
        reply: tokio::sync::oneshot::Sender<bool>,
    },
    Done,
    Error(String),
}

/// Set to stop the reply at the next chunk.
pub type Cancel = Arc<AtomicBool>;

pub async fn run(mut req: Request, tx: async_channel::Sender<Event>, cancel: Cancel) {
    if let Err(e) = run_inner(&mut req, &tx, &cancel).await {
        let _ = tx.send(Event::Error(api::short_error(&format!("{e:#}")))).await;
    }
    let _ = tx.send(Event::Done).await;
}

async fn run_inner(req: &mut Request, tx: &async_channel::Sender<Event>, cancel: &Cancel) -> Result<()> {
    for _ in 0..req.max_rounds.max(1) {
        let _ = tx.send(Event::RoundStart).await;
        let (content, thinking, calls, stats) = stream_once(req, tx, cancel).await?;
        let stopped = cancel.load(Ordering::Relaxed);
        let _ = tx
            .send(Event::RoundEnd {
                content: content.clone(),
                thinking,
                calls: if stopped { vec![] } else { calls.clone() },
                stats,
            })
            .await;
        if calls.is_empty() || stopped {
            return Ok(());
        }
        req.messages.push(Msg { role: "assistant".into(), content, tool_calls: calls.clone(), ..Default::default() });
        for call in calls {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let _ = tx.send(Event::ToolStart(call.clone())).await;
            let output = tools::call(&call, &req.tool_ctx, tx).await;
            req.messages.push(Msg {
                role: "tool".into(),
                content: output.text.clone(),
                tool_name: call.name.clone(),
                ..Default::default()
            });
            let _ = tx.send(Event::ToolResult { call, output }).await;
        }
    }
    Ok(())
}

pub fn body(req: &Request) -> Value {
    let mut body = json!({
        "model": req.model,
        "messages": req.messages.iter().map(Msg::to_json).collect::<Vec<_>>(),
        "stream": true,
        "options": req.options,
        "keep_alive": api::keep_alive_value(&req.keep_alive),
    });
    if !req.tools.is_empty() {
        body["tools"] = json!(req.tools);
    }
    if let Some(think) = req.think {
        body["think"] = json!(think);
    }
    if let Some(format) = &req.format {
        body["format"] = format.clone();
    }
    body
}

async fn stream_once(
    req: &Request,
    tx: &async_channel::Sender<Event>,
    cancel: &Cancel,
) -> Result<(String, String, Vec<ToolCall>, Stats)> {
    let response = req.conn.post("/api/chat").json(&body(req)).send().await?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or(text);
        anyhow::bail!("{} {}", status.as_u16(), msg.trim());
    }
    let mut content = String::new();
    let mut thinking = String::new();
    let mut calls = Vec::new();
    let mut stats = Stats::default();
    let mut split = ThinkSplitter::default();
    let emit = |c: String, t: String, content: &mut String, thinking: &mut String| {
        if !c.is_empty() || !t.is_empty() {
            content.push_str(&c);
            thinking.push_str(&t);
            let _ = tx.try_send(Event::Delta { content: c, thinking: t });
        }
    };
    api::read_ndjson(response, |v| {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let m = &v["message"];
        let (c, t_inline) = split.push(m["content"].as_str().unwrap_or(""));
        let t = format!("{}{t_inline}", m["thinking"].as_str().unwrap_or(""));
        emit(c, t, &mut content, &mut thinking);
        calls.extend(parse_calls(m));
        if v["done"].as_bool().unwrap_or(false) {
            stats = parse_stats(&v);
        }
        true
    })
    .await?;
    let (c, t) = split.finish();
    emit(c, t, &mut content, &mut thinking);
    Ok((content, thinking, calls, stats))
}

/// Some models write their reasoning into the reply itself, between
/// `<think>…</think>` or Gemma's `<|channel>thought…<channel|>`, when their
/// template doesn't separate it. This moves that text to thinking as it streams.
#[derive(Default)]
struct ThinkSplitter {
    inside: bool,
    /// Text held back because it might be the start of a marker.
    pending: String,
}

const OPENERS: [&str; 2] = ["<think>", "<|channel>thought"];
const CLOSERS: [&str; 2] = ["</think>", "<channel|>"];

impl ThinkSplitter {
    /// Returns (content, thinking) that can be shown now.
    fn push(&mut self, chunk: &str) -> (String, String) {
        self.pending.push_str(chunk);
        let (mut content, mut thinking) = (String::new(), String::new());
        loop {
            let markers: &[&str] = if self.inside { &CLOSERS } else { &OPENERS };
            let found = markers.iter().filter_map(|m| self.pending.find(m).map(|i| (i, m.len()))).min();
            let out = if self.inside { &mut thinking } else { &mut content };
            match found {
                Some((i, len)) => {
                    out.push_str(&self.pending[..i]);
                    self.pending.drain(..i + len);
                    self.inside = !self.inside;
                    if !self.inside {
                        // The answer usually starts on a new line after the marker.
                        let trimmed = self.pending.trim_start_matches('\n').to_string();
                        self.pending = trimmed;
                    }
                }
                None => {
                    // Keep back a tail that could still grow into a marker.
                    let keep = markers
                        .iter()
                        .flat_map(|m| (1..m.len()).rev().filter(move |k| m.is_char_boundary(*k)).map(move |k| &m[..k]))
                        .filter(|p| self.pending.ends_with(p))
                        .map(|p| p.len())
                        .max()
                        .unwrap_or(0);
                    let cut = self.pending.len() - keep;
                    out.push_str(&self.pending[..cut]);
                    self.pending.drain(..cut);
                    break;
                }
            }
        }
        (content, thinking)
    }

    fn finish(&mut self) -> (String, String) {
        let rest = std::mem::take(&mut self.pending);
        if self.inside { (String::new(), rest) } else { (rest, String::new()) }
    }
}

/// A whole reply without any reasoning written inline.
pub fn strip_thinking(text: &str) -> String {
    let mut s = ThinkSplitter::default();
    let (a, _) = s.push(text);
    let (b, _) = s.finish();
    a + &b
}

fn parse_calls(m: &Value) -> Vec<ToolCall> {
    m["tool_calls"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let f = &c["function"];
            let name = f["name"].as_str()?.to_string();
            let mut arguments = f["arguments"].clone();
            // Some models send the arguments as a JSON string.
            if let Some(s) = arguments.as_str() {
                arguments = serde_json::from_str(s).unwrap_or(json!({}));
            }
            if arguments.is_null() {
                arguments = json!({});
            }
            Some(ToolCall { name, arguments })
        })
        .collect()
}

fn parse_stats(v: &Value) -> Stats {
    let tokens = v["eval_count"].as_u64().unwrap_or(0);
    let nanos = v["eval_duration"].as_u64().unwrap_or(0);
    Stats { tokens, tokens_per_sec: if nanos == 0 || tokens < 2 { 0.0 } else { tokens as f64 / (nanos as f64 / 1e9) } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tool_calls_in_either_shape() {
        let m = json!({ "tool_calls": [
            { "function": { "name": "web_search", "arguments": { "query": "rust" } } },
            { "function": { "name": "calculator", "arguments": "{\"expression\": \"2+2\"}" } },
        ]});
        let calls = parse_calls(&m);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].arguments["query"], "rust");
        assert_eq!(calls[1].arguments["expression"], "2+2");
    }

    fn split_all(chunks: &[&str]) -> (String, String) {
        let mut s = ThinkSplitter::default();
        let (mut c, mut t) = (String::new(), String::new());
        for ch in chunks {
            let (a, b) = s.push(ch);
            c.push_str(&a);
            t.push_str(&b);
        }
        let (a, b) = s.finish();
        (c + &a, t + &b)
    }

    #[test]
    fn inline_thinking_is_moved_out() {
        assert_eq!(split_all(&["<|channel>thought\n<channel|>Hello!"]), ("Hello!".into(), "\n".into()));
        assert_eq!(split_all(&["<|chan", "nel>thought plan", "ning<chan", "nel|>\nHi"]), ("Hi".into(), " planning".into()));
        assert_eq!(split_all(&["<think>a</think>b"]), ("b".into(), "a".into()));
        assert_eq!(split_all(&["a < b and 2<3"]), ("a < b and 2<3".into(), String::new()));
        assert_eq!(split_all(&["ends with <"]), ("ends with <".into(), String::new()));
        assert_eq!(strip_thinking("<|channel>thought\nhmm<channel|>Greeting Exchange"), "Greeting Exchange");
    }

    #[test]
    fn stats_from_done_chunk() {
        let s = parse_stats(&json!({ "eval_count": 100, "eval_duration": 2_000_000_000u64 }));
        assert_eq!(s.tokens, 100);
        assert!((s.tokens_per_sec - 50.0).abs() < 1e-9);
    }

    #[test]
    fn assistant_tool_messages_round_trip() {
        let m = Msg {
            role: "assistant".into(),
            tool_calls: vec![ToolCall { name: "x".into(), arguments: json!({"a": 1}) }],
            ..Default::default()
        };
        let v = m.to_json();
        assert_eq!(v["tool_calls"][0]["function"]["name"], "x");
        let t = Msg { role: "tool".into(), content: "ok".into(), tool_name: "x".into(), ..Default::default() };
        assert_eq!(t.to_json()["tool_name"], "x");
    }
}
