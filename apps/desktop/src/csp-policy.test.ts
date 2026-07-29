import { readFileSync } from "node:fs";
import { resolve } from "node:path";

type TauriSecurityConfig = {
  app: {
    security: {
      csp: string | null;
      devCsp?: string | null;
      dangerousDisableAssetCspModification?: boolean | string[];
    };
  };
};

function readSecurityConfig() {
  const config = JSON.parse(
    readFileSync(resolve(process.cwd(), "src-tauri/tauri.conf.json"), "utf8")
  ) as TauriSecurityConfig;

  return config.app.security;
}

function parseCsp(csp: string) {
  return new Map(
    csp
      .split(";")
      .map((directive) => directive.trim())
      .filter(Boolean)
      .map((directive) => {
        const [name, ...sources] = directive.split(/\s+/);
        return [name, sources] as const;
      })
  );
}

describe("desktop production CSP", () => {
  it("denies unlisted content and locks document-controlled capabilities", () => {
    const csp = readSecurityConfig().csp;
    expect(csp).not.toBeNull();

    const directives = parseCsp(csp as string);
    expect(directives.get("default-src")).toEqual(["'self'"]);
    expect(directives.get("script-src")).toEqual(["'self'"]);
    expect(directives.get("object-src")).toEqual(["'none'"]);
    expect(directives.get("base-uri")).toEqual(["'none'"]);
    expect(directives.get("form-action")).toEqual(["'none'"]);
    expect(directives.get("frame-src")).toEqual(["'none'"]);
    expect(directives.get("frame-ancestors")).toEqual(["'none'"]);
  });

  it("preserves local themes, remote images, blob workers, and Tauri IPC", () => {
    const security = readSecurityConfig();
    const csp = security.csp;
    const directives = parseCsp(csp as string);

    expect(directives.get("style-src")).toEqual([
      "'self'",
      "'unsafe-inline'",
      "asset:",
      "http://asset.localhost"
    ]);
    expect(directives.get("img-src")).toEqual([
      "'self'",
      "asset:",
      "http://asset.localhost",
      "data:",
      "blob:",
      "https:",
      "http:"
    ]);
    expect(directives.get("font-src")).toEqual([
      "'self'",
      "asset:",
      "http://asset.localhost",
      "data:"
    ]);
    expect(directives.get("worker-src")).toEqual(["'self'", "blob:"]);
    expect(directives.get("connect-src")).toEqual([
      "'self'",
      "ipc:",
      "http://ipc.localhost"
    ]);
    expect(security.dangerousDisableAssetCspModification).toEqual(["style-src"]);
  });

  it("does not expose the dormant Kernel host over browser loopback", () => {
    const csp = readSecurityConfig().csp;
    const connectSources = parseCsp(csp as string).get("connect-src") ?? [];

    expect(connectSources).not.toContain("http:");
    expect(connectSources).not.toContain("https:");
    expect(connectSources.every((source) => !source.includes("127.0.0.1"))).toBe(true);
    expect(connectSources.every((source) => !source.includes("localhost:*"))).toBe(true);
  });
});
