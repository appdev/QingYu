import type { McpClientConnection } from "./mcp";

export type McpClientConfigFormat = "codex" | "json";

export function formatMcpClientConfiguration(
  command: string,
  format: McpClientConfigFormat
) {
  if (format === "codex") {
    return `[mcp_servers.qingyu]\ncommand = ${JSON.stringify(command)}`;
  }
  return JSON.stringify({ mcpServers: { qingyu: { command } } }, null, 2);
}

export function formatMcpClientConnection(
  connection: McpClientConnection,
  format: McpClientConfigFormat
): string {
  if (connection.transport === "stdio") {
    return formatMcpClientConfiguration(connection.command, format);
  }
  return JSON.stringify({
    mcpServers: {
      qingyu: {
        url: connection.url,
        headers: connection.tokenConfigured ? { Authorization: "Bearer <QINGYU_MCP_TOKEN>" } : {}
      }
    }
  }, null, 2);
}
