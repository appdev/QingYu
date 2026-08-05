import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { markdownImageDragMime } from "@markra/shared";
import { afterEach, describe, expect, it, vi } from "vitest";
import { codeMirrorClipboardAssetsPlugin } from "./clipboard-assets.ts";
import type { SaveEditorResources } from "../clipboard-asset-types.ts";
import { liveMarkdown } from "./index.ts";
import "./dom.test-support.ts";

const views: EditorView[] = [];

function createDeferred<T>() {
  let resolve!: (value: T) => unknown;
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}

function createView(
  doc: string,
  options: Parameters<typeof codeMirrorClipboardAssetsPlugin>[0] = {},
  readOnly = false,
) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [
        liveMarkdown({ plugins: [codeMirrorClipboardAssetsPlugin(options)] }),
        EditorState.readOnly.of(readOnly),
      ],
      selection: EditorSelection.cursor(doc.length),
    }),
  });
  views.push(view);
  return view;
}

function fileList(files: readonly File[]) {
  return Object.assign([...files], {
    item: (index: number) => files[index] ?? null,
  });
}

function bytesFromBase64(value: string) {
  return Uint8Array.from(atob(value), (character) => character.charCodeAt(0));
}

async function sha256Hex(file: File) {
  const digest = await crypto.subtle.digest("SHA-256", await file.arrayBuffer());
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

function paste(
  view: EditorView,
  options: {
    editorData?: string;
    files?: readonly File[];
    html?: string;
    text?: string;
  },
) {
  const event = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", {
    value: {
      files: fileList(options.files ?? []),
      getData: (type: string) => {
        if (type === "text/html") return options.html ?? "";
        if (type === "text/plain") return options.text ?? "";
        if (type === "vscode-editor-data") return options.editorData ?? "";
        return "";
      },
    },
  });
  view.contentDOM.dispatchEvent(event);
  return event;
}

function drop(
  view: EditorView,
  options: { files?: readonly File[]; payload?: unknown; text?: string },
) {
  const event = new MouseEvent("drop", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "dataTransfer", {
    value: {
      files: fileList(options.files ?? []),
      getData: (type: string) => {
        if (type === markdownImageDragMime && options.payload) {
          return JSON.stringify(options.payload);
        }
        if (type === "text/plain") return options.text ?? "";
        return "";
      },
    },
  });
  view.contentDOM.dispatchEvent(event);
  return event;
}

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.replaceChildren();
});

