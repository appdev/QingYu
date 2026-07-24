import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { t, type I18nKey } from "@markra/shared";
import { McpClientConfiguration } from "./McpClientConfiguration";

const englishTranslate = (key: I18nKey) => t("en", key);

describe("McpClientConfiguration", () => {
  it("copies the selected Codex configuration", async () => {
    const writeClipboard = vi.fn(async (_text: string) => undefined);
    render(
      <McpClientConfiguration
        command="/Applications/QingYu.app/Contents/MacOS/qingyu-mcp"
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

  it("switches to generic JSON and copies an AI installation request", async () => {
    const writeClipboard = vi.fn(async (_text: string) => undefined);
    render(
      <McpClientConfiguration
        command="/opt/qingyu/qingyu-mcp"
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
        command="/opt/qingyu/qingyu-mcp"
        translate={englishTranslate}
        writeClipboard={writeClipboard}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Copy configuration" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not copy the configuration.");
  });
});
