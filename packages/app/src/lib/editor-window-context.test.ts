import { parseEditorWindowContext } from "./editor-window-context";

describe("editor window context", () => {
  it.each([
    ["", { kind: "primary" }],
    ["?unrelated=1", { kind: "primary" }],
    ["?blank=1", { kind: "external-blank" }],
    ["?path=%2FExternal%2Fnote.md", { kind: "external-file", path: "/External/note.md" }],
    ["?folder=%2FExternal", { kind: "external-folder", path: "/External" }]
  ] as const)("parses %s without duplicating query checks", (search, expected) => {
    expect(parseEditorWindowContext(search)).toEqual(expected);
  });

  it("uses one deterministic external target when malformed launch parameters are combined", () => {
    expect(parseEditorWindowContext("?blank=1&path=%2FExternal%2Fnote.md&folder=%2FExternal"))
      .toEqual({ kind: "external-file", path: "/External/note.md" });
  });

  it("prefers an external folder over a blank launch when both targets are present", () => {
    expect(parseEditorWindowContext("?blank=1&folder=%2FExternal"))
      .toEqual({ kind: "external-folder", path: "/External" });
  });
});
