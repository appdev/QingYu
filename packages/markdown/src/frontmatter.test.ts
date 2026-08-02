import { describe, expect, it } from "vitest";
import {
  readMarkdownFrontmatter,
  upsertMarkdownFrontmatterTitle,
} from "./frontmatter.ts";

describe("readMarkdownFrontmatter", () => {
  it.each([
    "",
    "# Heading\n\nBody",
    "Intro\n\n---\ntitle: Later\n---",
  ])("reports no metadata for ordinary Markdown", (source) => {
    expect(readMarkdownFrontmatter(source)).toEqual({ status: "none" });
  });

  it("reads a leading YAML block and exposes exact half-open ranges", () => {
    const source = "\uFEFF---\r\ntitle: YAML\r\ntags:\r\n  - note\r\n---\r\n\r\n# Body\r\n";
    const contentFrom = source.indexOf("title:");
    const contentTo = source.indexOf("---", contentFrom);
    const to = contentTo + 3;

    expect(readMarkdownFrontmatter(source)).toEqual({
      status: "valid",
      title: "YAML",
      range: {
        from: 1,
        to,
        contentFrom,
        contentTo,
        kind: "yaml",
        delimiter: "---",
        source: source.slice(1, to),
      },
    });
  });

  it("reads a leading TOML block with LF line endings", () => {
    const source = "+++\ntitle = \"TOML\"\nprivate = true\n+++\n\n# Body\n";
    const contentFrom = source.indexOf("title");
    const contentTo = source.lastIndexOf("+++");
    const to = contentTo + 3;

    expect(readMarkdownFrontmatter(source)).toEqual({
      status: "valid",
      title: "TOML",
      range: {
        from: 0,
        to,
        contentFrom,
        contentTo,
        kind: "toml",
        delimiter: "+++",
        source: source.slice(0, to),
      },
    });
  });

  it("reads only the complete leading JSON object", () => {
    const source = [
      "{",
      "  \"title\": \"JSON\",",
      "  \"nested\": { \"brace\": \"}\" }",
      "}",
      "",
      "# Body",
    ].join("\n");
    const to = source.indexOf("}\n\n") + 1;

    expect(readMarkdownFrontmatter(source)).toEqual({
      status: "valid",
      title: "JSON",
      range: {
        from: 0,
        to,
        contentFrom: 1,
        contentTo: to - 1,
        kind: "json",
        source: source.slice(0, to),
      },
    });
  });

  it.each([
    "---\ntitle: Missing close\n# Body",
    "---\ntitle: [unterminated\n---\n",
    "+++\ntitle = [\n+++\n",
    "{\"title\": }\n\n# Body",
    "{\"title\": \"JSON\"} trailing text\n",
    "---\nplain scalar\n---\n",
  ])("reports recognized but unusable metadata as malformed", (source) => {
    expect(readMarkdownFrontmatter(source)).toEqual({ status: "malformed" });
  });

  it.each([
    { source: "---\ntitle: 42\n---\n", title: null },
    { source: "+++\ntitle = [\"one\", \"two\"]\n+++\n", title: null },
    { source: "{\"title\":false}\n", title: null },
    { source: "---\nauthor: Ying\n---\n", title: null },
  ])("accepts object metadata whose title is absent or non-string", ({ source, title }) => {
    const result = readMarkdownFrontmatter(source);

    expect(result.status).toBe("valid");
    if (result.status === "valid") expect(result.title).toBe(title);
  });

  it("does not confuse a literal title with the malformed status", () => {
    const source = "---\ntitle: malformed\n---\n";

    expect(readMarkdownFrontmatter(source)).toMatchObject({
      status: "valid",
      title: "malformed",
    });
  });
});

