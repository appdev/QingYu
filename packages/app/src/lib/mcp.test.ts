import { defaultMcpConfig, normalizeMcpConfig } from "./mcp";

describe("MCP configuration", () => {
  it("defaults to the Kernel-owned recoverable recycle bin without fake retention", () => {
    expect(defaultMcpConfig().deletion).toBe("recoverable");
    expect(defaultMcpConfig()).not.toHaveProperty("recycleBinRetentionDays");
  });

  it("normalizes unsupported deletion policies to the canonical recoverable default", () => {
    expect(normalizeMcpConfig({ deletion: "unsupported" }).deletion).toBe("recoverable");
  });
});
