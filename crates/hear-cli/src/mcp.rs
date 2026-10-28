//! Minimal stdio MCP server: fixed, read-only tools; no network listener.
use hear_protocol::context::{self, Query};
use serde_json::{Value, json};
use std::io::{BufRead, Read, Write};
const MAX_MESSAGE: u64 = 1_048_576;
#[derive(Default)]
struct Server {
    initialized: bool,
    ready: bool,
}
impl Server {
    fn handle(&mut self, v: Value) -> Option<Value> {
        let id = v.get("id").cloned();
        let method = v.get("method").and_then(Value::as_str);
        if v["jsonrpc"] != "2.0"
            || method.is_none()
            || id
                .as_ref()
                .is_some_and(|id| !id.is_string() && !id.is_i64() && !id.is_u64())
        {
            return Some(failure(
                id.unwrap_or(Value::Null),
                -32600,
                "Invalid JSON-RPC request",
            ));
        }
        let method = method.unwrap();
        if id.is_none() {
            if method == "notifications/initialized" && self.initialized {
                self.ready = true;
            }
            return None;
        }
        let id = id.unwrap();
        let params = v.get("params").cloned().unwrap_or_else(|| json!({}));
        let result = match method {
            "initialize" => {
                if self.initialized {
                    return Some(failure(id, -32600, "Already initialized"));
                }
                let Some(version) = params["protocolVersion"].as_str() else {
                    return Some(failure(id, -32602, "protocolVersion required"));
                };
                if !params["capabilities"].is_object() || !params["clientInfo"].is_object() {
                    return Some(failure(id, -32602, "capabilities and clientInfo required"));
                }
                self.initialized = true;
                json!({"protocolVersion":if ["2025-11-25","2025-06-18"].contains(&version){version}else{"2025-11-25"},"capabilities":{"tools":{}},"serverInfo":{"name":"hear","title":"hear context","version":env!("CARGO_PKG_VERSION"),"description":context::SERVER_DESCRIPTION},"instructions":context::INSTRUCTIONS})
            }
            "ping" => json!({}),
            _ if !self.ready => {
                return Some(failure(
                    id,
                    -32000,
                    "Initialize and send notifications/initialized first",
                ));
            }
            "tools/list" => {
                if params.get("cursor").is_some() {
                    return Some(failure(id, -32602, "The fixed tool list has no cursor"));
                }
                json!({"tools":context::tools()})
            }
            "tools/call" => {
                let Some(name) = params["name"].as_str() else {
                    return Some(failure(id, -32602, "Tool name required"));
                };
                let Some((op, _, _)) = context::TOOLS.iter().find(|(_, n, _)| *n == name) else {
                    return Some(failure(id, -32602, "Unknown tool"));
                };
                let result = match Query::parse(
                    op,
                    params
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| json!({})),
                ) {
                    Ok(q) => context::execute(q),
                    Err(e) => context::error(uuid::Uuid::new_v4(), "invalid_argument", e),
                };
                json!({"isError":result["ok"]==false,"content":[{"type":"text","text":serde_json::to_string(&result).unwrap()}],"structuredContent":result})
            }
            _ => return Some(failure(id, -32601, "Method not found")),
        };
        Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
    }
}
fn failure(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
pub fn serve() -> Result<(), String> {
    serve_io(std::io::stdin().lock(), std::io::stdout().lock())
}
fn serve_io(mut input: impl BufRead, mut output: impl Write) -> Result<(), String> {
    let mut server = Server::default();
    loop {
        let mut line = Vec::new();
        let n = input
            .by_ref()
            .take(MAX_MESSAGE + 1)
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(());
        }
        if n as u64 > MAX_MESSAGE {
            return Err("MCP message exceeds 1 MiB".into());
        }
        let response = match serde_json::from_slice(&line) {
            Ok(v) => server.handle(v),
            Err(_) => Some(failure(Value::Null, -32700, "Invalid JSON")),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut output, &response).map_err(|e| e.to_string())?;
            output.write_all(b"\n").map_err(|e| e.to_string())?;
            output.flush().map_err(|e| e.to_string())?;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handshake_and_tool_discovery_without_core() {
        let input = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"1\"}}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n"
        );
        let mut out = Vec::new();
        serve_io(std::io::Cursor::new(input), &mut out).unwrap();
        let lines: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[1]["result"]["tools"].as_array().unwrap().len(),
            context::TOOLS.len()
        );
    }
    #[test]
    fn malformed_and_uninitialized_requests() {
        let mut s = Server::default();
        assert_eq!(
            s.handle(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
                .unwrap()["error"]["code"],
            -32000
        );
        assert_eq!(s.handle(json!([])).unwrap()["error"]["code"], -32600);
    }
}