describe("codeMirrorClipboardAssetsPlugin", () => {
  it("normalizes native seven-format files and saves one byte-exact image batch", async () => {
    const avif = new File([bytesFromBase64("AAAAIGZ0eXBhdmlmAAAAAGF2aWZtaWYxbWlhZk1BMUIAAAD5bWV0YQAAAAAAAAAvaGRscgAAAAAAAAAAcGljdAAAAAAAAAAAAAAAAFBpY3R1cmVIYW5kbGVyAAAAAA5waXRtAAAAAAABAAAAHmlsb2MAAAAARAAAAQABAAAAAQAAASEAAADxAAAAKGlpbmYAAAAAAAEAAAAaaW5mZQIAAAAAAQAAYXYwMUNvbG9yAAAAAGppcHJwAAAAS2lwY28AAAAUaXNwZQAAAAAAAAAQAAAADAAAABBwaXhpAAAAAAMICAgAAAAMYXYxQ4EADAAAAAATY29scm5jbHgAAgACAAIAAAAAF2lwbWEAAAAAAAAAAQABBAECgwQAAAD5bWRhdAoKAgAABQz+xK+QBDLiARAAloAQQIKB94DAXp2W8xbG+qGYQZDfijM9kuWB+kLCAK0jeG84US9KCgPrGaIlb6RX2S+/CTm9h9eO/0yZfAVy1st6Kph10tEPbSTiSfV8a5tcoiCXpmFwOXQbmQC6zsUbgLky/8U3zfOMCtoKw+dVyhmdhx2OrSfxIiKp6rp6aBkwN1nFpwS7i8XPXaq8hK0F05roGuiwTlitOUb8xmkMGs/WxLdHiBxYt24BeFZqTpUoODjjym8ViX/b1dXd9b2SjQaR6vhB+Ymz0A+xMrMuEc/qJK6p2O5/JiNxb7byCDA=")], "fixture.avif", { type: "" });
    const bmp = new File([bytesFromBase64("Qk02AwAAAAAAADYAAAAoAAAAEAAAAAwAAAABACAAAAAAAAADAAAAAAAAAAAAAAAAAAAAAAAAAAD+/wAA/v8AAD//AAA//wB///8Af///AC9v/y6Pz/9piMf/AAA2/wB///8Af///AH///wB///8AKkn/VJW0/wAA/v8AAP7/AAA//wAAP/8Af///AH///wAvb/8uj8//aYjH/wAANv8Af///AH///wB///8Af///ACpJ/1SVtP8AAP7/AAD+/wAAP/8AAD//AH///wB///8APz7/AD8+/z8AAP8/AAD/AH///wB///8Af///AH///z4/AP8+PwD/AAD+/wAA/v8AAD//AAA//wB///8Af///AD8+/wA/Pv8/AAD/PwAA/wB///8Af///AH///wB///8+PwD/Pj8A/wAA/v8AAP7/AAA//wAAP/8AMiP/VqSV/xiH5/8AJ4f/AABT/zSD5P+di6z/DQAb/w8AUP+Nfc3/Kora/wAfb/8AAP7/AAD+/wAAP/8AAD//ADIj/wAyI/8Yh+f/GIfn/zSD5P80g+T/DQAb/w0AG/8PAFD/DwBQ/yqK2v8qitr/AAD+/wAA/v8AAD//AAA//wE/AP8BPwD/AD8+/wA/Pv8/AAD/PwAA/z8AAP8/AAD/PgA+/z4APv8+PwD/Pj8A/wAA/v8AAP7/AAA//wAAP/8BPwD/AT8A/wA/Pv8APz7/PwAA/z8AAP8/AAD/PwAA/z4APv8+AD7/Pj8A/z4/AP8AAP7/AAD+/wAAP/8AAD//AT8A/wE/AP8APz7/AD8+/z8AAP8/AAD/PwAA/z8AAP8+AD7/PgA+/z4/AP8+PwD/AAD+/wAA/v8AAD//AAA//wE/AP8BPwD/AD8+/wA/Pv8/AAD/PwAA/z8AAP8/AAD/PgA+/z4APv8+PwD/Pj8A/wAA/v8AAP7/AAD+/wAA/v8B/wD/Af8A/wD+/v8A/v7//wAA//8AAP//AAD//wAA//4A///+AP////8A////AP8AAP7/AAD+/wAA/v8AAP7/Af8A/wH/AP8A/v7/AP7+//8AAP//AAD//wAA//8AAP/+AP///gD/////AP///wD/")], "fixture.bmp", { type: "image/x-ms-bmp" });
    const svg = new File([bytesFromBase64("PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAxNiAxMiIgd2lkdGg9IjE2IiBoZWlnaHQ9IjEyIiByb2xlPSJpbWciIGFyaWEtbGFiZWw9Ik1hcmtyYSBmaXh0dXJlIj48dGl0bGU+TWFya3JhIHNldmVuLWZvcm1hdCBmaXh0dXJlPC90aXRsZT48cmVjdCB4PSIwIiB5PSIwIiB3aWR0aD0iMTYiIGhlaWdodD0iMTIiIGZpbGw9IiMyODY0ZGMiLz48Y2lyY2xlIGN4PSI4IiBjeT0iNiIgcj0iMyIgZmlsbD0iI2Y1YzI0MiIvPjwvc3ZnPgo=")], "fixture.svg", { type: "application/svg+xml" });
    const files = [
      avif,
      bmp,
      new File([new Uint8Array([0x47, 0x49, 0x46])], "fixture.gif", { type: "image/gif" }),
      new File([new Uint8Array([0xff, 0xd8, 0xff])], "fixture.jpg", { type: "image/pjpeg" }),
      new File([new Uint8Array([0x89, 0x50, 0x4e, 0x47])], "fixture.png", { type: "image/png" }),
      svg,
      new File([new Uint8Array([0x52, 0x49, 0x46, 0x46])], "fixture.webp", { type: "image/webp" }),
    ];
    const saveResources: SaveEditorResources = vi.fn(async (request: Parameters<SaveEditorResources>[0]) => "files" in request
      ? request.files.map((file) => ({
          alt: file.name,
          kind: "image" as const,
          src: `assets/${file.name}`,
        }))
      : []);
    const view = createView("", { saveResources });

    expect(paste(view, { files }).defaultPrevented).toBe(true);
    await vi.waitFor(() => expect(saveResources).toHaveBeenCalledOnce());
    const request = vi.mocked(saveResources).mock.calls[0]?.[0];
    const savedFiles = request && "files" in request ? request.files : [];
    expect(savedFiles.map((file) => file.type)).toEqual([
      "image/avif", "image/bmp", "image/gif", "image/jpeg", "image/png", "image/svg+xml", "image/webp",
    ]);
    expect(await Promise.all(savedFiles.slice(0, 2).map(sha256Hex))).toEqual([
      "a51d8055a9e709e4e970e05ecddf834da5efeb47694f29aa25c5b79784b63e6a",
      "bbc548ae9c4d95a61029f39b6fa1a32fc5e63950ae166010ce7d797d17a64091",
    ]);
    expect(await sha256Hex(savedFiles[5] ?? new File([], "missing"))).toBe(
      "31c458a9110ddf17e4eba65c247279fc522cbcfcc502b136c6f87b69566972e5",
    );
    await vi.waitFor(() => expect(view.state.doc.toString()).toBe(files
      .map((file) => `![${file.name}](assets/${file.name})`)
      .join("")));
  });

  it("removes a failed mixed-image batch and retries it without partial Markdown", async () => {
    const files = [
      new File([new Uint8Array([1])], "first.png", { type: "image/png" }),
      new File([new Uint8Array([2])], "second.avif", { type: "image/avif" }),
    ];
    const saveResources = vi.fn()
      .mockRejectedValueOnce(new Error("invalid request details must stay private"))
      .mockResolvedValueOnce([
        { alt: "first", kind: "image", src: "assets/first.png" },
        { alt: "second", kind: "image", src: "assets/second.avif" },
      ]);
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    const view = createView("", { saveResources });

    paste(view, { files });
    await vi.waitFor(() => expect(view.dom.querySelector(".markra-image-upload-placeholder")).toBeNull());
    expect(view.state.doc.toString()).toBe("");

    paste(view, { files });
    await vi.waitFor(() => expect(view.state.doc.toString()).toBe(
      "![first](assets/first.png)![second](assets/second.avif)",
    ));
    expect(saveResources).toHaveBeenCalledTimes(2);
    error.mockRestore();
  });

  it.each(["paste", "drop"] as const)(
    "rejects unsupported or conflicting %s images before every save side effect",
    async (origin) => {
      const rejected = [
        new File([new Uint8Array([1])], "unknown.tiff", { type: "image/tiff" }),
        new File([new Uint8Array([2])], "sequence.avis", { type: "application/octet-stream" }),
        new File([new Uint8Array([3])], "conflict.svg", { type: "image/png" }),
      ];
      const png = new File([new Uint8Array([4])], "valid.png", { type: "image/png" });

      for (const candidate of rejected) {
        for (const files of [[candidate], [png, candidate]]) {
          const saveResources = vi.fn(async () => []);
          const saveAttachment = vi.fn(async () => null);
          const saveImage = vi.fn(async () => null);
          const view = createView("", { saveAttachment, saveImage, saveResources });
          const event = origin === "paste" ? paste(view, { files }) : drop(view, { files });

          expect(event.defaultPrevented, `${candidate.name} ${files.length}`).toBe(true);
          expect(saveResources).not.toHaveBeenCalled();
          expect(saveAttachment).not.toHaveBeenCalled();
          expect(saveImage).not.toHaveBeenCalled();
          expect(view.dom.querySelector(".markra-image-upload-placeholder")).toBeNull();
          expect(view.state.doc.toString()).toBe("");
        }
      }
    },
  );

  it("shows a placeholder and inserts a saved pasted image", async () => {
    const pending = createDeferred<{ alt: string; src: string } | null>();
    const saveImage = vi.fn(() => pending.promise);
    const image = new File([new Uint8Array([1, 2, 3])], "Screenshot.png", {
      type: "image/png",
    });
    const view = createView("", { saveImage });

    const event = paste(view, { files: [image] });

    expect(event.defaultPrevented).toBe(true);
    await vi.waitFor(() => expect(saveImage).toHaveBeenCalledWith(image));
    expect(view.dom.querySelector(".markra-image-upload-placeholder")).not.toBeNull();
    expect(view.state.doc.toString()).toBe("");

    pending.resolve({ alt: "Screenshot", src: "assets/pasted-image.png" });
    await vi.waitFor(() => {
      expect(view.state.doc.toString()).toBe("![Screenshot](assets/pasted-image.png)");
    });
    expect(view.dom.querySelector(".markra-image-upload-placeholder")).toBeNull();
  });

  it("maps a pending image insertion through typing without losing text", async () => {
    const pending = createDeferred<{ alt: string; src: string } | null>();
    const view = createView("", { saveImage: () => pending.promise });
    const image = new File([new Uint8Array([1])], "Delayed.png", { type: "image/png" });

    paste(view, { files: [image] });
    view.dispatch({ changes: { from: 0, insert: "Typed while waiting" } });
    pending.resolve({ alt: "Delayed", src: "assets/delayed.png" });

    await vi.waitFor(() => expect(view.state.doc.toString()).toContain("![Delayed](assets/delayed.png)"));
    expect(view.state.doc.toString()).toContain("Typed while waiting");
  });

  it("saves attachments and inserts Markdown links", async () => {
    const saveAttachment = vi.fn().mockResolvedValue({
      label: "Reference Doc.pdf",
      src: "assets/Reference%20Doc.pdf",
    });
    const attachment = new File([new Uint8Array([4])], "Reference Doc.pdf", {
      type: "application/pdf",
    });
    const view = createView("", { saveAttachment });

    expect(paste(view, { files: [attachment] }).defaultPrevented).toBe(true);

    await vi.waitFor(() => {
      expect(view.state.doc.toString()).toBe(
        "[Reference Doc.pdf](assets/Reference%20Doc.pdf)",
      );
    });
    expect(saveAttachment).toHaveBeenCalledWith(attachment);
  });

  it("prefers a structured HTML table over its bitmap clipboard preview", async () => {
    const saveImage = vi.fn().mockResolvedValue({
      alt: "Preview",
      src: "assets/preview.png",
    });
    const preview = new File([new Uint8Array([1])], "preview.png", { type: "image/png" });
    const view = createView("", { saveImage });

    const event = paste(view, {
      files: [preview],
      html: "<table><tr><th>Name</th><th>Role</th></tr><tr><td>Alpha</td><td>Editor</td></tr></table>",
      text: "Name\tRole\nAlpha\tEditor",
    });

    expect(event.defaultPrevented).toBe(true);
    expect(saveImage).not.toHaveBeenCalled();
    expect(view.state.doc.toString()).toContain("| Name | Role |");
    expect(view.state.doc.toString()).toContain("| Alpha | Editor |");
  });

  it("converts pasted web HTML and localizes remote images", async () => {
    const saveRemoteImage = vi.fn().mockResolvedValue({
      alt: "Kitten",
      src: "assets/kitten.png",
    });
    const view = createView("", { saveRemoteImage });

    const event = paste(view, {
      html: '<p>Intro</p><img src="https://images.example.test/kitten.png" alt="Kitten"><p>Outro</p>',
      text: "Intro\n\nOutro",
    });

    expect(event.defaultPrevented).toBe(true);
    await vi.waitFor(() => {
      expect(saveRemoteImage).toHaveBeenCalledWith({
        alt: "Kitten",
        src: "https://images.example.test/kitten.png",
        title: "",
      });
    });
    await vi.waitFor(() => expect(view.state.doc.toString()).toContain("![Kitten](assets/kitten.png)"));
    expect(view.state.doc.toString()).toContain("Intro");
    expect(view.state.doc.toString()).toContain("Outro");
  });

  it("prefers structured rich HTML over Markdown-looking fallback text", () => {
    const view = createView("");

    const event = paste(view, {
      html: [
        "<p>Mock summary</p>",
        "<ol><li>First <code>choice</code></li><li>Second choice</li></ol>",
        '<p>See <a href="https://example.test/mock-docs">mock docs</a>.</p>',
      ].join(""),
      text: [
        "Mock summary",
        "First choice",
        "Second choice",
        "See [mock docs](https://example.test/mock-docs).",
      ].join("\n"),
    });

    expect(event.defaultPrevented).toBe(true);
    expect(view.state.doc.toString()).toBe([
      "Mock summary",
      "",
      "1.  First `choice`",
      "2.  Second choice",
      "",
      "See [mock docs](https://example.test/mock-docs).",
    ].join("\n"));
  });

  it("keeps styled file badges as inline links", () => {
    const view = createView("");
    const expected = [
      "Mock changes: ",
      "[example-a.ts (line 108)](/mock-project/src/example-a.ts:108), ",
      "[example-b.ts (line 438)](C:/mock-project/src/example-b.ts:438), ",
      "[example-c.ts (line 7)](https://example.test/mock-file#L7).",
    ].join("");

    const event = paste(view, {
      html: [
        "<p>Mock changes: ",
        '<a href="/mock-project/src/example-a.ts:108">',
        '<div style="font-family: Menlo, monospace; white-space: pre-wrap">',
        "example-a.ts (line 108)",
        "</div>",
        "</a>, ",
        '<a href="C:/mock-project/src/example-b.ts:438" ',
        'style="font-family: Menlo, monospace; white-space: pre-wrap">',
        "<div>example-b.ts</div>",
        "<div>(line 438)</div>",
        "</a>, ",
        '<a href="https://example.test/mock-file#L7">',
        '<p style="font-family: Menlo, monospace">example-c.ts (line 7)</p>',
        "</a>.</p>",
      ].join(""),
      text: expected,
    });

    expect(event.defaultPrevented).toBe(true);
    expect(view.state.doc.toString()).toBe(expected);
  });

  it("does not merge ordinary linked card blocks", () => {
    const view = createView("");
    const href = "https://example.test/mock-card";

    paste(view, {
      html: [
        '<p>See <a href="https://example.test/mock-card">',
        "<div>Mock title</div>",
        "<div>Mock subtitle</div>",
        "</a>.</p>",
      ].join(""),
      text: "See Mock title Mock subtitle.",
    });

    const markdown = view.state.doc.toString();
    expect(markdown).toContain(`[Mock title](${href})`);
    expect(markdown).toContain(`[Mock subtitle](${href})`);
    expect(markdown).not.toContain("Mock titleMock subtitle");
  });

  it("does not flatten semantic or multiline linked code", () => {
    const semanticView = createView("");
    const multilineView = createView("");

    paste(semanticView, {
      html: [
        '<p>See <a href="https://example.test/mock-code">',
        '<pre style="font-family: Menlo, monospace"><code>const mock = 1;</code></pre>',
        "</a>.</p>",
      ].join(""),
      text: "See const mock = 1;.",
    });
    paste(multilineView, {
      html: [
        '<p>See <a href="https://example.test/mock-lines">',
        '<div style="font-family: Menlo, monospace; white-space: pre-wrap">',
        "Mock line one<br>Mock line two",
        "</div></a>.</p>",
      ].join(""),
      text: "See Mock line one\nMock line two.",
    });

    expect(semanticView.state.doc.toString()).toContain("```\nconst mock = 1;\n```");
    expect(multilineView.state.doc.toString()).toContain("```\nMock line one\nMock line two\n```");
  });

  it("preserves Markdown-looking lines inside a styled mixed-content code block", () => {
    const view = createView("");
    const code = [
      "# Mock score",
      "=",
      "+ reward × 100",
      "",
      "- resource cost",
    ].join("\n");

    const event = paste(view, {
      html: [
        "<h2>Mock formula</h2>",
        "<ul><li>First constraint</li><li>Second constraint</li></ul>",
        "<p>Use this synthetic model:</p>",
        '<div style="font-family: Menlo, monospace; white-space: pre-wrap">',
        "<div># Mock score</div>",
        "<div>=</div>",
        "<div>+ reward × 100</div>",
        "<div><br></div>",
        "<div>- resource cost</div>",
        "</div>",
      ].join(""),
      text: [
        "Mock formula",
        "First constraint",
        "Second constraint",
        "Use this synthetic model:",
        code,
      ].join("\n"),
    });

    expect(event.defaultPrevented).toBe(true);
    expect(view.state.doc.toString()).toBe([
      "## Mock formula",
      "",
      "-   First constraint",
      "-   Second constraint",
      "",
      "Use this synthetic model:",
      "",
      "```",
      code,
      "```",
    ].join("\n"));
  });

  it("preserves language metadata from a styled mixed-content code block", () => {
    const code = "print('mock value')";
    const view = createView("");

    paste(view, {
      html: [
        "<p>Mock introduction</p>",
        '<div style="font-family: Menlo, monospace; white-space: pre-wrap">',
        `<code class="language-python">${code}</code>`,
        "</div>",
        "<p>Mock conclusion</p>",
      ].join(""),
      text: ["Mock introduction", code, "Mock conclusion"].join("\n"),
    });

    expect(view.state.doc.toString()).toBe([
      "Mock introduction",
      "",
      "```python",
      code,
      "```",
      "",
      "Mock conclusion",
    ].join("\n"));
  });

  it("keeps Markdown source from a non-semantic editor clipboard", () => {
    const source = [
      "# Mock heading",
      "",
      "- First item",
      "- Second item",
    ].join("\n");
    const view = createView("");

    paste(view, {
      html: [
        '<div class="mock-editor-line"><span># Mock heading</span></div>',
        '<div class="mock-editor-line"><br></div>',
        '<div class="mock-editor-line"><span>- First item</span></div>',
        '<div class="mock-editor-line"><span>- Second item</span></div>',
      ].join(""),
      text: source,
    });

    expect(view.state.doc.toString()).toBe(source);
  });

  it("wraps code copied with syntax-highlighted HTML in a fenced block", () => {
    const code = [
      "const mock_value = items[0];",
      'if (mock_value === "synthetic") {',
      "  return /a+b*/.test(mock_value);",
      "}",
    ].join("\n");
    const view = createView("");

    const event = paste(view, {
      html: [
        '<div style="font-family: Menlo, Monaco, monospace">',
        "<div>const mock_value = items[0];</div>",
        '<div>if (mock_value === &quot;synthetic&quot;) {</div>',
        "<div>&nbsp;&nbsp;return /a+b*/.test(mock_value);</div>",
        "<div>}</div>",
        "</div>",
      ].join(""),
      text: code,
    });

    expect(event.defaultPrevented).toBe(true);
    expect(view.state.doc.toString()).toBe(`\`\`\`javascript\n${code}\n\`\`\``);
  });

  it("preserves a code block inside mixed rich HTML", () => {
    const code = "print('synthetic')";
    const view = createView("");

    const event = paste(view, {
      html: [
        "<p>Mock introduction</p>",
        `<pre><code class="language-python">${code}</code></pre>`,
        "<p>Mock conclusion</p>",
      ].join(""),
      text: `Mock introduction\n\n${code}\n\nMock conclusion`,
    });

    expect(event.defaultPrevented).toBe(true);
    expect(view.state.doc.toString()).toBe(
      `Mock introduction\n\n\`\`\`python\n${code}\n\`\`\`\n\nMock conclusion`,
    );
  });

  it("wraps high-confidence plain text code at a block boundary", () => {
    const code = [
      "const mockValue = items[0];",
      "if (mockValue) {",
      "  console.log(mockValue);",
      "}",
    ].join("\n");
    const view = createView("Intro");

    const event = paste(view, { text: code });

    expect(event.defaultPrevented).toBe(true);
    expect(view.state.doc.toString()).toBe(
      `Intro\n\n\`\`\`javascript\n${code}\n\`\`\``,
    );
  });

  it("uses a longer fence when pasted code contains Markdown backticks", () => {
    const code = 'const fence = "```";\nconsole.log(fence);';
    const view = createView("");

    paste(view, {
      editorData: JSON.stringify({ mode: "javascript", version: 1 }),
      text: code,
    });

    expect(view.state.doc.toString()).toBe(
      `\`\`\`\`javascript\n${code}\n\`\`\`\``,
    );
  });

  it("does not nest an automatically detected block inside fenced code", () => {
    const view = createView("```ts\nconst before = true;\n```");
    view.dispatch({ selection: EditorSelection.cursor(6) });
    const original = view.state.doc.toString();

    paste(view, {
      editorData: JSON.stringify({ mode: "typescript", version: 1 }),
      text: "const one = 1;\nconst two = 2;\n",
    });

    expect(view.state.doc.toString()).toBe(
      original.replace("const before", "const one = 1;\nconst two = 2;\nconst before"),
    );
    expect(view.state.doc.toString().match(/```/gu)).toHaveLength(2);
  });

  it("inserts existing file-tree image drags without saving again", () => {
    const saveImage = vi.fn();
    const view = createView("", {
      documentPath: () => "/vault/docs/note.md",
      saveImage,
    });

    const event = drop(view, {
      payload: {
        alt: "Diagram",
        path: "/vault/assets/diagram.png",
        relativePath: "assets/diagram.png",
      },
    });

    expect(event.defaultPrevented).toBe(true);
    expect(view.state.doc.toString()).toBe("![Diagram](../assets/diagram.png)");
    expect(saveImage).not.toHaveBeenCalled();
  });

  it("does not handle file mutations in a read-only editor", () => {
    const saveImage = vi.fn();
    const image = new File([new Uint8Array([1])], "Screenshot.png", { type: "image/png" });
    const view = createView("Read only", { saveImage }, true);

    const event = paste(view, { files: [image] });

    // CodeMirror itself suppresses browser mutations in read-only mode.
    expect(event.defaultPrevented).toBe(true);
    expect(saveImage).not.toHaveBeenCalled();
    expect(view.state.doc.toString()).toBe("Read only");
  });
});
