import { defaultMcpConfig, normalizeMcpConfig } from "./mcp";

describe("MCP configuration", () => {
  it("defaults QingYu recycle-bin cleanup to thirty days", () => {
    expect(defaultMcpConfig().recycleBinRetentionDays).toBe(30);
  });

  it("accepts only the supported recycle-bin retention presets", () => {
    for (const recycleBinRetentionDays of [0, 7, 30, 90] as const) {
      expect(normalizeMcpConfig({ recycleBinRetentionDays }).recycleBinRetentionDays)
        .toBe(recycleBinRetentionDays);
    }

    expect(normalizeMcpConfig({ recycleBinRetentionDays: 180 }).recycleBinRetentionDays).toBe(30);
    expect(normalizeMcpConfig({}).recycleBinRetentionDays).toBe(30);
  });
});
