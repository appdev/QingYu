import { detectDownloadPlatform, orderDownloadPlatforms } from "./platform";

describe("download platform", () => {
  it.each([
    ["MacIntel", 0, "macos"],
    ["MacIntel", 5, null],
    ["Win32", 0, "windows"],
    ["Linux x86_64", 0, "linux"],
    ["iPhone", 5, null]
  ])("maps %s with %i touch points to %s", (platform, maxTouchPoints, expected) => {
    expect(detectDownloadPlatform({ platform, maxTouchPoints })).toBe(expected);
  });

  it("moves the detected desktop platform first without hiding others", () => {
    expect(orderDownloadPlatforms("windows")).toEqual(["windows", "macos", "linux"]);
    expect(orderDownloadPlatforms(null)).toEqual(["macos", "windows", "linux"]);
  });
});