describe("upsertMarkdownFrontmatterTitle", () => {
  it("creates YAML metadata and a separating blank line for a new file", () => {
    expect(upsertMarkdownFrontmatterTitle("", "Untitled")).toEqual({
      ok: true,
      changed: true,
      kind: "yaml",
      source: "---\ntitle: Untitled\n---\n\n",
    });
  });

  it("quotes YAML-significant characters when creating metadata", () => {
    expect(upsertMarkdownFrontmatterTitle("# Body\n", "A: #1")).toEqual({
      ok: true,
      changed: true,
      kind: "yaml",
      source: "---\ntitle: \"A: #1\"\n---\n\n# Body\n",
    });
  });

  it("inserts YAML after a BOM and preserves the body's CRLF convention", () => {
    const source = "\uFEFF# Heading\r\n\r\nBody\r\n";

    expect(upsertMarkdownFrontmatterTitle(source, "Untitled")).toEqual({
      ok: true,
      changed: true,
      kind: "yaml",
      source: "\uFEFF---\r\ntitle: Untitled\r\n---\r\n\r\n# Heading\r\n\r\nBody\r\n",
    });
  });

  it("replaces a YAML title without disturbing comments, keys, indentation, BOM, or blank lines", () => {
    const source = "\uFEFF---\r\n# identity\r\nauthor: Ying\r\ntitle: Old\r\noptions:\r\n  enabled: true\r\n---\r\n\r\n# Body\r\n";
    const expected = source.replace("title: Old", "title: New title");

    expect(upsertMarkdownFrontmatterTitle(source, "New title")).toEqual({
      ok: true,
      changed: true,
      kind: "yaml",
      source: expected,
    });
  });

  it("adds a YAML title after existing top-level keys", () => {
    const source = "---\n# identity\nauthor: Ying\noptions:\n  enabled: true\n---\n\n# Body\n";
    const expected = "---\n# identity\nauthor: Ying\noptions:\n  enabled: true\ntitle: Untitled\n---\n\n# Body\n";

    expect(upsertMarkdownFrontmatterTitle(source, "Untitled")).toEqual({
      ok: true,
      changed: true,
      kind: "yaml",
      source: expected,
    });
  });

  it("replaces a TOML title without disturbing comments, key order, delimiters, or CRLF", () => {
    const source = "+++\r\n# identity\r\nauthor = \"Ying\"\r\ntitle = \"Old\" # visible\r\n[options]\r\n  enabled = true\r\n+++\r\n\r\n# Body\r\n";
    const expected = source.replace('title = "Old"', 'title = "New title"');

    expect(upsertMarkdownFrontmatterTitle(source, "New title")).toEqual({
      ok: true,
      changed: true,
      kind: "toml",
      source: expected,
    });
  });

  it("adds a TOML title before the first table without reordering existing keys", () => {
    const source = "+++\n# identity\nauthor = \"Ying\"\n[options]\n  enabled = true\n+++\n\n# Body\n";
    const expected = "+++\n# identity\nauthor = \"Ying\"\ntitle = \"Untitled\"\n[options]\n  enabled = true\n+++\n\n# Body\n";

    expect(upsertMarkdownFrontmatterTitle(source, "Untitled")).toEqual({
      ok: true,
      changed: true,
      kind: "toml",
      source: expected,
    });
  });

  it("replaces a JSON title while preserving indentation, key order, blank lines, and CRLF", () => {
    const source = "{\r\n    \"author\": \"Ying\",\r\n    \"title\": \"Old\",\r\n    \"options\": {\r\n        \"enabled\": true\r\n    }\r\n}\r\n\r\n# Body\r\n";
    const expected = source.replace('"title": "Old"', '"title": "New title"');

    expect(upsertMarkdownFrontmatterTitle(source, "New title")).toEqual({
      ok: true,
      changed: true,
      kind: "json",
      source: expected,
    });
  });

  it("adds a JSON title without reserializing unrelated content", () => {
    const source = "{\n  \"author\": \"Ying\",\n  \"options\": { \"enabled\": true }\n}\n\n# Body\n";
    const expected = "{\n  \"author\": \"Ying\",\n  \"options\": { \"enabled\": true },\n  \"title\": \"Untitled\"\n}\n\n# Body\n";

    expect(upsertMarkdownFrontmatterTitle(source, "Untitled")).toEqual({
      ok: true,
      changed: true,
      kind: "json",
      source: expected,
    });
  });

  it("adds a title to an empty multi-line JSON object using its indentation", () => {
    const source = "{\r\n}\r\n\r\n# Body\r\n";

    expect(upsertMarkdownFrontmatterTitle(source, "Untitled")).toEqual({
      ok: true,
      changed: true,
      kind: "json",
      source: "{\r\n  \"title\": \"Untitled\"\r\n}\r\n\r\n# Body\r\n",
    });
  });

  it.each([
    { source: "---\ntitle: Same\n---\n", kind: "yaml" as const },
    { source: "+++\ntitle = \"Same\"\n+++\n", kind: "toml" as const },
    { source: "{\"title\":\"Same\"}\n", kind: "json" as const },
  ])("does not rewrite $kind metadata when the title is already equal", ({ source, kind }) => {
    expect(upsertMarkdownFrontmatterTitle(source, "Same")).toEqual({
      ok: true,
      changed: false,
      kind,
      source,
    });
  });

  it.each([
    "---\ntitle: Missing close\n# Body",
    "---\ntitle: [unterminated\n---\n",
    "+++\ntitle = [\n+++\n",
    "{\"title\": }\n\n# Body",
  ])("refuses malformed metadata instead of returning rewritten bytes", (source) => {
    const original = source;

    expect(upsertMarkdownFrontmatterTitle(source, "Replacement")).toEqual({
      ok: false,
      reason: "malformed",
    });
    expect(source).toBe(original);
  });

  it.each([
    {
      source: "---\ntitle: 42\nauthor: Ying\n---\n",
      expected: "---\ntitle: Replacement\nauthor: Ying\n---\n",
      kind: "yaml" as const,
    },
    {
      source: "+++\ntitle = 42\nauthor = \"Ying\"\n+++\n",
      expected: "+++\ntitle = \"Replacement\"\nauthor = \"Ying\"\n+++\n",
      kind: "toml" as const,
    },
    {
      source: "{\n  \"title\": 42,\n  \"author\": \"Ying\"\n}\n",
      expected: "{\n  \"title\": \"Replacement\",\n  \"author\": \"Ying\"\n}\n",
      kind: "json" as const,
    },
  ])("overwrites a non-string $kind title without changing other metadata", ({ source, expected, kind }) => {
    expect(upsertMarkdownFrontmatterTitle(source, "Replacement")).toEqual({
      ok: true,
      changed: true,
      kind,
      source: expected,
    });
  });
});
