import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { t, type I18nKey } from "@markra/shared";
import { McpClientConfiguration } from "./McpClientConfiguration";

const englishTranslate = (key: I18nKey) => t("en", key);

describe("McpClientConfiguration", () => {
  it("renders an HTTP MCP client connection", () => {
    render(
      <McpClientConfiguration
        connections={[{ transport: "http", url: "http://127.0.0.1:3211/mcp", tokenConfigured: true }]}
        translate={englishTranslate}
      />
    );
    expect(screen.getByText("http://127.0.0.1:3211/mcp")).toBeInTheDocument();
  });

  it("copies the selected Codex configuration for a stdio connection", async () => {
    const writeClipboard = vi.fn(async (_text: string) => undefined);
    render(
      <McpClientConfiguration
        connections={[{ transport: "stdio", command: "/Applications/QingYu.app/Contents/MacOS/qingyu-mcp" }]}
        translate={englishTranslate}
        writeClipboard={writeClipboard}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Copy configuration" }));

    await waitFor(() => expect(writeClipboard).toHaveBeenCalledWith(
      '[mcp_servers.qingyu]\ncommand = "/Applications/QingYu.app/Contents/MacOS/qingyu-mcp"'
    ));
    expect(screen.getByRole("status")).toHaveTextContent("Configuration copied.");
  });

  it("switches to generic JSON and copies an AI installation request for stdio", async () => {
    const writeClipboard = vi.fn(async (_text: string) => undefined);
    render(
      <McpClientConfiguration
        connections={[{ transport: "stdio", command: "/opt/qingyu/qingyu-mcp" }]}
        translate={englishTranslate}
        writeClipboard={writeClipboard}
      />
    );

    fireEvent.change(screen.getByLabelText("Configuration format"), {
      target: { value: "json" }
    });
    expect(screen.getByText(/"mcpServers"/u)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Copy for AI tool" }));

    await waitFor(() => expect(writeClipboard).toHaveBeenCalledWith(
      expect.stringContaining('"command": "/opt/qingyu/qingyu-mcp"')
    ));
    expect(vi.mocked(writeClipboard).mock.calls[0][0]).toContain("Do not add a URL or token");
    expect(screen.getByRole("status")).toHaveTextContent("Instructions copied.");
  });

  it("shows an error when the clipboard rejects the write", async () => {
    const writeClipboard = vi.fn(async (_text: string) => {
      throw new Error("clipboard unavailable");
    });
    render(
      <McpClientConfiguration
        connections={[{ transport: "stdio", command: "/opt/qingyu/qingyu-mcp" }]}
        translate={englishTranslate}
        writeClipboard={writeClipboard}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Copy configuration" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not copy the configuration.");
  });

  it("renders a list of connections with transport badges", () => {
    render(
      <McpClientConfiguration
        connections={[
          { transport: "stdio", command: "/usr/local/bin/qingyu-mcp" },
          { transport: "http", url: "http://192.168.1.100:3211/mcp", tokenConfigured: true }
        ]}
        translate={englishTranslate}
      />
    );

    const badges = screen.getAllByText("stdio");
    expect(badges.length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText("http").length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("/usr/local/bin/qingyu-mcp")).toBeInTheDocument();
    expect(screen.getByText("http://192.168.1.100:3211/mcp")).toBeInTheDocument();
  });

  it("shows Bearer token indicator for HTTP connections with tokenConfigured", () => {
    render(
      <McpClientConfiguration
        connections={[{ transport: "http", url: "http://127.0.0.1:3211/mcp", tokenConfigured: true }]}
        translate={englishTranslate}
      />
    );

    expect(screen.getAllByText("Bearer token configured").length).toBeGreaterThanOrEqual(1);
  });

  it("switches between connections and formats config accordingly", () => {
    const writeClipboard = vi.fn(async (_text: string) => undefined);
    render(
      <McpClientConfiguration
        connections={[
          { transport: "stdio", command: "/usr/bin/mcp" },
          { transport: "http", url: "http://server:3211/mcp", tokenConfigured: false }
        ]}
        translate={englishTranslate}
        writeClipboard={writeClipboard}
      />
    );

    // Default: first connection (stdio) selected
    expect(screen.getByText("/usr/bin/mcp")).toBeInTheDocument();

    // Click second connection (HTTP)
    fireEvent.click(screen.getByText("http://server:3211/mcp"));

    // Should now show HTTP config
    fireEvent.click(screen.getByRole("button", { name: "Copy configuration" }));
    expect(writeClipboard).toHaveBeenCalledWith(expect.stringContaining('"url": "http://server:3211/mcp"'));
  });

  it("returns null when there are no connections", () => {
    const { container } = render(
      <McpClientConfiguration
        connections={[]}
        translate={englishTranslate}
      />
    );

    expect(container.firstChild).toBeNull();
  });
});
