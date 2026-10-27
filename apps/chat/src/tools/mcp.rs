//! Model Context Protocol servers over stdio. Each enabled server is started
//! once; its tools are offered to the model as `mcp__<server>__<tool>`.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::Output;
use crate::prefs::McpServer;

struct Tool {
    name: String,
    description: String,
    schema: Value,
}

struct Server {
    name: String,
    tools: Mutex<Vec<Tool>>,
    stdin: tokio::sync::Mutex<tokio::process::ChildStdin>,
    pending: Mutex<HashMap<u64, tokio::sync::oneshot::Sender<Value>>>,
    next: AtomicU64,
    _child: Mutex<tokio::process::Child>,
}

static SERVERS: Mutex<Vec<Arc<Server>>> = Mutex::new(Vec::new());

/// "Running · 4 tools" or the error, per server name.
static STATUS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

fn slug(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect()
}

fn full_name(server: &str, tool: &str) -> String {
    format!("mcp__{}__{}", slug(server), tool)
}

pub fn status(name: &str) -> Option<String> {
    STATUS.lock().ok()?.iter().find(|(n, _)| n == name).map(|(_, s)| s.clone())
}

pub fn definitions() -> Vec<Value> {
    let servers = SERVERS.lock().map(|s| s.clone()).unwrap_or_default();
    servers
        .iter()
        .flat_map(|s| {
            let tools = s.tools.lock().map(|t| {
                t.iter()
                    .map(|t| {
                        json!({ "type": "function", "function": {
                            "name": full_name(&s.name, &t.name),
                            "description": t.description,
                            "parameters": t.schema,
                        }})
                    })
                    .collect::<Vec<_>>()
            });
            tools.unwrap_or_default()
        })
        .collect()
}

pub fn owns(name: &str) -> bool {
    name.starts_with("mcp__")
}

pub fn title_for(name: &str) -> Option<String> {
    let rest = name.strip_prefix("mcp__")?;
    let (server, tool) = rest.split_once("__")?;
    Some(format!("{} · {}", tool.replace('_', " "), server.replace('_', " ")))
}

pub async fn call(name: &str, args: &Value) -> Result<Output> {
    let rest = name.strip_prefix("mcp__").context("not an MCP tool")?;
    let (server, tool) = rest.split_once("__").context("bad MCP tool name")?;
    let s = SERVERS
        .lock()
        .ok()
        .and_then(|list| list.iter().find(|s| slug(&s.name) == server).cloned())
        .context("that MCP server isn't running")?;
    let result = request(&s, "tools/call", json!({ "name": tool, "arguments": args })).await?;
    let text = result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| match c["type"].as_str() {
            Some("text") => c["text"].as_str().unwrap_or("").to_string(),
            Some(kind) => format!("[{kind} content]"),
            None => String::new(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let failed = result["isError"].as_bool().unwrap_or(false);
    Ok(Output { text: crate::util::clip(&text, 30_000), failed, ..Default::default() })
}

async fn request(s: &Server, method: &str, params: Value) -> Result<Value> {
    let id = s.next.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = tokio::sync::oneshot::channel();
    s.pending.lock().map_err(|_| anyhow::anyhow!("poisoned"))?.insert(id, tx);
    let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string() + "\n";
    s.stdin.lock().await.write_all(line.as_bytes()).await?;
    let reply = tokio::time::timeout(Duration::from_secs(120), rx).await.context("the MCP server didn't answer in time")??;
    if let Some(e) = reply.get("error") {
        bail!("{}", e["message"].as_str().unwrap_or("MCP error"));
    }
    Ok(reply["result"].clone())
}

async fn notify(s: &Server, method: &str) -> Result<()> {
    let line = json!({ "jsonrpc": "2.0", "method": method }).to_string() + "\n";
    s.stdin.lock().await.write_all(line.as_bytes()).await?;
    Ok(())
}

async fn start_one(cfg: &McpServer) -> Result<Arc<Server>> {
    let words = crate::util::shell_words(&cfg.command);
    let (program, args) = words.split_first().context("no command")?;
    let mut child = tokio::process::Command::new(crate::paths::expand(program))
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("couldn't start {program}"))?;
    let stdin = child.stdin.take().context("no stdin")?;
    let stdout = child.stdout.take().context("no stdout")?;
    let server = Arc::new(Server {
        name: cfg.name.clone(),
        tools: Mutex::new(Vec::new()),
        stdin: tokio::sync::Mutex::new(stdin),
        pending: Mutex::new(HashMap::new()),
        next: AtomicU64::new(1),
        _child: Mutex::new(child),
    });
    let weak = Arc::downgrade(&server);
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            let Some(s) = weak.upgrade() else { break };
            if let Some(id) = v["id"].as_u64()
                && v.get("method").is_none()
                && let Some(tx) = s.pending.lock().ok().and_then(|mut p| p.remove(&id))
            {
                let _ = tx.send(v);
            }
        }
    });
    request(
        &server,
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "nyx-chat", "version": env!("CARGO_PKG_VERSION") },
        }),
    )
    .await?;
    notify(&server, "notifications/initialized").await?;
    let list = request(&server, "tools/list", json!({})).await?;
    let tools: Vec<Tool> = list["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            Some(Tool {
                name: t["name"].as_str()?.to_string(),
                description: t["description"].as_str().unwrap_or("").to_string(),
                schema: if t["inputSchema"].is_object() { t["inputSchema"].clone() } else { json!({ "type": "object" }) },
            })
        })
        .collect();
    if let Ok(mut t) = server.tools.lock() {
        *t = tools;
    }
    Ok(server)
}

