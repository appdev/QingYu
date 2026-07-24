import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { I18nKey } from "@markra/shared";
import type { WorkspaceResourceGraph } from "../../lib/workspace-resources";
import { showAppToast } from "../../lib/app-toast";
import { useWorkspaceResources } from "../../hooks/useWorkspaceResources";
import { ResourcesSettings } from "./ResourcesSettings";

vi.mock("../../hooks/useWorkspaceResources", () => ({
  useWorkspaceResources: vi.fn()
}));

vi.mock("../../lib/app-toast", () => ({
  showAppToast: vi.fn()
}));

const mockedUseWorkspaceResources = vi.mocked(useWorkspaceResources);
const mockedShowAppToast = vi.mocked(showAppToast);
const refresh = vi.fn();
const trashResources = vi.fn();

const copy: Record<string, string> = {
  "settings.resources.absolutePath": "Absolute path",
  "settings.resources.analyzingProgress": "Analyzing documents",
  "settings.resources.collecting": "Collecting workspace files",
  "settings.resources.confirmTrash": "Move selected resources to Trash?",
  "settings.resources.confirmTrashAction": "Move to Trash",
  "settings.resources.fileName": "File name",
  "settings.resources.fileSize": "File size",
  "settings.resources.fileType": "File type",
  "settings.resources.finalizing": "Finalizing results",
  "settings.resources.incompleteDescription": "Some files could not be checked.",
  "settings.resources.incompleteTitle": "Scan incomplete",
  "settings.resources.missing": "Missing resources",
  "settings.resources.modifiedAt": "Modified",
  "settings.resources.moveToTrash": "Move to Trash",
  "settings.resources.noMissing": "No missing resources",
  "settings.resources.noUnused": "No unused resources",
  "settings.resources.noWorkspaceDescription": "Open a Markdown file or folder first.",
  "settings.resources.noWorkspaceTitle": "No workspace",
  "settings.resources.openDocumentFailed": "Could not open document",
  "settings.resources.preview": "Preview",
  "settings.resources.previewUnavailable": "Preview unavailable",
  "settings.resources.readingProgress": "Reading documents",
  "settings.resources.referenceLine": "Line",
  "settings.resources.references": "References",
  "settings.resources.refresh": "Refresh",
  "settings.resources.relativePath": "Relative path",
  "settings.resources.resultsUpdating": "Results are still updating",
  "settings.resources.retry": "Retry",
  "settings.resources.scanFailed": "Resource scan failed",
  "settings.resources.selectAll": "Select all",
  "settings.resources.selectedCount": "selected",
  "settings.resources.showInFolder": "Show in Folder",
  "settings.resources.snapshotUnavailable": "Live editor state unavailable",
  "settings.resources.staleRescan": "Workspace changed; results were refreshed",
  "settings.resources.trashPartialFailure": "Some resources could not be moved",
  "settings.resources.trashSucceeded": "Resources moved to Trash",
  "settings.resources.unused": "Unused resources"
};

const translate = (key: I18nKey) => copy[key] ?? key;

function graph(patch: Partial<WorkspaceResourceGraph> = {}): WorkspaceResourceGraph {
  return {
    complete: true,
    existing: [],
    failures: [],
    missing: [],
    unused: [],
    ...patch
  };
}

function unused(relativePath: string, kind: "asset" | "attachment" = "asset") {
  return {
    kind,
    modifiedAt: 1_700_000_000_000,
    name: relativePath.split("/").at(-1) ?? relativePath,
    path: `/vault/${relativePath}`,
    referenceCount: 0,
    relativePath,
    sizeBytes: 2_048
  } as const;
}

function installState(patch: Partial<ReturnType<typeof useWorkspaceResources>> = {}) {
  mockedUseWorkspaceResources.mockReturnValue({
    canTrash: false,
    graph: null,
    progress: { completed: 0, phase: "inventory", total: 0 },
    refresh,
    snapshotGeneration: null,
    status: "scanning",
    trashResources,
    warning: null,
    ...patch
  });
}

function renderSettings(workspaceSourcePath: string | null = "/vault") {
  return render(
    <ResourcesSettings
      active
      globalIgnoreRules=""
      sourceWindowLabel="markra-editor-2"
      translate={translate}
      workspaceSourcePath={workspaceSourcePath}
    />
  );
}

beforeEach(() => {
  refresh.mockReset();
  trashResources.mockReset();
  trashResources.mockResolvedValue({ failed: [], kind: "completed", trashed: [] });
  mockedShowAppToast.mockReset();
  installState();
});

