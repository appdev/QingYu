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
