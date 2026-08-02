import type { DocumentState } from "@markra/shared";
import {
  createDocumentTab,
  draftWorkspacePatchFromTabs,
  isPristineUntitledDocument
} from "./document-model";

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
