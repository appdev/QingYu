import { formatMcpClientConfiguration, formatMcpClientConnection } from "./mcp-client-config";
import type { McpClientConnection } from "./mcp";

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

describe("formatMcpClientConnection", () => {
  it("delegates to formatMcpClientConfiguration for stdio connections", () => {
    const connection: McpClientConnection = {
      transport: "stdio",
      command: "/usr/bin/mcp"
    };
    const result = formatMcpClientConnection(connection, "codex");

    expect(result).toBe('[mcp_servers.qingyu]\ncommand = "/usr/bin/mcp"');
  });

  it("formats HTTP connection JSON with token placeholder when tokenConfigured is true", () => {
    const connection: McpClientConnection = {
      transport: "http",
      url: "http://127.0.0.1:3211/mcp",
      tokenConfigured: true
    };
    const result = JSON.parse(formatMcpClientConnection(connection, "json"));

    expect(result).toEqual({
      mcpServers: {
        qingyu: {
          url: "http://127.0.0.1:3211/mcp",
          headers: { Authorization: "Bearer <QINGYU_MCP_TOKEN>" }
        }
      }
    });
  });

  it("formats HTTP connection JSON with empty headers when tokenConfigured is false", () => {
    const connection: McpClientConnection = {
      transport: "http",
      url: "http://192.168.1.100:3211/mcp",
      tokenConfigured: false
    };
    const result = JSON.parse(formatMcpClientConnection(connection, "json"));

    expect(result).toEqual({
      mcpServers: {
        qingyu: {
          url: "http://192.168.1.100:3211/mcp",
          headers: {}
        }
      }
    });
  });

  it("formats HTTP connection as Codex TOML with token note when tokenConfigured", () => {
    const connection: McpClientConnection = {
      transport: "http",
      url: "http://127.0.0.1:3211/mcp",
      tokenConfigured: true
    };
    const result = formatMcpClientConnection(connection, "codex");

    expect(JSON.parse(result)).toEqual({
      mcpServers: {
        qingyu: {
          url: "http://127.0.0.1:3211/mcp",
          headers: { Authorization: "Bearer <QINGYU_MCP_TOKEN>" }
        }
      }
    });
  });
});
