import {
  documentFromDraftTab,
  documentFromTab,
  draftTabFromDocumentTab,
  type MarkdownDocumentTab
} from "./document-model";

const tab = {
  content: "draft",
  deleted: false,
  dirty: true,
  id: "draft-1",
  name: "Draft.md",
  open: true,
  path: null,
  revision: 3
} satisfies MarkdownDocumentTab;

describe("draft creation directory document mappings", () => {
  it("documentFromTab preserves a value, explicit null, and absence", () => {
    expect(documentFromTab({
      ...tab,
      creationDirectory: "kernel-workspace://primary/abc"
    })).toHaveProperty("creationDirectory", "kernel-workspace://primary/abc");
    expect(documentFromTab({ ...tab, creationDirectory: null }))
      .toHaveProperty("creationDirectory", null);
    expect(documentFromTab(tab)).not.toHaveProperty("creationDirectory");
  });

  it("documentFromDraftTab preserves a value, explicit null, and absence", () => {
    expect(documentFromDraftTab({
      content: "draft",
      creationDirectory: "kernel-workspace://primary/abc",
      id: "draft-1",
      name: "Draft.md",
      path: null
    }, 4)).toHaveProperty("creationDirectory", "kernel-workspace://primary/abc");
    expect(documentFromDraftTab({
      content: "draft",
      creationDirectory: null,
      id: "draft-1",
      name: "Draft.md",
      path: null
    }, 4)).toHaveProperty("creationDirectory", null);
    expect(documentFromDraftTab({
      content: "draft",
      id: "draft-1",
      name: "Draft.md",
      path: null
    }, 4)).not.toHaveProperty("creationDirectory");
  });

  it("draftTabFromDocumentTab preserves a value, explicit null, and absence", () => {
    expect(draftTabFromDocumentTab({
      ...tab,
      creationDirectory: "kernel-workspace://primary/abc"
    })).toEqual({
      content: "draft",
      creationDirectory: "kernel-workspace://primary/abc",
      id: "draft-1",
      name: "Draft.md",
      path: null
    });
    expect(draftTabFromDocumentTab({ ...tab, creationDirectory: null })).toEqual({
      content: "draft",
      creationDirectory: null,
      id: "draft-1",
      name: "Draft.md",
      path: null
    });
    expect(draftTabFromDocumentTab(tab)).toEqual({
      content: "draft",
      id: "draft-1",
      name: "Draft.md",
      path: null
    });
  });
});
