import type {
  WorkspaceExistingResource,
  WorkspaceMissingResource
} from "../../../lib/workspace-resources";

export function ResourceResultsList({
  activeTab,
  checkingLabel,
  emptyLabel,
  missing,
  onSelectMissing,
  onSelectUnused,
  scanning,
  selectedMissingPath,
  selectedUnusedPath,
  selectedUnusedPaths,
  unused
}: {
  activeTab: "missing" | "unused";
  checkingLabel: string;
  emptyLabel: string;
  missing: readonly WorkspaceMissingResource[];
  onSelectMissing: (resource: WorkspaceMissingResource) => unknown;
  onSelectUnused: (resource: WorkspaceExistingResource, selected: boolean) => unknown;
  scanning: boolean;
  selectedMissingPath: string | null;
  selectedUnusedPath: string | null;
  selectedUnusedPaths: ReadonlySet<string>;
  unused: readonly WorkspaceExistingResource[];
}) {
  const rows = activeTab === "unused" ? unused : missing;

  return (
    <div className="min-h-0 flex-1 overflow-auto rounded-lg border border-(--border-default) bg-(--bg-secondary)">
      {rows.length === 0 && scanning ? (
        <div className="space-y-2 p-3" aria-label={checkingLabel}>
          {[0, 1, 2].map((index) => (
            <div
              key={index}
              className="h-10 animate-pulse rounded-md bg-(--bg-hover)"
              aria-hidden="true"
            />
          ))}
        </div>
      ) : null}
      {rows.length === 0 && !scanning ? (
        <p className="m-0 px-4 py-8 text-center text-[13px] text-(--text-secondary)">{emptyLabel}</p>
      ) : null}
      {activeTab === "unused" ? unused.map((resource) => {
        const selected = selectedUnusedPaths.has(resource.relativePath);
        const active = selectedUnusedPath === resource.relativePath;
        return (
          <div
            key={resource.relativePath}
            className="flex min-w-0 cursor-pointer items-center gap-3 border-b border-(--border-default) px-3 py-2.5 last:border-b-0 hover:bg-(--bg-hover) has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-inset has-[:focus-visible]:ring-(--accent)"
            data-active={active || undefined}
          >
            <input
              aria-label={resource.name}
              checked={selected}
              className="size-4 shrink-0 accent-(--accent)"
              type="checkbox"
              onChange={(event) => onSelectUnused(resource, event.currentTarget.checked)}
            />
            <button
              className="min-w-0 flex-1 border-0 bg-transparent p-0 text-left focus:outline-none"
              type="button"
              onClick={() => onSelectUnused(resource, selected)}
            >
              <span className="block truncate text-[13px] font-[620] text-(--text-heading)">{resource.name}</span>
              <span className="block truncate text-[12px] text-(--text-secondary)">{resource.relativePath}</span>
            </button>
          </div>
        );
      }) : missing.map((resource) => (
        <button
          key={resource.relativePath}
          className="block w-full border-0 border-b border-(--border-default) bg-transparent px-3 py-2.5 text-left last:border-b-0 hover:bg-(--bg-hover) focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-(--accent)"
          data-active={selectedMissingPath === resource.relativePath || undefined}
          type="button"
          onClick={() => onSelectMissing(resource)}
        >
          <span className="block truncate text-[13px] font-[620] text-(--text-heading)">{resource.relativePath}</span>
          <span className="block truncate text-[12px] text-(--text-secondary)">{resource.occurrences.length}</span>
        </button>
      ))}
    </div>
  );
}
