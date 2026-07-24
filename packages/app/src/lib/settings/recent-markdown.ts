import { pathNameFromPath } from "@markra/shared";

export type RecentMarkdownFolder = {
  name: string;
  path: string;
};

export type RecentMarkdownFile = {
  name: string;
  path: string;
};

export type RecentNotebook = RecentMarkdownFolder;

export const recentMarkdownFilesMaxLength = 10;
export const recentNotebooksMaxLength = 5;

export function normalizeRecentMarkdownFiles(value: unknown): RecentMarkdownFile[] {
  if (!Array.isArray(value)) return [];

  const seenPaths = new Set<string>();
  const files: RecentMarkdownFile[] = [];

  value.forEach((item) => {
    if (files.length >= recentMarkdownFilesMaxLength) return;
    if (typeof item !== "object" || item === null) return;

    const candidate = item as Partial<RecentMarkdownFile>;
    const path = typeof candidate.path === "string" ? candidate.path.trim() : "";
    if (!path || seenPaths.has(path)) return;

    const name = typeof candidate.name === "string" ? candidate.name.trim() : "";
    seenPaths.add(path);
    files.push({
      name: name || pathNameFromPath(path),
      path
    });
  });

  return files;
}

export function prependRecentMarkdownFile(
  files: readonly RecentMarkdownFile[],
  file: RecentMarkdownFile
) {
  return normalizeRecentMarkdownFiles([file, ...files]);
}

export function normalizeRecentNotebooks(value: unknown): RecentNotebook[] {
  if (!Array.isArray(value)) return [];

  const seenPaths = new Set<string>();
  const notebooks: RecentNotebook[] = [];

  value.forEach((item) => {
    if (notebooks.length >= recentNotebooksMaxLength) return;
    if (typeof item !== "object" || item === null) return;

    const candidate = item as Partial<RecentNotebook>;
    if (
      typeof candidate.name !== "string" || candidate.name.length === 0 ||
      typeof candidate.path !== "string" || candidate.path.length === 0 ||
      seenPaths.has(candidate.path)
    ) return;

    seenPaths.add(candidate.path);
    notebooks.push({ name: candidate.name, path: candidate.path });
  });

  return notebooks;
}

export function prependRecentNotebook(
  notebooks: readonly RecentNotebook[],
  notebook: RecentNotebook
) {
  return normalizeRecentNotebooks([notebook, ...notebooks]);
}
