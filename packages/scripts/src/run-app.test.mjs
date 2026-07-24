import { describe, expect, it } from "vitest";

import { APP_USAGE, AppUsageError, resolveAppArgs } from "./run-app.mjs";

describe("resolveAppArgs", () => {
  it.each([
    [["dev", "desktop"], ["dev"]],
    [["build", "desktop", "--no-sign"], ["build", "--no-sign"]],
    [["dev", "android", "--open"], ["android", "dev", "--open"]],
    [
      ["build", "android", "--apk", "--target", "aarch64", "--ci"],
      ["android", "build", "--apk", "--target", "aarch64", "--ci"],
    ],
    [["dev", "ios", "--open"], ["ios", "dev", "--open"]],
    [
      ["build", "ios", "--target", "aarch64-sim", "--no-sign", "--ci"],
      ["ios", "build", "--target", "aarch64-sim", "--no-sign", "--ci"],
    ],
  ])("maps %j to %j", (input, expected) => {
    expect(resolveAppArgs(input, "darwin")).toEqual(expected);
  });

  it.each([
    [[]],
    [["dev"]],
    [["serve", "desktop"]],
    [["build", "web"]],
  ])("rejects invalid arguments %j", (input) => {
    expect(() => resolveAppArgs(input, "darwin")).toThrow(AppUsageError);
  });

  it("rejects iOS commands outside macOS", () => {
    expect(() => resolveAppArgs(["build", "ios"], "linux")).toThrow(
      "iOS commands require macOS",
    );
  });

  it("documents the stable public syntax", () => {
    expect(APP_USAGE).toContain("pnpm app <dev|build> <desktop|android|ios>");
  });
});
