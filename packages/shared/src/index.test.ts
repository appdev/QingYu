import {
  clampNumber,
  fileNameFromPath,
  firstMarkdownPath,
  folderNameFromDocumentPath,
  hasTauriRuntime,
  isMarkdownPath,
  isRecord,
  markdownDocumentTitleFromFileName,
  normalizeNullableString,
  normalizeMarkdownDocumentTitle,
  numberedMarkdownDocumentName,
  normalizedExternalAutolinkUrl,
  parentPathFromPath,
  pathNameFromPath,
  runtimeDiagnosticEvent,
  sanitizeDiagnosticDetails,
  sanitizeDiagnosticText,
  stableTextKey,
  untitledMarkdownDocumentName
} from ".";

describe("utilities", () => {
  beforeEach(() => {
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it("normalizes blank or non-string values to null", () => {
    expect(normalizeNullableString("")).toBeNull();
    expect(normalizeNullableString("   ")).toBeNull();
    expect(normalizeNullableString(null)).toBeNull();
    expect(normalizeNullableString(42)).toBeNull();
  });

  it("keeps non-empty strings unchanged", () => {
    expect(normalizeNullableString("/mock-files/vault")).toBe("/mock-files/vault");
  });

  it("continues an existing Markdown document number instead of nesting suffixes", () => {
    expect(numberedMarkdownDocumentName("Untitled.md", 0)).toBe("Untitled.md");
    expect(numberedMarkdownDocumentName("Untitled.md", 3)).toBe("Untitled 3.md");
    expect(numberedMarkdownDocumentName("Untitled 1.md", 1)).toBe("Untitled 2.md");
    expect(numberedMarkdownDocumentName("Draft 7.MaRkDoWn", 2)).toBe("Draft 9.MaRkDoWn");
    expect(numberedMarkdownDocumentName("未命名.md", 7)).toBe("未命名 7.md");
  });

  it.each([
    ["Meeting.md", "Meeting"],
    ["Meeting.markdown", "Meeting"],
    ["Meeting.MARKDOWN", "Meeting"],
    ["Ideas", "Ideas"]
  ])("extracts the title stem from %s", (fileName, expectedTitle) => {
    expect(markdownDocumentTitleFromFileName(fileName)).toBe(expectedTitle);
  });

  it.each([
    ["  launch ✨ plan  ", { ok: true, title: "launch ✨ plan", fileName: "launch ✨ plan.md" }],
    ["First\r\n\tsecond", { ok: true, title: "First second", fileName: "First second.md" }],
    [
      'plan/part\\notes:*?"<>|',
      { ok: true, title: "plan／part＼notes：＊？＂＜＞｜", fileName: "plan／part＼notes：＊？＂＜＞｜.md" }
    ],
    ["Draft...  ", { ok: true, title: "Draft", fileName: "Draft.md" }],
    ["CON", { ok: true, title: "_CON", fileName: "_CON.md" }]
  ])("normalizes a document title safely", (input, expected) => {
    expect(normalizeMarkdownDocumentTitle(input)).toEqual(expected);
  });

  it.each([
    ["\r\n\t .", { ok: false, reason: "empty" }],
    ["a".repeat(252), { ok: true, title: "a".repeat(252), fileName: `${"a".repeat(252)}.md` }],
    ["a".repeat(253), { ok: false, reason: "too-long" }]
  ])("reports document title validation errors", (input, expected) => {
    expect(normalizeMarkdownDocumentTitle(input)).toEqual(expected);
  });

  it.each([
    ["en", "Untitled.md"],
    ["zh-CN", "未命名.md"],
    ["zh-TW", "未命名.md"],
    ["ja", "無題.md"],
    ["ko", "제목 없음.md"],
    ["fr", "Sans titre.md"],
    ["de", "Unbenannt.md"],
    ["es", "Sin título.md"],
    ["pt-BR", "Sem título.md"],
    ["it", "Senza titolo.md"],
    ["ru", "Без названия.md"]
  ])("returns the localized default document name for %s", (language, expectedName) => {
    expect(untitledMarkdownDocumentName(language as Parameters<typeof untitledMarkdownDocumentName>[0])).toBe(expectedName);
  });

  it("recognizes non-null object records", () => {
    expect(isRecord({ ok: true })).toBe(true);
    expect(isRecord([])).toBe(true);
    expect(isRecord(null)).toBe(false);
    expect(isRecord("text")).toBe(false);
  });

  it("detects whether the app is running inside Tauri", () => {
    expect(hasTauriRuntime()).toBe(false);

    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};

    expect(hasTauriRuntime()).toBe(true);
  });

  it("clamps numeric values and rejects non-numeric input", () => {
    expect(clampNumber(12, 0, 10)).toBe(10);
    expect(clampNumber(-2, 0, 10)).toBe(0);
    expect(clampNumber(4, 0, 10)).toBe(4);
    expect(clampNumber(undefined, 0, 10)).toBeNull();
    expect(clampNumber(Number.NaN, 0, 10)).toBeNull();
  });

  it("reads names from POSIX and Windows-style paths", () => {
    expect(fileNameFromPath("/vault/docs/readme.md")).toBe("readme.md");
    expect(fileNameFromPath("C:\\vault\\docs\\readme.md")).toBe("readme.md");
    expect(fileNameFromPath("")).toBe("Untitled.md");
    expect(pathNameFromPath("/vault/docs")).toBe("docs");
    expect(pathNameFromPath(null)).toBe("No folder");
    expect(folderNameFromDocumentPath("/vault/docs/readme.md")).toBe("docs");
    expect(folderNameFromDocumentPath(null)).toBe("No folder");
    expect(parentPathFromPath("/vault/docs/readme.md")).toBe("/vault/docs");
    expect(parentPathFromPath("/readme.md")).toBe("/");
    expect(parentPathFromPath("C:\\vault\\docs\\readme.md")).toBe("C:\\vault\\docs");
    expect(parentPathFromPath("C:\\readme.md")).toBe("C:\\");
    expect(parentPathFromPath("readme.md")).toBeNull();
  });

  it("builds stable short text keys", () => {
    expect(stableTextKey("/vault/readme.md")).toBe(stableTextKey("/vault/readme.md"));
    expect(stableTextKey("/vault/readme.md")).not.toBe(stableTextKey("/vault/notes.md"));
  });

  it("detects supported markdown file paths", () => {
    expect(isMarkdownPath("/vault/readme.md")).toBe(true);
    expect(isMarkdownPath("/vault/readme.markdown")).toBe(true);
    expect(isMarkdownPath("/vault/notes.txt")).toBe(true);
    expect(isMarkdownPath("/vault/image.png")).toBe(false);
    expect(firstMarkdownPath(["/vault/image.png", "/vault/readme.md"])).toBe("/vault/readme.md");
    expect(firstMarkdownPath(["/vault/image.png"])).toBeNull();
  });

  it("normalizes explicit external URLs for autolinking", () => {
    expect(normalizedExternalAutolinkUrl(" https://example.test/articles/about ")).toBe(
      "https://example.test/articles/about"
    );
    expect(normalizedExternalAutolinkUrl("mailto:hello@example.test")).toBe("mailto:hello@example.test");
  });

  it("rejects text that should not be autolinked", () => {
    expect(normalizedExternalAutolinkUrl("example.test/articles/about")).toBeNull();
    expect(normalizedExternalAutolinkUrl("https://example.test first")).toBeNull();
    expect(normalizedExternalAutolinkUrl("javascript:alert(1)")).toBeNull();
    expect(normalizedExternalAutolinkUrl("file:///mock-files/secret.md")).toBeNull();
  });

  it("redacts diagnostic details without losing useful context", () => {
    expect(runtimeDiagnosticEvent).toBe("markra:runtime-diagnostic");
    expect(sanitizeDiagnosticText("failed at https://s3.example.test/private /Users/example/private-note.md")).toBe(
      "failed at [redacted] [redacted]"
    );
    expect(sanitizeDiagnosticDetails({
      bucket: "private-bucket",
      command: "sync_project_folder",
      endpointUrl: "https://s3.example.test/private",
      error: "S3 sync failed: PUT /Users/example/notes/private.md: HTTP 403",
      password: "synthetic-secret",
      region: "private-region",
      remoteRoot: "private-root",
      payload: {
        relativePath: "notes/private.md",
        rootPath: "/Users/example/notes",
        secretAccessKey: "synthetic-secret"
      }
    })).toEqual({
      bucket: "[redacted]",
      command: "sync_project_folder",
      endpointUrl: "[redacted]",
      error: "S3 sync failed: PUT [redacted] HTTP 403",
      password: "[redacted]",
      region: "[redacted]",
      remoteRoot: "[redacted]",
      payload: "{\"relativePath\":\"[redacted]\",\"rootPath\":\"[redacted]\",\"secretAccessKey\":\"[redacted]\"}"
    });
  });
});
