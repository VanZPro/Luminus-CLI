use std::collections::HashMap;

use super::client::{McpClient, McpTool};
use super::config::McpConfig;
use crate::tools::{Permission, ToolSpec};

#[derive(Default)]
pub struct McpManager {
    clients: HashMap<String, McpClient>,
    tools: HashMap<String, (String, McpTool)>, // full_tool_name -> (server_name, tool)
}

impl McpManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load configuration and connect to all defined MCP servers.
    pub async fn connect_all(&mut self, config: &McpConfig) -> Vec<(String, Result<(), String>)> {
        let mut results = Vec::new();
        for (name, cfg) in &config.mcp_servers {
            match McpClient::connect(&cfg.command, &cfg.args, &cfg.env).await {
                Ok(client) => {
                    // Fetch tools
                    match client.list_tools().await {
                        Ok(tools_list) => {
                            for t in tools_list {
                                let full_name = format!("mcp:{}:{}", name, t.name);
                                self.tools.insert(full_name, (name.clone(), t));
                            }
                            self.clients.insert(name.clone(), client);
                            results.push((name.clone(), Ok(())));
                        }
                        Err(e) => {
                            results.push((name.clone(), Err(format!("Failed to list tools: {e}"))));
                        }
                    }
                }
                Err(e) => {
                    results.push((name.clone(), Err(format!("Connection failed: {e}"))));
                }
            }
        }
        results
    }

    /// Get dynamically registered `ToolSpec`s for all discovered MCP tools.
    pub fn dynamic_tool_specs(&self) -> Vec<ToolSpec> {
        let mut specs = Vec::new();
        for (full_name, (_server, tool)) in &self.tools {
            let desc = tool.description.as_deref().unwrap_or("MCP tool");
            let name_static: &'static str = Box::leak(full_name.clone().into_boxed_str());
            let desc_static: &'static str = Box::leak(desc.to_owned().into_boxed_str());
            specs.push(ToolSpec {
                name: name_static,
                description: desc_static,
                permission: Permission::Execute,
            });
        }
        specs
    }

    /// Call an MCP tool by its prefixed name (`mcp:<server>:<tool>`).
    pub async fn call_tool(
        &self,
        full_name: &str,
        arguments: serde_json::Value,
    ) -> Result<String, String> {
        let (server_name, tool) = self
            .tools
            .get(full_name)
            .ok_or_else(|| format!("Unknown MCP tool: {full_name}"))?;

        let client = self
            .clients
            .get(server_name)
            .ok_or_else(|| format!("MCP server '{server_name}' is not connected"))?;

        client
            .call_tool(&tool.name, arguments)
            .await
            .map_err(|e| e.to_string())
    }
}
