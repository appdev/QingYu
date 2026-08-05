import { syntaxTree } from "@codemirror/language";
import { EditorState, type Extension } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import {
  imageAttributeDetails,
  replaceImageWidth,
} from "./image-attributes.ts";
import { liveMarkdown, markraLanguage } from "./index.ts";
import type { MarkraSyntaxNode } from "./renderers.ts";

function createState(doc: string, extension: Extension = markraLanguage) {
  return EditorState.create({ doc, extensions: [extension] });
}

function nodes(doc: string) {
  const state = createState(doc);
  const result: Array<{ name: string; from: number; to: number }> = [];
  syntaxTree(state).iterate({
    enter(node) {
      result.push({ name: node.name, from: node.from, to: node.to });
    },
  });
  return result;
}

function imageNodes(state: EditorState) {
  const result: MarkraSyntaxNode[] = [];
  syntaxTree(state).iterate({
    enter(node) {
      if (node.name === "Image") result.push(node.node as MarkraSyntaxNode);
    },
  });
  return result;
}

function details(doc: string, imageIndex = 0) {
  const state = createState(doc);
  const image = imageNodes(state)[imageIndex];
  if (!image) throw new Error(`Expected image ${imageIndex} in ${doc}`);
  return imageAttributeDetails(state, image);
}

function resize(doc: string, widthPx: number | null) {
  const state = createState(doc);
  const image = imageNodes(state)[0];
  if (!image) throw new Error(`Expected an image in ${doc}`);
  const attributes = imageAttributeDetails(state, image);
  const source = state.sliceDoc(attributes.ownedFrom, attributes.ownedTo);
  return replaceImageWidth(source, attributes, widthPx);
}

