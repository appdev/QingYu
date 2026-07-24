import { formatMcpClientConfiguration } from "./mcp-client-config";

describe("formatMcpClientConfiguration", () => {
  it("formats a Codex TOML server with an escaped command", () => {
    expect(formatMcpClientConfiguration(
      "C:\\Program Files\\QingYu\\qingyu-mcp.exe",
      "codex"
    )).toBe(
      '[mcp_servers.qingyu]\ncommand = "C:\\\\Program Files\\\\QingYu\\\\qingyu-mcp.exe"'
    );
  });

  it("formats generic MCP JSON without credentials or arguments", () => {
    const value = JSON.parse(formatMcpClientConfiguration(
      "/Applications/QingYu.app/Contents/MacOS/qingyu-mcp",
      "json"
    ));

    expect(value).toEqual({
      mcpServers: {
        qingyu: {
          command: "/Applications/QingYu.app/Contents/MacOS/qingyu-mcp"
        }
      }
    });
  });

  it("uses the bundled absolute bridge path without a PATH-installed wrapper", () => {
    const command = "/Applications/QingYu.app/Contents/MacOS/qingyu-mcp";
    const codex = formatMcpClientConfiguration(command, "codex");
    const generic = JSON.parse(formatMcpClientConfiguration(command, "json"));

    expect(codex).toBe(
      `[mcp_servers.qingyu]\ncommand = ${JSON.stringify(command)}`
    );
    expect(generic.mcpServers.qingyu).toEqual({ command });
    expect(codex).not.toContain("args =");
    expect(codex).not.toContain("env =");
    expect(codex).not.toContain("command = \"markra\"");
  });
});
