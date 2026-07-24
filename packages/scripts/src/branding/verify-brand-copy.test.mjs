import { describe, expect, test } from "vitest";
import {
  scanTextForBrandCopyViolations,
  scanTextForLegacyBrandReferences
} from "./verify-brand-copy.mjs";

const legacyDisplayName = ["Mar", "kra"].join("");

describe("scanTextForLegacyBrandReferences", () => {
  test("reports standalone legacy display names with source locations", () => {
    const violations = scanTextForLegacyBrandReferences(
      `header\n${legacyDisplayName} Notes\nfooter`,
      "copy.txt"
    );

    expect(violations).toEqual([{
      column: 1,
      context: `${legacyDisplayName} Notes`,
      kind: "legacy-display-name",
      line: 2,
      path: "copy.txt"
    }]);
  });

  test("ignores protected technical identifiers", () => {
    const text = [
      "markra",
      "@markra/shared",
      "dev.markra.app",
      ".markraignore",
      ".markra-sync",
      "markra:file",
      "aboutMarkra",
      "MARKRA_GITHUB_URL"
    ].join("\n");

    expect(scanTextForLegacyBrandReferences(text, "identifiers.txt")).toEqual([]);
  });
});

describe("scanTextForBrandCopyViolations", () => {
  test.each([
    "packages/shared/src/i18n/locales/zh-CN.ts",
    "packages/shared/src/i18n/locales/zh-TW.ts",
    "apps/desktop/src-tauri/src/menu_labels/zh_cn.rs",
    "apps/desktop/src-tauri/src/menu_labels/zh_tw.rs",
    "apps/desktop/src-tauri/macos-locales/zh-Hans.lproj/InfoPlist.strings",
    "apps/desktop/src-tauri/macos-locales/zh-Hant.lproj/InfoPlist.strings"
  ])("rejects the nonlocalized display name in %s", (path) => {
    const violations = scanTextForBrandCopyViolations(
      "QingYu 同步保留 .qingyu/config.json 和 QingYuProjectConfig。",
      path
    );

    expect(violations).toEqual([{
      column: 1,
      context: "QingYu 同步保留 .qingyu/config.json 和 QingYuProjectConfig。",
      kind: "chinese-locale-display-name",
      line: 1,
      path
    }]);
  });

  test("allows the nonlocalized display name in other locales", () => {
    expect(scanTextForBrandCopyViolations(
      "QingYu keeps .qingyu/config.json and QingYuProjectConfig.",
      "packages/shared/src/i18n/locales/en.ts"
    )).toEqual([]);
  });
});
