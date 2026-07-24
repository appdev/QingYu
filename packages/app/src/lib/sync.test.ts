import { runApplicationSync } from "./sync";

describe("application sync runtime boundary", () => {
  it("passes the primary notes request through unchanged", async () => {
    const request = {
      notebookName: "notes",
      notesRoot: "/canonical/notes",
      revision: "rev-1",
      trigger: "manual" as const
    };
    const expectedResult = {
      notebookName: "notes",
      notesRoot: "/canonical/notes",
      provider: "webdav" as const,
      revision: "rev-1",
      summary: {
        bytesDownloaded: 0,
        bytesUploaded: 0,
        conflictFiles: 0,
        downloadedFiles: 0,
        scannedFiles: 0,
        skippedFiles: 0,
        uploadedFiles: 0
      },
      trigger: "manual" as const
    };
    const sync = vi.fn(async () => expectedResult);

    await runApplicationSync(request, { sync });

    expect(sync).toHaveBeenCalledWith(request);
  });
});
