use luminus::mcp::client::McpClient;
use luminus::mcp::config::{McpConfig, McpServerConfig};
use luminus::mcp::manager::McpManager;
use std::collections::HashMap;

#[tokio::test]
async fn mcp_client_connects_and_lists_tools() {
    // We create a mock python MCP stdio server
    let script = r#"
import sys, json

def main():
    while True:
        line = sys.stdin.readline()
        if not line:
            break
        req = json.loads(line)
        method = req.get("method")
        req_id = req.get("id")

        if method == "initialize":
            resp = {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "serverInfo": {"name": "mock-server", "version": "1.0"}
                }
            }
            print(json.dumps(resp), flush=True)
        elif method == "notifications/initialized":
            pass
        elif method == "tools/list":
            resp = {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "tools": [
                        {
                            "name": "echo_tool",
                            "description": "Echoes back input",
                            "input_schema": {"type": "object"}
                        }
                    ]
                }
            }
            print(json.dumps(resp), flush=True)
        elif method == "tools/call":
            args = req.get("params", {}).get("arguments", {})
            resp = {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "content": [
                        {"type": "text", "text": f"Hello from mock tool! input: {args}"}
                    ]
                }
            }
            print(json.dumps(resp), flush=True)

if __name__ == "__main__":
    main()
"#;

    let dir = std::env::temp_dir().join(format!("mcp-int-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let script_path = dir.join("mock_mcp.py");
    std::fs::write(&script_path, script).unwrap();

    let client = McpClient::connect(
        "python",
        &[script_path.to_str().unwrap().to_string()],
        &HashMap::new(),
    )
    .await;

    assert!(client.is_ok(), "failed to connect: {:?}", client.err());
    let client = client.unwrap();

    let tools = client.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo_tool");

    let output = client
        .call_tool("echo_tool", serde_json::json!({"msg": "hi"}))
        .await
        .unwrap();
    assert!(output.contains("Hello from mock tool!"));
    assert!(output.contains("hi"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mcp_config_lists_servers_correctly() {
    let dir = std::env::temp_dir().join(format!("mcp-cfg-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let mut config = McpConfig::default();
    config.mcp_servers.insert(
        "ares-mcp".into(),
        McpServerConfig {
            command: "node".into(),
            args: vec!["/path/to/ares/index.js".into()],
            env: HashMap::new(),
        },
    );
    config.save_project(&dir).unwrap();

    let loaded = McpConfig::load(&dir);
    let list_str = loaded.list_servers();
    assert!(list_str.contains("ares-mcp"));
    assert!(list_str.contains("node /path/to/ares/index.js"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn mcp_manager_discovers_and_calls_dynamic_tool() {
    let script = r#"
import sys, json
for line in sys.stdin:
    req = json.loads(line); method = req.get("method"); rid = req.get("id")
    if method == "initialize":
        print(json.dumps({"jsonrpc":"2.0","id":rid,"result":{"protocolVersion":"2024-11-05","capabilities":{},"serverInfo":{"name":"mock","version":"1"}}}), flush=True)
    elif method == "tools/list":
        print(json.dumps({"jsonrpc":"2.0","id":rid,"result":{"tools":[{"name":"test_tool","description":"Test tool","input_schema":{}}]}}), flush=True)
    elif method == "tools/call":
        print(json.dumps({"jsonrpc":"2.0","id":rid,"result":{"content":[{"type":"text","text":"Called test_tool"}]}}), flush=True)
"#;
    let dir = std::env::temp_dir().join(format!("mcp-mgr-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let script_path = dir.join("mock.py");
    std::fs::write(&script_path, script).unwrap();
    let mut config = McpConfig::default();
    config.mcp_servers.insert(
        "mock".into(),
        McpServerConfig {
            command: "python".into(),
            args: vec![script_path.to_str().unwrap().into()],
            env: HashMap::new(),
        },
    );
    let mut manager = McpManager::new();
    let results = manager.connect_all(&config).await;
    assert!(results[0].1.is_ok());
    let specs = manager.dynamic_tool_specs();
    assert_eq!(specs[0].name, "mcp:mock:test_tool");
    assert_eq!(
        manager
            .call_tool("mcp:mock:test_tool", serde_json::json!({}))
            .await
            .unwrap(),
        "Called test_tool"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
