//! A minimal MCP server over stdio.
//!
//! **Three tools, and that is the whole surface.** An MCP server's tool schemas are
//! re-sent on every turn of every session, so a fifteen-tool server can cost more
//! context than the knowledge it retrieves — which would make Reify a counterexample to
//! its own thesis. Anything beyond these three belongs on the command line, where it
//! costs nothing until it is used.
//!
//! The CLI remains the primary surface (`docs/integration/`); this exists for clients
//! that cannot run a shell command.

use anyhow::Result;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use reify::context::{self, ContextOptions};
use reify::query;
use reify::store::Store;

/// The MCP revision this server speaks.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// Serve MCP on stdin/stdout until the client disconnects.
pub fn serve(root: &Path) -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            // A malformed line has no id, so there is nobody to reply to.
            continue;
        };
        // A notification carries no id and must produce no response.
        let Some(id) = request.get("id").cloned() else {
            continue;
        };
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = request.get("params").cloned().unwrap_or(Value::Null);

        let response = match dispatch(root, method, &params) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(error) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32000, "message": format!("{error:#}")}
            }),
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}

fn dispatch(root: &Path, method: &str, params: &Value) -> Result<Value> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "reify", "version": env!("CARGO_PKG_VERSION")}
        })),
        "tools/list" => Ok(json!({"tools": tool_definitions()})),
        "tools/call" => call_tool(root, params),
        "ping" => Ok(json!({})),
        other => anyhow::bail!("unsupported method `{other}`"),
    }
}

/// The three tools. Descriptions are written for a model deciding whether to call
/// them, not for a human reading documentation.
fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "reify_context",
            "description":
                "Compile the minimum system knowledge needed for a change, with citations \
                 and a reading plan. Call this BEFORE editing code in an unfamiliar area.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task": {"type": "string", "description": "What you are about to do"},
                    "budget": {"type": "integer", "description": "Token budget for the whole answer"}
                },
                "required": ["task"]
            }
        }),
        json!({
            "name": "reify_why",
            "description":
                "Explain one location: what it is, what calls it, what data it touches, \
                 which concepts and documents describe it, and what changed it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target": {"type": "string", "description": "path:line, a path, or a symbol name"}
                },
                "required": ["target"]
            }
        }),
        json!({
            "name": "reify_impact",
            "description":
                "List what depends on a symbol, including through shared database tables \
                 where no call edge exists. Call this before changing shared logic.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "A symbol name or a described change"}
                },
                "required": ["query"]
            }
        }),
    ]
}

fn call_tool(root: &Path, params: &Value) -> Result<Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
    let string_arg = |key: &str| -> Result<String> {
        arguments
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("`{key}` is required"))
    };

    // Validate the request before doing any work: an unknown tool or a missing
    // argument must be reported as such, not masked by whatever fails first.
    if !tool_definitions()
        .iter()
        .any(|t| t["name"].as_str() == Some(name))
    {
        anyhow::bail!("unknown tool `{name}`");
    }

    let store = open(root)?;
    let payload = match name {
        "reify_context" => {
            let budget = arguments
                .get("budget")
                .and_then(Value::as_u64)
                .unwrap_or(context::DEFAULT_BUDGET as u64) as u32;
            serde_json::to_value(context::compile(
                &store,
                &string_arg("task")?,
                &ContextOptions {
                    budget,
                    ..Default::default()
                },
            )?)?
        }
        "reify_why" => serde_json::to_value(query::why(&store, root, &string_arg("target")?)?)?,
        "reify_impact" => serde_json::to_value(query::impact(&store, &string_arg("query")?)?)?,
        // Unreachable: the name was validated against the tool list above.
        other => anyhow::bail!("unknown tool `{other}`"),
    };

    // MCP carries tool results as content blocks. JSON is handed over as text so the
    // client's own renderer does not reshape it.
    Ok(json!({
        "content": [{"type": "text", "text": serde_json::to_string_pretty(&payload)?}],
        "isError": false
    }))
}

fn open(root: &Path) -> Result<Store> {
    let path: PathBuf = root
        .join(reify::index::REIFY_DIR)
        .join(reify::index::STORE_FILE);
    anyhow::ensure!(
        path.exists(),
        "no index at {}; run `reify index` first",
        path.display()
    );
    Store::open(&path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_reports_the_protocol_and_tool_capability() {
        let result = dispatch(Path::new("/tmp"), "initialize", &Value::Null).unwrap();
        assert_eq!(result["protocolVersion"], PROTOCOL_VERSION);
        assert!(result["capabilities"]["tools"].is_object());
        assert_eq!(result["serverInfo"]["name"], "reify");
    }

    #[test]
    fn the_tool_surface_is_exactly_three_tools() {
        // Load-bearing: schemas are re-sent every turn, so this is a context budget,
        // not a style preference.
        let tools = tool_definitions();
        assert_eq!(
            tools.len(),
            3,
            "adding a fourth tool needs a written reason"
        );
        for tool in &tools {
            assert!(tool["name"]
                .as_str()
                .is_some_and(|n| n.starts_with("reify_")));
            assert!(tool["description"].as_str().is_some_and(|d| d.len() > 40));
            assert!(tool["inputSchema"]["required"].is_array());
        }
    }

    #[test]
    fn the_tool_schemas_stay_small_enough_to_be_worth_sending() {
        let rendered = serde_json::to_string(&tool_definitions()).unwrap();
        let cost = reify::tokens::estimate(&rendered);
        assert!(cost < 600, "tool schemas cost {cost} tokens every turn");
    }

    #[test]
    fn an_unknown_method_is_an_error_not_a_silent_success() {
        assert!(dispatch(Path::new("/tmp"), "tools/nonsense", &Value::Null).is_err());
    }

    #[test]
    fn an_unknown_tool_is_rejected_by_name() {
        let params = json!({"name": "reify_delete_everything", "arguments": {}});
        let err = call_tool(Path::new("/tmp"), &params)
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown tool"), "{err}");
    }

    #[test]
    fn an_unvalidated_request_never_reaches_the_store() {
        // Ordering matters: a bad request must be rejected on its own terms rather
        // than surfacing whatever unrelated thing happens to fail first.
        let params = json!({"name": "reify_why", "arguments": {}});
        let err = call_tool(Path::new("/tmp"), &params)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no index"), "{err}");
    }
}
