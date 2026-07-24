import type { NativeMarkdownFolderFile } from "../../lib/tauri";
import { sameNativePath } from "../../lib/path-move";

export type FolderNode = {
  type: "folder";
  createdAt?: number;
  name: string;
  path: string | null;
  relativePath: string;
  modifiedAt?: number;
  children: TreeNode[];
};

export type FileNode = {
  type: "file";
  file: NativeMarkdownFolderFile;
  name: string;
  relativePath: string;
};

export type TreeNode = FolderNode | FileNode;
export type FileTreeSortKey = "createdAt" | "modifiedAt" | "name";
export type FileTreeSortDirection = "ascending" | "descending";
export type FileTreeSort = {
  direction: FileTreeSortDirection;
  key: FileTreeSortKey;
};
export type FileTreeCreateRowKind = "file" | "folder";
export type FileTreeRenderItem =
  | {
    createType: FileTreeCreateRowKind;
    depth: number;
    key: string;
    type: "create";
  }
  | {
    depth: number;
    expanded?: boolean;
    key: string;
    node: TreeNode;
    type: "node";
  };
export type VirtualFileTreeRow = {
  index: number;
  key: string | number;
  size: number;
  start: number;
};
export type FileTreeRowRenderMode = "nested" | "virtual";
export type FileTreeMoveTarget =
  | {
    depth: 0;
    kind: "root";
    path: null;
  }
  | {
    depth: number;
    kind: "folder";
    node: FolderNode;
    path: string;
  };

export const defaultFileTreeSort = {
  direction: "ascending",
  key: "name"
} satisfies FileTreeSort;
export const fileTreeRowHeight = 32;
export const fileTreeVirtualizationThreshold = 200;
export const fallbackFileTreeViewportHeight = 320;

const fileTreeVirtualOverscanCount = 10;
const fileTreeNameCollator = new Intl.Collator(undefined, {
  numeric: true,
  sensitivity: "base"
});
const protectedProjectFolders = new Set([".markra-sync", ".qingyu"]);

function protectedProjectRelativePath(relativePath: string) {
  const firstPathPart = relativePath.split(/[\\/]/u).find(Boolean);
  return firstPathPart ? protectedProjectFolders.has(firstPathPart) : false;
}

function nodeNameSort(left: TreeNode, right: TreeNode) {
  return fileTreeNameCollator.compare(left.name, right.name);
}

