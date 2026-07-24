import { useEffect, useState } from "react";
import { localFileUrlFromPath } from "../../../lib/document-export";
import type {
  WorkspaceExistingResource,
  WorkspaceMissingResource
} from "../../../lib/workspace-resources";

const imageExtensions = new Set(["avif", "bmp", "gif", "jpeg", "jpg", "png", "svg", "webp"]);

function extensionForName(name: string) {
  return name.includes(".") ? name.split(".").at(-1)?.toLocaleLowerCase() ?? "" : "";
}

function fileType(name: string) {
  const extension = extensionForName(name);
  if (extension === "pdf") return "PDF document";
  return extension ? `${extension.toLocaleUpperCase()} file` : "File";
}

function MetadataRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid grid-cols-[110px_minmax(0,1fr)] gap-3 text-[12px] leading-5">
      <dt className="text-(--text-secondary)">{label}</dt>
      <dd className="m-0 break-all text-(--text-primary)">{value}</dd>
    </div>
  );
}

export function ResourcePreview({
  absolutePathLabel,
  fileNameLabel,
  fileSizeLabel,
  fileTypeLabel,
  formatDate,
  formatSize,
  missing,
  modifiedAtLabel,
  onOpenOccurrence,
  previewUnavailableLabel,
  referenceLineLabel,
  referencesLabel,
  relativePathLabel,
  resource
}: {
  absolutePathLabel: string;
  fileNameLabel: string;
  fileSizeLabel: string;
  fileTypeLabel: string;
  formatDate: (value: number) => string;
  formatSize: (value: number) => string;
  missing: WorkspaceMissingResource | null;
  modifiedAtLabel: string;
  onOpenOccurrence: (path: string) => unknown;
  previewUnavailableLabel: string;
  referenceLineLabel: string;
  referencesLabel: string;
  relativePathLabel: string;
  resource: WorkspaceExistingResource | null;
}) {
  const [previewFailed, setPreviewFailed] = useState(false);
  useEffect(() => setPreviewFailed(false), [resource?.path]);

  if (missing) {
    return (
      <div className="space-y-3">
        <dl className="m-0 space-y-1">
          <MetadataRow label={relativePathLabel} value={missing.relativePath} />
        </dl>
        <div>
          <h4 className="m-0 mb-1 text-[12px] font-[650] text-(--text-heading)">{referencesLabel}</h4>
          <div className="space-y-1">
            {missing.occurrences.map((occurrence) => (
              <button
                key={`${occurrence.sourceFile.path}:${occurrence.from}`}
                className="block w-full rounded-md border border-(--border-default) bg-transparent px-2.5 py-2 text-left text-[12px] text-(--text-primary) hover:bg-(--bg-hover) focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent)"
                type="button"
                onClick={() => onOpenOccurrence(occurrence.sourceFile.path)}
              >
                {occurrence.sourceFile.name} · {referenceLineLabel} {occurrence.lineNumber}
              </button>
            ))}
          </div>
        </div>
      </div>
    );
  }
  if (!resource) return <div className="h-full" />;

  const extension = extensionForName(resource.name);
  if (imageExtensions.has(extension)) {
    return previewFailed ? (
      <p className="m-0 text-[13px] text-(--text-secondary)">{previewUnavailableLabel}</p>
    ) : (
      <img
        alt={resource.name}
        className="h-full max-h-48 w-full object-contain"
        src={localFileUrlFromPath(resource.path)}
        onError={() => setPreviewFailed(true)}
      />
    );
  }

  return (
    <dl className="m-0 space-y-1">
      <MetadataRow label={fileNameLabel} value={resource.name} />
      <MetadataRow label={relativePathLabel} value={resource.relativePath} />
      <MetadataRow label={absolutePathLabel} value={resource.path} />
      <MetadataRow label={fileTypeLabel} value={fileType(resource.name)} />
      <MetadataRow label={fileSizeLabel} value={formatSize(resource.sizeBytes)} />
      <MetadataRow label={modifiedAtLabel} value={formatDate(resource.modifiedAt)} />
    </dl>
  );
}
