import { describe, expect, it } from "vitest";
import { detectCodePaste } from "./code-paste.ts";

describe("detectCodePaste", () => {
  it("uses VS Code clipboard metadata to recognize code and its language", () => {
    const text = [
      "interface MockUser {",
      "  id: string;",
      "}",
    ].join("\n");

    expect(detectCodePaste({
      editorData: JSON.stringify({ mode: "typescript", version: 1 }),
      text,
    })).toEqual({ code: text, language: "ts" });
  });

  it("recognizes code copied as preformatted HTML", () => {
    const text = [
      "def mock_greeting(name):",
      "    return f\"Hello {name}\"",
    ].join("\n");

    expect(detectCodePaste({
      html: '<pre><code class="language-python">def mock_greeting(name):\n    return f"Hello {name}"</code></pre>',
      text,
    })).toEqual({ code: text, language: "python" });
  });

  it("recognizes high-confidence multiline plain text code", () => {
    const text = [
      "const mockValue = items[0];",
      "if (mockValue) {",
      "  console.log(mockValue);",
      "}",
    ].join("\n");

    expect(detectCodePaste({ text })).toEqual({
      code: text,
      language: "javascript",
    });
  });

  it("recognizes structured JSON without relying on clipboard HTML", () => {
    const text = [
      "{",
      '  "name": "mock-project",',
      '  "enabled": true',
      "}",
    ].join("\n");

    expect(detectCodePaste({ text })).toEqual({ code: text, language: "json" });
  });

  it.each([
    {
      language: "python",
      text: "import os\nprint(os.getcwd())",
    },
    {
      language: "css",
      text: "body {\n  color: red;\n}",
    },
    {
      language: "bash",
      text: "npm install\npnpm dev",
    },
  ])("recognizes common $language snippets", ({ language, text }) => {
    expect(detectCodePaste({ text })).toEqual({ code: text, language });
  });

  it("leaves prose, Markdown, and ambiguous single lines unchanged", () => {
    expect(detectCodePaste({
      text: "First paragraph.\n\nSecond paragraph with ordinary prose.",
    })).toBeNull();
    expect(detectCodePaste({
      text: "- First item\n- Second item",
    })).toBeNull();
    expect(detectCodePaste({
      text: "```ts\nconst value = 1;\n```",
    })).toBeNull();
    expect(detectCodePaste({ text: "const value = 1;" })).toBeNull();
  });

  it("does not trust malformed editor clipboard metadata", () => {
    expect(detectCodePaste({
      editorData: "not-json",
      text: "First paragraph.\nSecond paragraph.",
    })).toBeNull();
  });

  it("does not treat an article as code because it contains one monospace span", () => {
    expect(detectCodePaste({
      html: '<p>First paragraph with <code style="font-family: monospace">mockValue</code>.</p><p>Second paragraph.</p>',
      text: "First paragraph with mockValue.\nSecond paragraph.",
    })).toBeNull();
  });

  it("keeps a Markdown document copied from a preformatted viewer as Markdown", () => {
    const text = [
      "# 文档编辑会话迁移 Implementation Plan",
      "",
      "> **For agentic workers:** Follow the implementation plan.",
      "",
      "**Goal:** Preserve the Markdown document structure.",
      "",
      "```ts",
      "export interface DocumentSessionState {",
      "  dirty: boolean;",
      "}",
      "```",
      "",
      "## Global Constraints",
      "",
      "- Keep headings, quotes, lists, and fenced code intact.",
    ].join("\n");

    expect(detectCodePaste({
      html: '<pre style="font-family: Menlo; white-space: pre-wrap">Markdown source</pre>',
      text,
    })).toBeNull();
  });

  it("keeps explicit code metadata authoritative for Markdown-like code", () => {
    const text = [
      "const mockLabel = `inline code`;",
      "console.log(mockLabel);",
    ].join("\n");

    expect(detectCodePaste({
      editorData: JSON.stringify({ mode: "javascript", version: 1 }),
      text,
    })).toEqual({ code: text, language: "javascript" });
  });
});