describe("image attribute syntax", () => {
  it("parses an adjacent attribute list with child source ranges", () => {
    const doc = "prefix ![alt](image.png){#hero .wide width=420px data-x=yes}";

    expect(nodes(doc).filter((node) => node.name.startsWith("ImageAttribute")))
      .toEqual([
        { name: "ImageAttributes", from: 24, to: 60 },
        { name: "ImageAttributeMark", from: 24, to: 25 },
        { name: "ImageAttributeName", from: 25, to: 30 },
        { name: "ImageAttributeName", from: 31, to: 36 },
        { name: "ImageAttributeName", from: 37, to: 42 },
        { name: "ImageAttributeValue", from: 43, to: 48 },
        { name: "ImageAttributeName", from: 49, to: 55 },
        { name: "ImageAttributeValue", from: 56, to: 59 },
        { name: "ImageAttributeMark", from: 59, to: 60 },
      ]);
  });

  it("owns only an exactly adjacent image attribute sibling", () => {
    const valid = "![alt](image.png){width=420px}";

    expect(nodes(valid).map(({ name }) => name)).toContain("ImageAttributes");
    expect(details(valid)).toMatchObject({
      ownedFrom: 0,
      attributesFrom: 17,
      attributesTo: valid.length,
      authoredWidthPx: 420,
      ownedTo: valid.length,
      widthValueFrom: 24,
      widthValueTo: 29,
    });
    expect(details("![alt](image.png) {width=420px}")).toMatchObject({
      attributesFrom: null,
      authoredWidthPx: null,
      ownedTo: 17,
    });
    expect(details("![alt](image.png)\n{width=420px}")).toMatchObject({
      attributesFrom: null,
      authoredWidthPx: null,
      ownedTo: 17,
    });
  });

  it.each([
    ["decimal", "![alt](image.png){width=1.5px}"],
    ["duplicate", "![alt](image.png){width=12px width=13px}"],
    ["zero", "![alt](image.png){width=0px}"],
    ["negative", "![alt](image.png){width=-12px}"],
    ["unitless", "![alt](image.png){width=12}"],
    ["percent", "![alt](image.png){width=12%}"],
    ["em", "![alt](image.png){width=12em}"],
  ])("rejects %s authored width", (_label, doc) => {
    expect(details(doc).authoredWidthPx).toBeNull();
  });

  it("preserves absolute ownership and value ranges after preceding text", () => {
    const doc = "before ![alt](image.png){#hero width=420px data-x=yes} after";

    expect(details(doc)).toMatchObject({
      ownedFrom: 7,
      attributesFrom: 24,
      attributesTo: 54,
      authoredWidthPx: 420,
      ownedTo: 54,
      widthValueFrom: 37,
      widthValueTo: 42,
    });
  });

  it("rejects incomplete, newline-containing, and nested attribute lists", () => {
    for (const doc of [
      "![a](x.png){width=12px",
      "![a](x.png){width=12px\ndata-x=yes}",
      "![a](x.png){width={12px}}",
    ]) {
      expect(details(doc)).toMatchObject({
        attributesFrom: null,
        authoredWidthPx: null,
      });
    }
  });

  it("stops at the first unescaped closing brace", () => {
    const doc = "![a](x.png){title=a\\}b width=12px} trailing }";

    expect(details(doc)).toMatchObject({
      authoredWidthPx: 12,
      ownedTo: 34,
    });
  });

  it("keeps two same-line images bound to their own adjacent attributes", () => {
    const doc = "![a](a.png){width=12px} ![b](b.png){width=34px}";
    const state = createState(doc);

    expect(imageNodes(state).map((image) => imageAttributeDetails(state, image)))
      .toMatchObject([
        { ownedFrom: 0, ownedTo: 23, authoredWidthPx: 12 },
        { ownedFrom: 24, ownedTo: 47, authoredWidthPx: 34 },
      ]);
  });

  it("does not own adjacent braces inside a Markdown link ancestor", () => {
    const doc = "[![a](x.png){width=12px}](target)";

    expect(details(doc)).toMatchObject({
      attributesFrom: null,
      authoredWidthPx: null,
      ownedTo: doc.indexOf(")") + 1,
      widthValueFrom: null,
      widthValueTo: null,
    });
  });

  it("scans quoted unknown values with spaces and a valid width", () => {
    const doc =
      `![a](x.png){title="wide hero" width=420px data-note='keep this'}`;

    expect(details(doc)).toMatchObject({
      attributesFrom: 11,
      authoredWidthPx: 420,
      ownedTo: doc.length,
    });
    expect(resize(doc, 360)).toBe(
      `![a](x.png){title="wide hero" width=360px data-note='keep this'}`,
    );
  });

  it("does not create owned image attributes in code, raw HTML, or ordinary prose", () => {
    for (const doc of [
      "`![a](x.png){width=12px}`",
      "```md\n![a](x.png){width=12px}\n```",
      "    ![a](x.png){width=12px}",
      "<div>\n![a](x.png){width=12px}\n</div>",
      "ordinary {width=12px} braces",
    ]) {
      const state = createState(doc);
      expect(imageNodes(state).map((image) => imageAttributeDetails(state, image)))
        .toEqual([]);
    }
  });

  it.each([
    ["inline dollar", "$![a](x.png){width=12px}$"],
    ["display dollar", "$$![a](x.png){width=12px}$$"],
    ["Hugo/backslash", String.raw`\(![a](x.png){width=12px}\)`],
  ])("does not bind adjacent attributes inside %s math", (_label, doc) => {
    expect(details(doc)).toMatchObject({
      attributesFrom: null,
      authoredWidthPx: null,
      ownedTo: doc.indexOf(")") + 1,
      widthValueFrom: null,
      widthValueTo: null,
    });
  });

  it("binds image attributes inside a GFM table cell", () => {
    const doc = "| image |\n| --- |\n| ![a](x.png){width=12px} |";

    expect(details(doc)).toMatchObject({ authoredWidthPx: 12 });
  });

  it("registers image attributes when live Markdown highlighting is disabled", () => {
    const doc = "![a](x.png){width=12px}";
    const state = createState(doc, liveMarkdown({ highlight: false }));
    const image = imageNodes(state)[0];

    expect(imageAttributeDetails(state, image)).toMatchObject({
      authoredWidthPx: 12,
      ownedTo: doc.length,
    });
  });

  it.each([
    ["non-safe integer", "9007199254740992"],
    ["non-finite integer", "9".repeat(400)],
  ])("rejects a %s width without giving up attribute ownership", (_label, width) => {
    const doc = `![a](x.png){#hero width=${width}px}`;

    expect(details(doc)).toMatchObject({
      attributesFrom: 11,
      authoredWidthPx: null,
      ownedTo: doc.length,
      widthValueFrom: null,
      widthValueTo: null,
    });
  });
});

describe("replaceImageWidth", () => {
  it.each([
    ["![a](x.png)", 420, "![a](x.png){width=420px}"],
    ["![a](x.png){width=320px}", 420, "![a](x.png){width=420px}"],
    [
      "![a](x.png){#hero width=320px data-x=yes}",
      420,
      "![a](x.png){#hero width=420px data-x=yes}",
    ],
    ["![a](x.png){#hero width=320px}", null, "![a](x.png){#hero}"],
    ["![a](x.png){width=320px}", null, "![a](x.png)"],
  ] as const)("transforms %s at width %s", (doc, widthPx, expected) => {
    expect(resize(doc, widthPx)).toBe(expected);
  });

  it("canonicalizes duplicate and invalid widths while retaining non-width token order", () => {
    const doc = "![a](x.png){#hero width=0px .wide width=13px data-x=yes}";

    expect(resize(doc, 420))
      .toBe("![a](x.png){#hero .wide data-x=yes width=420px}");
    expect(resize(doc, null))
      .toBe("![a](x.png){#hero .wide data-x=yes}");
  });

  it("uses absolute detail ranges for an owned source slice", () => {
    const doc = "prefix ![a](x.png){#hero width=320px data-x=yes} suffix";
    const state = createState(doc);
    const image = imageNodes(state)[0];
    const attributes = imageAttributeDetails(state, image);
    const ownedSource = state.sliceDoc(attributes.ownedFrom, attributes.ownedTo);

    expect(replaceImageWidth(ownedSource, attributes, 420))
      .toBe("![a](x.png){#hero width=420px data-x=yes}");
  });
});