/// Stop every server and start the enabled ones again.
pub async fn restart(configs: Vec<McpServer>) {
    if let Ok(mut s) = SERVERS.lock() {
        s.clear();
    }
    if let Ok(mut st) = STATUS.lock() {
        st.clear();
    }
    for cfg in configs.into_iter().filter(|c| c.enabled && !c.command.trim().is_empty()) {
        let status = match start_one(&cfg).await {
            Ok(server) => {
                let n = server.tools.lock().map(|t| t.len()).unwrap_or(0);
                if let Ok(mut s) = SERVERS.lock() {
                    s.push(server);
                }
                format!("Running · {n} tool{}", if n == 1 { "" } else { "s" })
            }
            Err(e) => format!("Failed: {e:#}"),
        };
        if let Ok(mut st) = STATUS.lock() {
            st.push((cfg.name.clone(), status));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(full_name("My Files", "read"), "mcp__my_files__read");
        assert!(owns("mcp__x__y"));
        assert_eq!(title_for("mcp__my_files__read_text").unwrap(), "read text · my files");
    }

    #[test]
    fn talks_to_a_stdio_server() {
        // A tiny MCP server in bash: answers initialize, tools/list and tools/call.
        let script = r#"while read -r line; do
            id=$(echo "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
            case "$line" in
              *initialize*) echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{}}" ;;
              *tools/list*) echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"tools\":[{\"name\":\"echo\",\"description\":\"Echo\",\"inputSchema\":{\"type\":\"object\"}}]}}" ;;
              *tools/call*) echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"pong\"}]}}" ;;
            esac
          done"#;
        let dir = std::env::temp_dir().join(format!("nyx-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("server.sh");
        std::fs::write(&path, script).unwrap();
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        rt.block_on(async {
            restart(vec![McpServer { name: "Test".into(), command: format!("bash {}", path.display()), enabled: true }]).await;
            assert_eq!(status("Test").unwrap(), "Running · 1 tool");
            let defs = definitions();
            assert_eq!(defs[0]["function"]["name"], "mcp__test__echo");
            let out = call("mcp__test__echo", &json!({})).await.unwrap();
            assert_eq!(out.text, "pong");
            restart(vec![]).await;
        });
        let _ = std::fs::remove_dir_all(dir);
    }
}
