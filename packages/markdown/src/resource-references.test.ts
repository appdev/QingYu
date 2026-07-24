import { describe, expect, it } from "vitest";
import { parseMarkdownResourceReferences } from "./resource-references";

describe("parseMarkdownResourceReferences", () => {
  it.each([
    ["![Cover](assets/cover.png)", "assets/cover.png", "image"],
    ["[Report](assets/report.pdf)", "assets/report.pdf", "attachment"],
    [
      "![封面][cover]\n\n[cover]: ./assets/%E5%B0%81%E9%9D%A2.png?raw=1#preview",
      "./assets/%E5%B0%81%E9%9D%A2.png?raw=1#preview",
      "image"
    ],
    ["<img alt='Cover' src=\"assets/cover.webp\">", "assets/cover.webp", "html-image"],
    ["<a href='assets/archive.zip'>Archive</a>", "assets/archive.zip", "html-attachment"]
  ] as const)("extracts a resource reference from %s", (markdown, href, kind) => {
    const references = parseMarkdownResourceReferences(markdown);

    expect(references).toEqual([
      expect.objectContaining({ href, kind })
    ]);
    expect(references[0]?.lineNumber).toBeGreaterThanOrEqual(1);
    expect(references[0]?.columnNumber).toBeGreaterThanOrEqual(1);
    expect(markdown.slice(references[0]?.from, references[0]?.to)).toContain(
      kind.startsWith("html-") ? href : kind === "image" && markdown.includes("[cover]") ? "[cover]" : href
    );
  });

  it("extracts case-insensitive reference definitions for links", () => {
    expect(parseMarkdownResourceReferences("[Download][FILE]\n\n[file]: assets/archive.zip"))
      .toEqual([
        expect.objectContaining({
          href: "assets/archive.zip",
          kind: "attachment",
          text: "Download"
        })
      ]);
  });

  it("extracts single-quoted, double-quoted, and unquoted raw HTML attributes", () => {
    const references = parseMarkdownResourceReferences([
      "<img src='assets/a.png'>",
      "<img src=\"assets/b.png\">",
      "<a href=assets/c.pdf>File</a>"
    ].join("\n"));

    expect(references.map(({ href, kind }) => ({ href, kind }))).toEqual([
      { href: "assets/a.png", kind: "html-image" },
      { href: "assets/b.png", kind: "html-image" },
      { href: "assets/c.pdf", kind: "html-attachment" }
    ]);
  });

  it("uses raw image alt text as the reference label", () => {
    expect(parseMarkdownResourceReferences("<img alt='Cover art' src='assets/cover.png'>"))
      .toEqual([
        expect.objectContaining({
          href: "assets/cover.png",
          text: "Cover art"
        })
      ]);
  });

  it.each([
    "`![code](assets/code.png)`",
    "```md\n![code](assets/code.png)\n```",
    "plain assets/plain.png text",
    "![remote](https://example.com/image.png)",
    "![embedded](data:image/png;base64,AAAA)",
    "[[assets/wiki.png]]",
    "[Document](notes/other.md)",
    "[Folder](assets/folder/)",
    "[Anchor](#section)",
    "[Mail](mailto:hello@example.com)",
    "![Protocol relative](//example.com/image.png)"
  ])("ignores non-resource syntax in %s", (markdown) => {
    expect(parseMarkdownResourceReferences(markdown)).toEqual([]);
  });
});
