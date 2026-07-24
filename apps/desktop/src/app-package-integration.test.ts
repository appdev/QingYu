import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const desktopRoot = process.cwd();
const workspaceRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");

function readBundleNames(locale: string) {
  const contents = readFileSync(
    resolve(desktopRoot, `src-tauri/macos-locales/${locale}.lproj/InfoPlist.strings`),
    "utf8"
  );

  return {
    displayName: contents.match(/"CFBundleDisplayName"\s*=\s*"([^"]+)";/)?.[1],
    name: contents.match(/"CFBundleName"\s*=\s*"([^"]+)";/)?.[1]
  };
}

describe("desktop app package integration", () => {
  it("renders the frontend from @markra/app", () => {
    const entry = readFileSync(resolve(desktopRoot, "src/main.tsx"), "utf8");

    expect(entry).toContain('from "@markra/app"');
    expect(entry).not.toContain('from "./App"');
  });

  it("configures the desktop runtime before rendering the shared app", () => {
    const entry = readFileSync(resolve(desktopRoot, "src/main.tsx"), "utf8");

    expect(entry).toContain("configureAppRuntime");
    expect(entry).toContain('from "./runtime"');
  });

  it("keeps Tauri packages out of the shared app package", () => {
    const appPackage = readFileSync(resolve(workspaceRoot, "packages/app/package.json"), "utf8");

    expect(appPackage).not.toContain("@tauri-apps/");
  });

  it("uses localized QingYu desktop package names without changing the identifier", () => {
    const tauriConfig = JSON.parse(
      readFileSync(resolve(desktopRoot, "src-tauri/tauri.conf.json"), "utf8")
    ) as { productName: string; identifier: string };

    expect(tauriConfig.productName).toBe("QingYu");
    expect(tauriConfig.identifier).toBe("dev.markra.app");

    const expectedNames = {
      de: "QingYu",
      en: "QingYu",
      es: "QingYu",
      fr: "QingYu",
      it: "QingYu",
      ja: "QingYu",
      ko: "QingYu",
      "pt-BR": "QingYu",
      ru: "QingYu",
      "zh-Hans": "轻语",
      "zh-Hant": "輕語"
    } as const;

    for (const [locale, expectedName] of Object.entries(expectedNames)) {
      expect(readBundleNames(locale), locale).toEqual({
        displayName: expectedName,
        name: expectedName
      });
    }
  });
});
