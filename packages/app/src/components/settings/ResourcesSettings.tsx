import { useEffect, useMemo, useState } from "react";
import type { I18nKey } from "@markra/shared";
import { getAppRuntime } from "../../runtime";
import { showAppToast } from "../../lib/app-toast";
import {
  useWorkspaceResources,
  type TrashSelectionResult
} from "../../hooks/useWorkspaceResources";
import type {
  WorkspaceExistingResource,
  WorkspaceMissingResource
} from "../../lib/workspace-resources";
import { ResourcePreview } from "./resources/ResourcePreview";
import { ResourceResultsList } from "./resources/ResourceResultsList";

export type ResourcesSettingsProps = {
  active: boolean;
  globalIgnoreRules: string;
  sourceWindowLabel: string | null;
  translate: (key: I18nKey) => string;
  workspaceSourcePath: string | null;
};

type ResourceTab = "missing" | "unused";

function resourceText(translate: ResourcesSettingsProps["translate"], key: string) {
  return translate(key as I18nKey);
}

function formatFileSize(value: number) {
  if (value < 1_024) return `${value} B`;
  if (value < 1_048_576) return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(value / 1_024)} KB`;
  return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(value / 1_048_576)} MB`;
}

function progressLabel(
  translate: ResourcesSettingsProps["translate"],
  progress: ReturnType<typeof useWorkspaceResources>["progress"]
) {
  const base = progress.phase === "inventory"
    ? resourceText(translate, "settings.resources.collecting")
    : progress.phase === "reading"
      ? resourceText(translate, "settings.resources.readingProgress")
      : progress.phase === "analyzing"
        ? resourceText(translate, "settings.resources.analyzingProgress")
        : resourceText(translate, "settings.resources.finalizing");
  return progress.total > 0 ? `${base} ${progress.completed}/${progress.total}` : base;
}

function InlineNotice({ description, title }: { description?: string; title: string }) {
  return (
    <div className="rounded-lg border border-(--border-default) bg-(--bg-secondary) px-3 py-2.5">
      <p className="m-0 text-[13px] font-[650] text-(--text-heading)">{title}</p>
      {description ? <p className="m-0 mt-1 text-[12px] text-(--text-secondary)">{description}</p> : null}
    </div>
  );
}

