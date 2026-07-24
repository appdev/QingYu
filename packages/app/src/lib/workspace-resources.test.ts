import type { MarkdownResourceReference } from "@markra/markdown";
import { describe, expect, it } from "vitest";
import {
  buildWorkspaceResourceGraph,
  isManagedResourceRelativePath,
  type WorkspaceMarkdownFile,
  type WorkspaceResourceFile
} from "./workspace-resources";

function markdownFile(relativePath: string, root = "/vault"): WorkspaceMarkdownFile {
  return {
    name: relativePath.split("/").at(-1) ?? relativePath,
    path: `${root}/${relativePath}`,
    relativePath
  };
}

function resource(
  relativePath: string,
  root = "/vault",
  kind: "asset" | "attachment" = relativePath.endsWith(".png") ? "asset" : "attachment"
): WorkspaceResourceFile {
  return {
    kind,
    modifiedAt: 100,
    name: relativePath.split("/").at(-1) ?? relativePath,
    path: `${root}/${relativePath}`,
    relativePath,
    sizeBytes: 42
  };
}

function reference(href: string, kind: "image" | "attachment" = "image"): MarkdownResourceReference {
  return {
    columnNumber: 3,
    from: 10,
    href,
    kind,
    lineNumber: 2,
    text: "Resource",
    to: 30
  };
}

describe("isManagedResourceRelativePath", () => {
  it.each([
    ["assets/logo.png", true],
    ["docs/assets/manual.pdf", true],
    ["my-assets/logo.png", false],
    ["assets.txt/logo.png", false],
    ["assets", false],
    ["../assets/logo.png", false]
  ])("classifies %s", (path, expected) => {
    expect(isManagedResourceRelativePath(path)).toBe(expected);
  });
});

describe("buildWorkspaceResourceGraph", () => {
  it("keeps duplicate basenames in separate managed directories", () => {
    const rootDocument = markdownFile("index.md");
    const nestedDocument = markdownFile("docs/guide.md");
    const graph = buildWorkspaceResourceGraph({
      complete: true,
      failures: [],
      markdownFiles: [rootDocument, nestedDocument],
      occurrences: new Map([
        [rootDocument.path, [reference("assets/logo.png")]],
        [nestedDocument.path, [reference("assets/logo.png")]]
      ]),
      resources: [
        resource("assets/logo.png"),
        resource("docs/assets/logo.png"),
        resource("docs/assets/manual.pdf")
      ],
      workspaceRoot: "/vault"
    });

    expect(graph.existing.map(({ referenceCount, relativePath }) => ({ referenceCount, relativePath })))
      .toEqual([
        { referenceCount: 1, relativePath: "assets/logo.png" },
        { referenceCount: 1, relativePath: "docs/assets/logo.png" },
        { referenceCount: 0, relativePath: "docs/assets/manual.pdf" }
      ]);
    expect(graph.unused.map((file) => file.relativePath)).toEqual(["docs/assets/manual.pdf"]);
  });

  it("normalizes encoded Unicode, spaces, query strings, and fragments", () => {
    const document = markdownFile("docs/guide.md");
    const graph = buildWorkspaceResourceGraph({
      complete: true,
      failures: [],
      markdownFiles: [document],
      occurrences: new Map([
        [document.path, [reference("./assets/%E5%B0%81%E9%9D%A2%20%E5%9B%BE.png?raw=1#preview")]]
      ]),
      resources: [resource("docs/assets/封面 图.png")],
      workspaceRoot: "/vault"
    });

    expect(graph.existing[0]?.referenceCount).toBe(1);
    expect(graph.missing).toEqual([]);
    expect(graph.unused).toEqual([]);
  });

  it("groups one missing target across referring documents", () => {
    const first = markdownFile("index.md");
    const second = markdownFile("docs/guide.md");
    const graph = buildWorkspaceResourceGraph({
      complete: true,
      failures: [],
      markdownFiles: [first, second],
      occurrences: new Map([
        [first.path, [reference("assets/missing.pdf", "attachment")]],
        [second.path, [reference("../assets/missing.pdf", "attachment")]]
      ]),
      resources: [],
      workspaceRoot: "/vault"
    });

    expect(graph.missing).toHaveLength(1);
    expect(graph.missing[0]).toEqual(expect.objectContaining({
      href: "assets/missing.pdf",
      relativePath: "assets/missing.pdf"
    }));
    expect(graph.missing[0]?.occurrences.map((item) => item.sourceFile.relativePath))
      .toEqual(["docs/guide.md", "index.md"]);
  });

  it("withholds unused resources when any document failed", () => {
    const document = markdownFile("index.md");
    const graph = buildWorkspaceResourceGraph({
      complete: false,
      failures: [{ message: "Unreadable", path: document.path, stage: "read" }],
      markdownFiles: [document],
      occurrences: new Map([[document.path, [reference("assets/missing.png")]]]),
      resources: [resource("assets/unused.png")],
      workspaceRoot: "/vault"
    });

    expect(graph.complete).toBe(false);
    expect(graph.missing.map((item) => item.relativePath)).toEqual(["assets/missing.png"]);
    expect(graph.unused).toEqual([]);
  });

  it("ignores external, remote, markdown, directory, and similarly named paths", () => {
    const document = markdownFile("docs/guide.md");
    const graph = buildWorkspaceResourceGraph({
      complete: true,
      failures: [],
      markdownFiles: [document],
      occurrences: new Map([[document.path, [
        reference("../../../outside/assets/secret.png"),
        reference("https://example.com/assets/remote.png"),
        reference("assets/other.md", "attachment"),
        reference("assets/folder/", "attachment"),
        reference("../my-assets/not-managed.png")
      ]]]),
      resources: [resource("my-assets/ignored.png")],
      workspaceRoot: "/vault"
    });

    expect(graph.existing).toEqual([]);
    expect(graph.missing).toEqual([]);
    expect(graph.unused).toEqual([]);
  });

  it("accepts only absolute paths inside the workspace", () => {
    const document = markdownFile("index.md");
    const graph = buildWorkspaceResourceGraph({
      complete: true,
      failures: [],
      markdownFiles: [document],
      occurrences: new Map([[document.path, [
        reference("/vault/assets/inside.png"),
        reference("/other/assets/outside.png")
      ]]]),
      resources: [resource("assets/inside.png")],
      workspaceRoot: "/vault"
    });

    expect(graph.existing[0]?.referenceCount).toBe(1);
    expect(graph.missing).toEqual([]);
  });

  it("matches Windows workspace identities case-insensitively", () => {
    const root = "C:\\Vault";
    const document = markdownFile("Notes/Guide.md", root);
    const graph = buildWorkspaceResourceGraph({
      complete: true,
      failures: [],
      markdownFiles: [document],
      occurrences: new Map([[document.path, [reference("c:\\vault\\ASSETS\\Cover.PNG")]]]),
      resources: [resource("assets/Cover.png", root)],
      workspaceRoot: root
    });

    expect(graph.existing[0]?.referenceCount).toBe(1);
    expect(graph.unused).toEqual([]);
  });
});
