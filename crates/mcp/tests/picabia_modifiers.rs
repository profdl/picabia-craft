//! PICABIA: an MCP smoke test: an agent adds a linear array to a layer and reads the stack back.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use figcraft_mcp::Server;
use serde_json::{Value, json};

fn rpc(s: &mut Server, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string();
    serde_json::from_str(&s.handle_line(&line).unwrap()).unwrap()
}

fn run(s: &mut Server, command: &str, params: Value) -> Value {
    let r = rpc(s, "tools/call", json!({"name": "run_command", "arguments": {"command": command, "params": params}}));
    assert_eq!(r["result"]["isError"], false, "{r}");
    serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn an_agent_adds_a_linear_array() {
    let mut s = Server::headless();
    let id = run(&mut s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 50, "height": 50}))["id"].clone();
    let added = run(&mut s, "modifier.add", json!({"ids": [id], "kind": "linearArray", "props": {"count": 4, "offsetX": 60}}));
    assert_eq!(added["modifier"], "m1");
    let list = run(&mut s, "modifier.list", json!({"id": id}));
    assert_eq!(list["instances"], 4);
    assert_eq!(list["modifiers"][0]["kind"], "linearArray");
    assert_eq!(list["modifiers"][0]["offsetX"], 60.0);
    // The stack is plain layer data: node.get shows it too.
    let node = run(&mut s, "node.get", json!({"id": id}));
    assert!(node["pluginData"]["picabia.modifiers"].as_str().unwrap().contains("linearArray"), "{node}");
    // A bad param is an error result, not a crash.
    let r = rpc(
        &mut s,
        "tools/call",
        json!({"name": "run_command", "arguments": {"command": "modifier.set", "params": {"ids": [id], "index": 0, "props": {"count": "NaN"}}}}),
    );
    assert_eq!(r["result"]["isError"], true, "{r}");
}
