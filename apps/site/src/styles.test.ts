import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const siteStyles = readFileSync(resolve(process.cwd(), "src/styles.css"), "utf8");
const siteTokens = readFileSync(resolve(process.cwd(), "src/tokens.css"), "utf8");

describe("Hallmark site contract", () => {
  it("declares the Workbench design system before any CSS rules", () => {
    expect(siteStyles.startsWith(
      "/* Hallmark · genre: editorial · macrostructure: Workbench"
    )).toBe(true);
    expect(siteStyles).toContain('@import "./tokens.css";');
  });

  it("clips horizontal overflow at both document roots", () => {
    expect(siteStyles).toMatch(/html\s*\{[^}]*overflow-x:\s*clip/isu);
    expect(siteStyles).toMatch(/body\s*\{[^}]*overflow-x:\s*clip/isu);
    expect(siteStyles).not.toMatch(/overflow-x:\s*hidden/iu);
  });

  it("does not restore the audited visual or motion tells", () => {
    expect(siteStyles).not.toMatch(/radial-gradient/iu);
    expect(siteStyles).not.toMatch(/\btransition-all\b/iu);
    const transitionDeclarations = siteStyles.match(/\btransition:[^;]+/giu) ?? [];
    for (const declaration of transitionDeclarations) {
      expect(declaration).not.toMatch(/(?:^|[\s,])ease(?:-in|-out|-in-out)?(?:[\s,]|$)/iu);
    }
    expect(siteStyles).not.toMatch(/min-width:\s*20rem/iu);
  });

  it("protects display headings and touch affordances", () => {
    expect(siteStyles).toMatch(/h1,\s*h2,\s*h3\s*\{[^}]*min-width:\s*0[^}]*overflow-wrap:\s*anywhere/isu);
    expect(siteStyles).toMatch(/@media\s*\(pointer:\s*coarse\)[\s\S]*min-height:\s*44px/iu);
  });

  it("keeps every page token reference resolved", () => {
    const declaredTokens = new Set(
      [...siteTokens.matchAll(/(--[a-z0-9-]+)\s*:/giu)].map((match) => match[1])
    );
    const referencedTokens = new Set(
      [...siteStyles.matchAll(/var\((--[a-z0-9-]+)/giu)].map((match) => match[1])
    );

    expect([...referencedTokens].filter((token) => !declaredTokens.has(token))).toEqual([]);
  });

  it("removes spatial press motion when reduced motion is requested", () => {
    expect(siteStyles).toMatch(
      /@media\s*\(prefers-reduced-motion:\s*reduce\)[\s\S]*\.button-link:active[\s\S]*transform:\s*none/iu
    );
  });

  it("provides hover feedback for every minimal-header link family", () => {
    for (const selector of [
      ".site-brand:hover",
      ".site-header__web:hover",
      ".site-compact-navigation a:hover"
    ]) {
      expect(siteStyles).toContain(selector);
    }
  });
});