function nodeTimestamp(node: TreeNode, key: Exclude<FileTreeSortKey, "name">) {
  const value = node.type === "folder" ? node[key] : node.file[key];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function sortTreeNodes(nodes: TreeNode[], sort: FileTreeSort) {
  nodes.sort((a, b) => {
    if (a.type !== b.type) return a.type === "folder" ? -1 : 1;
    if (sort.key === "name") {
      return sort.direction === "ascending" ? nodeNameSort(a, b) : nodeNameSort(b, a);
    }

    const leftTimestamp = nodeTimestamp(a, sort.key);
    const rightTimestamp = nodeTimestamp(b, sort.key);
    if (leftTimestamp !== null && rightTimestamp !== null && leftTimestamp !== rightTimestamp) {
      return sort.direction === "ascending" ? leftTimestamp - rightTimestamp : rightTimestamp - leftTimestamp;
    }
    if (leftTimestamp !== null && rightTimestamp === null) return -1;
    if (leftTimestamp === null && rightTimestamp !== null) return 1;
    return nodeNameSort(a, b);
  });

  nodes.forEach((node) => {
    if (node.type === "folder") sortTreeNodes(node.children, sort);
  });
}

export function normalizeCreateParentPath(path: string | null | undefined) {
  const trimmedPath = path?.trim();
  return trimmedPath ? trimmedPath : null;
}

function nativePathSeparator(path: string) {
  return path.includes("\\") && !path.includes("/") ? "\\" : "/";
}

function joinNativePath(rootPath: string | null | undefined, relativePath: string) {
  const normalizedRootPath = normalizeCreateParentPath(rootPath);
  if (!normalizedRootPath) return null;

  const pathSeparator = nativePathSeparator(normalizedRootPath);
  const normalizedRelativePath = relativePath
    .split(/[\\/]/)
    .filter(Boolean)
    .join(pathSeparator);

  if (!normalizedRelativePath) return normalizedRootPath;

  return `${normalizedRootPath.replace(/[\\/]+$/, "")}${pathSeparator}${normalizedRelativePath}`;
}

export function parentPathFromPath(path: string) {
  const lastSeparatorIndex = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  if (lastSeparatorIndex < 0) return null;
  if (lastSeparatorIndex === 0) return path.slice(0, 1);
  return path.slice(0, lastSeparatorIndex);
}

export function buildMarkdownFileTree(
  files: NativeMarkdownFolderFile[],
  rootPath?: string | null,
  sort: FileTreeSort = defaultFileTreeSort
) {
  const rootNodes: TreeNode[] = [];
  const folders = new Map<string, FolderNode>();

  const ensureFolder = (
    relativePath: string,
    siblings: TreeNode[],
    folderName: string,
    folderPath = joinNativePath(rootPath, relativePath)
  ) => {
    let folder = folders.get(relativePath);

    if (!folder) {
      folder = {
        type: "folder",
        createdAt: undefined,
        name: folderName,
        path: folderPath,
        relativePath,
        modifiedAt: undefined,
        children: []
      };
      folders.set(relativePath, folder);
      siblings.push(folder);
    } else if (!folder.path && folderPath) {
      folder.path = folderPath;
    }

    return folder;
  };

  files.forEach((file) => {
    if (protectedProjectRelativePath(file.relativePath)) return;

    const parts = file.relativePath.split(/[\\/]/).filter(Boolean);
    if (parts.length === 0) return;

    let siblings = rootNodes;
    let parentPath = "";

    parts.slice(0, -1).forEach((folderName) => {
      const relativePath = parentPath ? `${parentPath}/${folderName}` : folderName;
      const folder = ensureFolder(relativePath, siblings, folderName);

      siblings = folder.children;
      parentPath = relativePath;
    });

    if (file.kind === "folder") {
      const folder = ensureFolder(file.relativePath, siblings, parts.at(-1) ?? file.name, file.path);
      folder.createdAt = file.createdAt;
      folder.modifiedAt = file.modifiedAt;
      return;
    }

    siblings.push({
      type: "file",
      file,
      name: parts.at(-1) ?? file.name,
      relativePath: file.relativePath
    });
  });

  sortTreeNodes(rootNodes, sort);
  return rootNodes;
}

export function filterMarkdownFileTree(nodes: TreeNode[], query: string): TreeNode[] {
  const normalizedQuery = query.trim().toLowerCase();
  if (!normalizedQuery) return nodes;

  return nodes.flatMap((node): TreeNode[] => {
    if (node.type === "file") {
      return node.relativePath.toLowerCase().includes(normalizedQuery) ? [node] : [];
    }

    const children = filterMarkdownFileTree(node.children, normalizedQuery);
    return children.length > 0 ? [{ ...node, children }] : [];
  });
}

export function filterMarkdownFileTreeAssets(nodes: TreeNode[]): TreeNode[] {
  const filterNodes = (treeNodes: TreeNode[]): { hiddenAssetFound: boolean; nodes: TreeNode[] } => {
    let hiddenAssetFound = false;
    const filteredNodes: TreeNode[] = [];

    treeNodes.forEach((node) => {
      if (node.type === "file") {
        if (node.file.kind === "asset") {
          hiddenAssetFound = true;
          return;
        }

        filteredNodes.push(node);
        return;
      }

      const children = filterNodes(node.children);
      hiddenAssetFound = hiddenAssetFound || children.hiddenAssetFound;
      if (children.nodes.length > 0 || !children.hiddenAssetFound) {
        filteredNodes.push({ ...node, children: children.nodes });
      }
    });

    return { hiddenAssetFound, nodes: filteredNodes };
  };

  return filterNodes(nodes).nodes;
}

export function collectMarkdownFolderPaths(nodes: TreeNode[]) {
  const paths: string[] = [];

  const collect = (treeNodes: TreeNode[]) => {
    treeNodes.forEach((node) => {
      if (node.type !== "folder") return;

      paths.push(node.relativePath);
      collect(node.children);
    });
  };

  collect(nodes);
  return paths;
}

export function collectMarkdownFolderTargetPaths(nodes: TreeNode[]) {
  const paths: string[] = [];

  const collect = (treeNodes: TreeNode[]) => {
    treeNodes.forEach((node) => {
      if (node.type !== "folder") return;

      if (node.path) paths.push(node.path);
      collect(node.children);
    });
  };

  collect(nodes);
  return paths;
}

function normalizedRelativeTreePath(path: string) {
  return path.replace(/\\/gu, "/").replace(/^\/+|\/+$/gu, "");
}

function relativeTreePathIsWithin(path: string, parentPath: string) {
  const normalizedPath = normalizedRelativeTreePath(path);
  const normalizedParentPath = normalizedRelativeTreePath(parentPath);
  return normalizedPath === normalizedParentPath || normalizedPath.startsWith(`${normalizedParentPath}/`);
}

export function collectMarkdownMoveTargets(
  nodes: TreeNode[],
  movingFile: NativeMarkdownFolderFile | null | undefined,
  rootPath?: string | null
) {
  const currentParentPath = movingFile ? parentPathFromPath(movingFile.path) : null;
  const targets: FileTreeMoveTarget[] = sameNativePath(currentParentPath, rootPath)
    ? []
    : [{ depth: 0, kind: "root", path: null }];

  const collect = (treeNodes: TreeNode[], depth: number) => {
    treeNodes.forEach((node) => {
      if (node.type !== "folder" || !node.path) return;

      const movingFolderContainsTarget = movingFile?.kind === "folder" && (
        relativeTreePathIsWithin(node.relativePath, movingFile.relativePath)
        || sameNativePath(node.path, movingFile.path)
      );
      if (movingFolderContainsTarget) return;

      if (!sameNativePath(node.path, currentParentPath)) {
        targets.push({ depth, kind: "folder", node, path: node.path });
      }
      collect(node.children, depth + 1);
    });
  };

  collect(nodes, 1);
  return targets;
}

export function expandedFolderPathsForFileTreePath(nodes: TreeNode[], path: string | null | undefined) {
  if (!path?.trim()) return null;

  const findPath = (treeNodes: TreeNode[], parents: string[]): string[] | null => {
    for (const node of treeNodes) {
      if (node.type === "file") {
        if (sameNativePath(node.file.path, path)) return parents;
        continue;
      }

      const result = findPath(node.children, [...parents, node.relativePath]);
      if (result) return result;
    }

    return null;
  };

  return findPath(nodes, []);
}

export function folderNodeAsFile(node: FolderNode): NativeMarkdownFolderFile {
  return {
    createdAt: node.createdAt,
    kind: "folder",
    modifiedAt: node.modifiedAt,
    name: node.name,
    path: node.path ?? node.relativePath,
    relativePath: node.relativePath
  };
}

export function creatingAtFileTreeParentPath(
  parentPath: string | null | undefined,
  depth: number,
  creatingParentPath: string | null
) {
  const normalizedParentPath = normalizeCreateParentPath(parentPath);
  if (!normalizedParentPath && depth > 0) return false;
  return normalizedParentPath === creatingParentPath;
}

function appendFileTreeCreateRows(
  rows: FileTreeRenderItem[],
  parentPath: string | null | undefined,
  depth: number,
  creatingFile: boolean,
  creatingFolder: boolean,
  creatingParentPath: string | null
) {
  if ((!creatingFile && !creatingFolder) || !creatingAtFileTreeParentPath(parentPath, depth, creatingParentPath)) {
    return;
  }

  const parentKey = normalizeCreateParentPath(parentPath) ?? "__root__";
  if (creatingFile) {
    rows.push({
      createType: "file",
      depth,
      key: `create:file:${parentKey}:${depth}`,
      type: "create"
    });
  }

  if (creatingFolder) {
    rows.push({
      createType: "folder",
      depth,
      key: `create:folder:${parentKey}:${depth}`,
      type: "create"
    });
  }
}

export function folderExpandedForFileTreeRows(
  node: FolderNode,
  depth: number,
  expandedFolders: ReadonlySet<string>,
  searchActive: boolean,
  creatingFile: boolean,
  creatingFolder: boolean,
  creatingParentPath: string | null
) {
  return searchActive ||
    expandedFolders.has(node.relativePath) ||
    ((creatingFile || creatingFolder) &&
      creatingAtFileTreeParentPath(node.path, depth + 1, creatingParentPath));
}

export function buildVisibleFileTreeRows(
  nodes: TreeNode[],
  expandedFolders: ReadonlySet<string>,
  searchQuery: string,
  creatingFile: boolean,
  creatingFolder: boolean,
  creatingParentPath: string | null,
  depth = 0,
  parentPath: string | null = null,
  rows: FileTreeRenderItem[] = []
) {
  const searchActive = searchQuery.trim().length > 0;

  appendFileTreeCreateRows(rows, parentPath, depth, creatingFile, creatingFolder, creatingParentPath);

  nodes.forEach((node) => {
    if (node.type === "folder") {
      const expanded = folderExpandedForFileTreeRows(
        node,
        depth,
        expandedFolders,
        searchActive,
        creatingFile,
        creatingFolder,
        creatingParentPath
      );

      rows.push({
        depth,
        expanded,
        key: `folder:${node.relativePath}`,
        node,
        type: "node"
      });

      if (expanded) {
        buildVisibleFileTreeRows(
          node.children,
          expandedFolders,
          searchQuery,
          creatingFile,
          creatingFolder,
          creatingParentPath,
          depth + 1,
          node.path,
          rows
        );
      }
      return;
    }

    rows.push({
      depth,
      key: `file:${node.file.path}`,
      node,
      type: "node"
    });
  });

  return rows;
}

export function virtualFileTreeRowsForOffset(
  rows: readonly FileTreeRenderItem[],
  getFileTreeRowKey: (index: number) => string | number,
  scrollOffset: number,
  viewportHeight: number
) {
  const visibleRowCount = Math.ceil(viewportHeight / fileTreeRowHeight);
  const renderedRowCount = visibleRowCount + fileTreeVirtualOverscanCount * 2;
  const rawStartIndex = Math.max(0, Math.floor(scrollOffset / fileTreeRowHeight) - fileTreeVirtualOverscanCount);
  const maxStartIndex = Math.max(0, rows.length - renderedRowCount);
  const startIndex = Math.min(rawStartIndex, maxStartIndex);
  const endIndex = Math.min(rows.length, startIndex + renderedRowCount);
  const virtualRows: VirtualFileTreeRow[] = [];

  for (let index = startIndex; index < endIndex; index += 1) {
    virtualRows.push({
      index,
      key: getFileTreeRowKey(index),
      size: fileTreeRowHeight,
      start: index * fileTreeRowHeight
    });
  }

  return virtualRows;
}

export function fileTreeRowIndentStyle(depth: number, mode: FileTreeRowRenderMode) {
  return mode === "virtual" ? { paddingLeft: `${32 + depth * 20}px` } : undefined;
}

export function fileTreeRowIndentClass(mode: FileTreeRowRenderMode) {
  return mode === "virtual" ? "" : "pl-8";
}