export function ResourcesSettings({
  active,
  globalIgnoreRules,
  sourceWindowLabel,
  translate,
  workspaceSourcePath
}: ResourcesSettingsProps) {
  const workspaceAvailable = Boolean(workspaceSourcePath && sourceWindowLabel);
  const resources = useWorkspaceResources({
    active: active && workspaceAvailable,
    globalIgnoreRules,
    sourceWindowLabel,
    workspaceSourcePath
  });
  const [activeTab, setActiveTab] = useState<ResourceTab>("unused");
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [selectedUnusedPath, setSelectedUnusedPath] = useState<string | null>(null);
  const [selectedMissingPath, setSelectedMissingPath] = useState<string | null>(null);
  const [trashRunning, setTrashRunning] = useState(false);
  const [trashResult, setTrashResult] = useState<TrashSelectionResult | null>(null);
  const unusedRows = resources.graph?.unused ?? [];
  const missingRows = resources.graph?.missing ?? [];
  const unusedByPath = useMemo(
    () => new Map(unusedRows.map((resource) => [resource.relativePath, resource])),
    [unusedRows]
  );
  const selectedResources = Array.from(selectedPaths)
    .map((path) => unusedByPath.get(path))
    .filter((resource): resource is WorkspaceExistingResource => resource !== undefined);
  const selectedUnused = selectedUnusedPath ? unusedByPath.get(selectedUnusedPath) ?? null : null;
  const selectedMissing = selectedMissingPath
    ? missingRows.find((resource) => resource.relativePath === selectedMissingPath) ?? null
    : null;

  useEffect(() => {
    if (!resources.graph) return;
    setSelectedPaths((current) => new Set(Array.from(current).filter((path) => unusedByPath.has(path))));
    if (selectedUnusedPath && !unusedByPath.has(selectedUnusedPath)) setSelectedUnusedPath(null);
    if (selectedMissingPath && !missingRows.some((resource) => resource.relativePath === selectedMissingPath)) {
      setSelectedMissingPath(null);
    }
  }, [missingRows, resources.graph, selectedMissingPath, selectedUnusedPath, unusedByPath]);

  const selectUnused = (resource: WorkspaceExistingResource, selected: boolean) => {
    setSelectedUnusedPath(resource.relativePath);
    setSelectedPaths((current) => {
      const next = new Set(current);
      if (selected) next.add(resource.relativePath);
      else next.delete(resource.relativePath);
      return next;
    });
  };
  const selectMissing = (resource: WorkspaceMissingResource) => {
    setSelectedMissingPath(resource.relativePath);
  };
  const handleSelectAll = (selected: boolean) => {
    setSelectedPaths(selected ? new Set(unusedRows.map((resource) => resource.relativePath)) : new Set());
  };
  const handleTrash = async () => {
    if (trashRunning || selectedResources.length === 0) return;
    const totalSize = selectedResources.reduce((total, resource) => total + resource.sizeBytes, 0);
    setTrashRunning(true);
    setTrashResult(null);
    const result = await resources.trashResources(selectedResources, {
      cancelLabel: translate("app.cancelDeleteMarkdownFile"),
      message: `${resourceText(translate, "settings.resources.confirmTrash")} ${selectedResources.length} · ${formatFileSize(totalSize)}`,
      okLabel: resourceText(translate, "settings.resources.confirmTrashAction")
    });
    setTrashRunning(false);
    setTrashResult(result);
    if (result.kind === "completed") {
      setSelectedPaths(new Set(result.failed.map((failure) => failure.relativePath)));
    }
  };
  const handleShowInFolder = () => {
    if (selectedResources.length !== 1) return;
    getAppRuntime().files.openContainingFolder(selectedResources[0]!.path).catch(() => {});
  };
  const handleOpenOccurrence = (path: string) => {
    getAppRuntime().files.openMarkdownFileInNewWindow(path).catch(() => {
      showAppToast({
        message: resourceText(translate, "settings.resources.openDocumentFailed"),
        status: "error"
      });
    });
  };
  const formatDate = (value: number) => new Intl.DateTimeFormat(document.documentElement.lang || undefined, {
    dateStyle: "medium",
    timeStyle: "short"
  }).format(new Date(value));
  const scanning = resources.status === "scanning";
  const activeRows = activeTab === "unused" ? unusedRows : missingRows;

  return (
    <div className="flex h-full min-h-[520px] flex-col gap-3" aria-labelledby="resources-settings-title">
      <h3 id="resources-settings-title" className="sr-only">
        {resourceText(translate, `settings.resources.${activeTab}`)}
      </h3>
      <div className="flex shrink-0 items-center gap-3">
        <div
          className="inline-flex rounded-lg bg-(--bg-secondary) p-1"
          role="tablist"
          aria-label={resourceText(translate, "settings.categories.resources")}
        >
          {(["unused", "missing"] as const).map((tab) => (
            <button
              key={tab}
              aria-selected={activeTab === tab}
              className="rounded-md border-0 bg-transparent px-3 py-1.5 text-[13px] font-[620] text-(--text-secondary) aria-selected:bg-(--bg-primary) aria-selected:text-(--text-heading) focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent)"
              role="tab"
              type="button"
              onClick={() => setActiveTab(tab)}
            >
              {resourceText(translate, `settings.resources.${tab}`)}
            </button>
          ))}
        </div>
        <button
          className="ml-auto rounded-md border border-(--border-default) bg-transparent px-3 py-1.5 text-[12px] font-[620] text-(--text-primary) hover:bg-(--bg-hover) disabled:opacity-50"
          disabled={!workspaceAvailable || scanning}
          type="button"
          onClick={resources.refresh}
        >
          {resourceText(translate, resources.status === "error" ? "settings.resources.retry" : "settings.resources.refresh")}
        </button>
      </div>

      <div className="flex min-h-0 shrink-0 items-center gap-3">
        {activeTab === "unused" ? (
          <label className="inline-flex items-center gap-2 text-[12px] text-(--text-secondary)">
            <input
              aria-label={resourceText(translate, "settings.resources.selectAll")}
              checked={unusedRows.length > 0 && selectedResources.length === unusedRows.length}
              disabled={!resources.canTrash || unusedRows.length === 0}
              type="checkbox"
              onChange={(event) => handleSelectAll(event.currentTarget.checked)}
            />
            {resourceText(translate, "settings.resources.selectAll")}
          </label>
        ) : null}
        {selectedResources.length > 0 ? (
          <span className="text-[12px] text-(--text-secondary)">
            {selectedResources.length} {resourceText(translate, "settings.resources.selectedCount")}
          </span>
        ) : null}
        <button
          className="ml-auto rounded-md border border-(--border-default) bg-transparent px-3 py-1.5 text-[12px] font-[620] text-(--text-primary) disabled:opacity-50"
          disabled={selectedResources.length !== 1 || trashRunning}
          type="button"
          onClick={handleShowInFolder}
        >
          {resourceText(translate, "settings.resources.showInFolder")}
        </button>
        <button
          className="rounded-md border border-(--border-default) bg-transparent px-3 py-1.5 text-[12px] font-[620] text-(--text-primary) disabled:opacity-50"
          disabled={!resources.canTrash || selectedResources.length === 0 || trashRunning}
          type="button"
          onClick={handleTrash}
        >
          {resourceText(translate, "settings.resources.moveToTrash")}
        </button>
      </div>

      {!workspaceAvailable ? (
        <InlineNotice
          description={resourceText(translate, "settings.resources.noWorkspaceDescription")}
          title={resourceText(translate, "settings.resources.noWorkspaceTitle")}
        />
      ) : null}
      {workspaceAvailable && resources.status === "incomplete" ? (
        <InlineNotice
          description={resourceText(translate, "settings.resources.incompleteDescription")}
          title={resourceText(translate, "settings.resources.incompleteTitle")}
        />
      ) : null}
      {resources.warning === "snapshot-unavailable" ? (
        <InlineNotice title={resourceText(translate, "settings.resources.snapshotUnavailable")} />
      ) : null}
      {resources.status === "error" ? (
        <InlineNotice title={resourceText(translate, "settings.resources.scanFailed")} />
      ) : null}
      {scanning && activeTab === "missing" && missingRows.length > 0 ? (
        <p className="m-0 text-[12px] text-(--text-secondary)">
          {resourceText(translate, "settings.resources.resultsUpdating")}
        </p>
      ) : null}
      {trashResult?.kind === "stale" ? (
        <InlineNotice title={resourceText(translate, "settings.resources.staleRescan")} />
      ) : null}
      {trashResult?.kind === "completed" ? (
        <InlineNotice title={resourceText(
          translate,
          trashResult.failed.length > 0
            ? "settings.resources.trashPartialFailure"
            : "settings.resources.trashSucceeded"
        )} />
      ) : null}

      <div className="grid min-h-0 flex-1 grid-rows-[minmax(180px,1fr)_minmax(150px,0.75fr)] gap-3">
        <div
          className="flex min-h-0 flex-col"
          role="region"
          aria-label={resourceText(translate, `settings.resources.${activeTab}`)}
        >
          <ResourceResultsList
            activeTab={activeTab}
            checkingLabel={progressLabel(translate, resources.progress)}
            emptyLabel={resourceText(
              translate,
              activeTab === "unused" ? "settings.resources.noUnused" : "settings.resources.noMissing"
            )}
            missing={missingRows}
            onSelectMissing={selectMissing}
            onSelectUnused={selectUnused}
            scanning={scanning}
            selectedMissingPath={selectedMissingPath}
            selectedUnusedPath={selectedUnusedPath}
            selectedUnusedPaths={selectedPaths}
            unused={unusedRows}
          />
        </div>
        <div
          className="min-h-0 overflow-auto rounded-lg border border-(--border-default) bg-(--bg-secondary) p-3"
          role="region"
          aria-label={resourceText(translate, "settings.resources.preview")}
        >
          <ResourcePreview
            absolutePathLabel={resourceText(translate, "settings.resources.absolutePath")}
            fileNameLabel={resourceText(translate, "settings.resources.fileName")}
            fileSizeLabel={resourceText(translate, "settings.resources.fileSize")}
            fileTypeLabel={resourceText(translate, "settings.resources.fileType")}
            formatDate={formatDate}
            formatSize={formatFileSize}
            missing={activeTab === "missing" ? selectedMissing : null}
            modifiedAtLabel={resourceText(translate, "settings.resources.modifiedAt")}
            onOpenOccurrence={handleOpenOccurrence}
            previewUnavailableLabel={resourceText(translate, "settings.resources.previewUnavailable")}
            referenceLineLabel={resourceText(translate, "settings.resources.referenceLine")}
            referencesLabel={resourceText(translate, "settings.resources.references")}
            relativePathLabel={resourceText(translate, "settings.resources.relativePath")}
            resource={activeTab === "unused" ? selectedUnused : null}
          />
        </div>
      </div>
      {scanning ? (
        <p className="sr-only" aria-live="polite">{progressLabel(translate, resources.progress)}</p>
      ) : null}
      <span className="sr-only">{activeRows.length}</span>
    </div>
  );
}
