import { defaultMcpConfig, normalizeMcpConfig } from "./mcp";

describe("MCP configuration", () => {
  it("defaults to the Kernel-owned recoverable recycle bin without fake retention", () => {
    expect(defaultMcpConfig().deletion).toBe("qing-yu-recycle-bin");
    expect(defaultMcpConfig().recycleBinRetentionDays).toBe(0);
  });

  it("migrates legacy system trash and retention to the portable Kernel contract", () => {
    const migrated = normalizeMcpConfig({
      deletion: "system-trash",
      recycleBinRetentionDays: 30
    });
    expect(migrated.deletion).toBe("qing-yu-recycle-bin");
    expect(migrated.recycleBinRetentionDays).toBe(0);
  });
});
