import type { DocumentState } from "@markra/shared";
import {
  createDocumentTab,
  documentFromDraftTab,
  documentFromTab,
  draftTabFromDocumentTab,
  draftWorkspacePatchFromTabs,
  isPristineUntitledDocument,
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

const metadataOnlySource = "---\ntitle: 未命名\n---\n\n";

describe("markdown document model", () => {
  it("treats recognized Front-Matter-only documents as pristine and excludes dirty drafts from restart persistence", () => {
    const pristineDocument: DocumentState = {
      content: metadataOnlySource,
      deleted: false,
      dirty: false,
      name: "未命名.md",
      open: true,
      path: null,
      revision: 0
    };
    const dirtyTab = createDocumentTab({ ...pristineDocument, dirty: true, revision: 1 }, "untitled:1");

    expect(isPristineUntitledDocument(pristineDocument)).toBe(true);
    expect(draftWorkspacePatchFromTabs([dirtyTab], dirtyTab.id)).toEqual({
      activeDraftId: null,
      draftTabs: []
    });
  });

  it("keeps malformed Front Matter non-pristine and restart-persistable", () => {
    const malformedDocument: DocumentState = {
      content: "---\ntitle: [unterminated\n---\n\n",
      deleted: false,
      dirty: false,
      name: "未命名.md",
      open: true,
      path: null,
      revision: 0
    };
    const dirtyTab = createDocumentTab({ ...malformedDocument, dirty: true, revision: 1 }, "untitled:1");

    expect(isPristineUntitledDocument(malformedDocument)).toBe(false);
    expect(draftWorkspacePatchFromTabs([dirtyTab], dirtyTab.id)).toEqual({
      activeDraftId: dirtyTab.id,
      draftTabs: [{
        content: malformedDocument.content,
        id: dirtyTab.id,
        name: malformedDocument.name,
        path: null
      }]
    });
  });
});
