import { buildITrieFromWords } from "cspell-trie-lib";
import {
  createAsyncCspellTrieSpellchecker,
  createWordSetSpellchecker,
  defaultEnglishSpellchecker,
  tokenizeSpellcheckText,
} from "./spellcheck.ts";

describe("spellcheck utilities", () => {
  it("tokenizes ordinary words while skipping URLs, numbers, and short abbreviations", () => {
    expect(
      tokenizeSpellcheckText(
        "A valid markdown-ish word, https://example.test, PDF, 2026, and teh.",
      ),
    ).toEqual([
      { from: 2, text: "valid", to: 7 },
      { from: 8, text: "markdown-ish", to: 20 },
      { from: 21, text: "word", to: 25 },
      { from: 60, text: "and", to: 63 },
      { from: 64, text: "teh", to: 67 },
    ]);
  });

  it("tokenizes Latin accents and Cyrillic words without treating CJK text as spelling words", () => {
    expect(tokenizeSpellcheckText("café déjà-vu привет and 中文")).toEqual([
      { from: 0, text: "café", to: 4 },
      { from: 5, text: "déjà-vu", to: 12 },
      { from: 13, text: "привет", to: 19 },
      { from: 20, text: "and", to: 23 },
    ]);
  });

  it("skips identifier-style camelCase, PascalCase, and snake_case text", () => {
    expect(
      tokenizeSpellcheckText(
        "myVariableName ExampleComponent HTTPResponse api_response snake_case and wrnog",
      ).map(({ text }) => text),
    ).toEqual(["and", "wrnog"]);
  });

  it("checks word-set dictionaries and suggests nearby words", () => {
    const spellchecker = createWordSetSpellchecker([
      "accommodate",
      "document",
      "markdown",
    ]);

    expect(spellchecker.check("document")).toBe(true);
    expect(spellchecker.check("acommodate")).toBe(false);
    expect(spellchecker.suggest?.("acommodate")).toContain("accommodate");
  });

  it("uses a safe known-misspelling fallback when no dictionary is configured", () => {
    expect(defaultEnglishSpellchecker.check("customterm")).toBe(true);
    expect(defaultEnglishSpellchecker.check("teh")).toBe(false);
    expect(defaultEnglishSpellchecker.suggest?.("teh")).toEqual(["the"]);
  });

  it("treats words as valid while an async dictionary is loading", async () => {
    const spellchecker = createAsyncCspellTrieSpellchecker(async () =>
      buildITrieFromWords(["document", "environment"]),
    );

    expect(spellchecker.isReady?.()).toBe(false);
    expect(spellchecker.check("acommodate")).toBe(true);

    await spellchecker.load?.();

    expect(spellchecker.isReady?.()).toBe(true);
    expect(spellchecker.check("environment")).toBe(true);
    expect(spellchecker.check("acommodate")).toBe(false);
  });
});