describe("ResourcesSettings", () => {
  it("renders the complete non-blocking settings shell during the first scan", () => {
    renderSettings();

    expect(screen.getByRole("heading", { name: "Unused resources" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Unused resources" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Missing resources" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Unused resources" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Preview" })).toBeInTheDocument();
    expect(screen.getByText("Collecting workspace files")).toHaveAttribute("aria-live", "polite");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("shows provisional missing rows and preserves them in an incomplete scan", () => {
    installState({
      graph: graph({
        complete: false,
        failures: [{ message: "unreadable", path: "/vault/broken.md", stage: "read" }],
        missing: [{
          href: "assets/missing.png",
          occurrences: [{
            columnNumber: 2,
            from: 1,
            href: "assets/missing.png",
            kind: "image",
            lineNumber: 7,
            sourceFile: { name: "index.md", path: "/vault/index.md", relativePath: "index.md" },
            text: "Missing",
            to: 20
          }],
          relativePath: "assets/missing.png"
        }]
      }),
      progress: { completed: 1, phase: "analyzing", total: 2 },
      status: "scanning"
    });
    const { rerender } = renderSettings();
    fireEvent.click(screen.getByRole("tab", { name: "Missing resources" }));

    expect(screen.getByText("assets/missing.png")).toBeInTheDocument();
    expect(screen.getByText("Results are still updating")).toBeInTheDocument();

    installState({
      canTrash: false,
      graph: mockedUseWorkspaceResources.mock.results.at(-1)?.value.graph,
      progress: { completed: 1, phase: "finalizing", total: 1 },
      status: "incomplete"
    });
    rerender(
      <ResourcesSettings
        active
        globalIgnoreRules=""
        sourceWindowLabel="markra-editor-2"
        translate={translate}
        workspaceSourcePath="/vault"
      />
    );
    expect(screen.getByText("Scan incomplete")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Move to Trash" })).toBeDisabled();
  });

  it("shows distinct empty and no-workspace states", () => {
    installState({ canTrash: true, graph: graph(), status: "ready" });
    const { rerender } = renderSettings();
    expect(screen.getByText("No unused resources")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("tab", { name: "Missing resources" }));
    expect(screen.getByText("No missing resources")).toBeInTheDocument();

    rerender(
      <ResourcesSettings
        active
        globalIgnoreRules=""
        sourceWindowLabel="markra-editor-2"
        translate={translate}
        workspaceSourcePath={null}
      />
    );
    expect(screen.getByText("No workspace")).toBeInTheDocument();
    expect(mockedUseWorkspaceResources).toHaveBeenLastCalledWith(expect.objectContaining({ active: false }));
  });

  it("selects unused resources and includes count and total size in confirmation", async () => {
    const first = unused("assets/first.png");
    const second = unused("assets/manual.pdf", "attachment");
    installState({
      canTrash: true,
      graph: graph({ existing: [first, second], unused: [first, second] }),
      status: "ready"
    });
    trashResources.mockResolvedValue({
      failed: [{ error: "busy", relativePath: second.relativePath, status: "failed" }],
      kind: "completed",
      trashed: [{ relativePath: first.relativePath, status: "trashed" }]
    });
    renderSettings();

    fireEvent.click(screen.getByRole("checkbox", { name: "Select all" }));
    expect(screen.getByText("2 selected")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Move to Trash" }));

    await waitFor(() => expect(trashResources).toHaveBeenCalledWith([first, second], expect.objectContaining({
      message: expect.stringMatching(/2.*4\s?KB/iu)
    })));
    expect(await screen.findByText("Some resources could not be moved")).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "manual.pdf" })).toBeChecked();
  });

  it("shows image previews and attachment metadata", () => {
    const image = unused("assets/cover image.png");
    const attachment = unused("assets/manual.pdf", "attachment");
    installState({
      canTrash: true,
      graph: graph({ existing: [image, attachment], unused: [image, attachment] }),
      status: "ready"
    });
    renderSettings();

    fireEvent.click(screen.getByText("cover image.png"));
    expect(screen.getByRole("img", { name: "cover image.png" })).toHaveAttribute(
      "src",
      "file:///vault/assets/cover%20image.png"
    );

    fireEvent.click(screen.getByText("manual.pdf"));
    const preview = screen.getByRole("region", { name: "Preview" });
    expect(within(preview).getByText("assets/manual.pdf")).toBeInTheDocument();
    expect(within(preview).getByText("PDF document")).toBeInTheDocument();
    expect(within(preview).getByText("2 KB")).toBeInTheDocument();
  });

  it("shows missing occurrence lines and reports document-open failures", async () => {
    installState({
      graph: graph({
        missing: [{
          href: "assets/missing.png",
          occurrences: [{
            columnNumber: 2,
            from: 1,
            href: "assets/missing.png",
            kind: "image",
            lineNumber: 7,
            sourceFile: { name: "index.md", path: "/vault/index.md", relativePath: "index.md" },
            text: "Missing",
            to: 20
          }],
          relativePath: "assets/missing.png"
        }]
      }),
      status: "ready"
    });
    renderSettings();
    fireEvent.click(screen.getByRole("tab", { name: "Missing resources" }));
    fireEvent.click(screen.getByText("assets/missing.png"));
    fireEvent.click(screen.getByRole("button", { name: /index\.md.*Line 7/iu }));

    await waitFor(() => expect(mockedShowAppToast).toHaveBeenCalledWith(expect.objectContaining({
      message: "Could not open document",
      status: "error"
    })));
  });
});
