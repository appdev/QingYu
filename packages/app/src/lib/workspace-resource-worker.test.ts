import { describe, expect, it } from "vitest";
import {
  analyzeWorkspaceResourceBatch,
  isWorkspaceResourceWorkerRequest,
  isWorkspaceResourceWorkerResponse,
  type WorkspaceResourceWorkerRequest
} from "./workspace-resource-worker";

describe("workspace resource worker protocol", () => {
  it("accepts only bounded analyze requests with serializable documents", () => {
    expect(isWorkspaceResourceWorkerRequest({
      documents: [{ content: "![Cover](assets/cover.png)", path: "/vault/index.md" }],
      scanId: 3,
      type: "analyze"
    })).toBe(true);
    expect(isWorkspaceResourceWorkerRequest({
      documents: Array.from({ length: 5 }, (_, index) => ({ content: "", path: `${index}.md` })),
      scanId: 3,
      type: "analyze"
    })).toBe(false);
    expect(isWorkspaceResourceWorkerRequest({ documents: [], scanId: -1, type: "analyze" })).toBe(false);
  });

  it("reports a malformed sibling but still analyzes valid documents without echoing content", () => {
    const malformed = {
      documents: [
        { content: "![Cover](assets/cover.png)", path: "/vault/index.md" },
        { content: null, path: "/vault/broken.md" }
      ],
      scanId: 7,
      type: "analyze"
    } as unknown as WorkspaceResourceWorkerRequest;

    const responses = analyzeWorkspaceResourceBatch(malformed);

    expect(responses).toEqual([
      {
        error: expect.any(String),
        path: "/vault/broken.md",
        scanId: 7,
        type: "failed"
      },
      {
        occurrences: [{
          path: "/vault/index.md",
          references: [expect.objectContaining({ href: "assets/cover.png" })]
        }],
        scanId: 7,
        type: "analyzed"
      }
    ]);
    expect(JSON.stringify(responses)).not.toContain("![Cover]");
    expect(responses.every(isWorkspaceResourceWorkerResponse)).toBe(true);
  });

  it("rejects malformed worker responses", () => {
    expect(isWorkspaceResourceWorkerResponse({ occurrences: [], scanId: 1, type: "analyzed" })).toBe(true);
    expect(isWorkspaceResourceWorkerResponse({ occurrences: [], scanId: 1, type: "failed" })).toBe(false);
    expect(isWorkspaceResourceWorkerResponse({ error: "bad", path: "", scanId: 1, type: "failed" })).toBe(false);
  });
});
