import type {
  KernelCreatedDocumentSnapshot,
  KernelDocumentEntrySnapshot,
  KernelDocumentSnapshot,
  KernelDomainPort,
  KernelPageCursor,
  KernelSearchMatchSnapshot,
  KernelWorkspaceRelativePath,
  KernelWorkspaceSnapshot
} from "../runtime/kernel-domain";

export type PrimaryWorkspaceCreateDocumentInput =
  | {
      readonly contents: string;
      readonly kind: "file";
      readonly name: string;
      readonly parent: KernelWorkspaceRelativePath;
    }
  | {
      readonly kind: "directory";
      readonly name: string;
      readonly parent: KernelWorkspaceRelativePath;
    };

export type PrimaryWorkspaceDocumentControllerErrorCode =
  | "document-not-indexed"
  | "protocol-mismatch"
  | "rebuild-required"
  | "workspace-generation-drift";

const ERROR_MESSAGES: Record<PrimaryWorkspaceDocumentControllerErrorCode, string> = {
  "document-not-indexed": "The primary workspace document is not indexed.",
  "protocol-mismatch": "The primary workspace document response did not match its request.",
  "rebuild-required": "The primary workspace document controller must be rebuilt.",
  "workspace-generation-drift": "The primary workspace changed and the document controller must be rebuilt."
};

export class PrimaryWorkspaceDocumentControllerError extends Error {
  readonly code: PrimaryWorkspaceDocumentControllerErrorCode;

  constructor(code: PrimaryWorkspaceDocumentControllerErrorCode) {
    super(ERROR_MESSAGES[code]);
    this.name = "PrimaryWorkspaceDocumentControllerError";
    this.code = code;
  }
}

export interface PrimaryWorkspaceDocumentController {
  readonly create: (
    input: PrimaryWorkspaceCreateDocumentInput
  ) => Promise<KernelCreatedDocumentSnapshot>;
  readonly entries: () => readonly KernelDocumentEntrySnapshot[];
  readonly delete: (input: {
    readonly deletionPolicy: "recoverable" | "permanent";
    readonly relativePath: KernelWorkspaceRelativePath;
  }) => Promise<undefined>;
  readonly move: (input: {
    readonly name: string;
    readonly relativePath: KernelWorkspaceRelativePath;
    readonly targetParent: KernelWorkspaceRelativePath;
  }) => Promise<KernelDocumentEntrySnapshot>;
  readonly read: (
    relativePath: KernelWorkspaceRelativePath
  ) => Promise<KernelDocumentSnapshot>;
  readonly search: (input: {
    readonly dirtyOverlay?: readonly {
      readonly contents: string;
      readonly relativePath: KernelWorkspaceRelativePath;
    }[];
    readonly query: string;
  }) => Promise<readonly KernelSearchMatchSnapshot[]>;
  readonly update: (input: {
    readonly contents: string;
    readonly relativePath: KernelWorkspaceRelativePath;
  }) => Promise<KernelDocumentSnapshot>;
}

export interface PrimaryWorkspaceDocumentControllerOptions {
  readonly kernel: KernelDomainPort;
  readonly workspace: KernelWorkspaceSnapshot;
}

export async function createPrimaryWorkspaceDocumentController({
  kernel,
  workspace
}: PrimaryWorkspaceDocumentControllerOptions): Promise<PrimaryWorkspaceDocumentController> {
  const entriesByPath = new Map<KernelWorkspaceRelativePath, KernelDocumentEntrySnapshot>();
  const pathsByLocator = new Map<string, KernelWorkspaceRelativePath>();
  const pendingParents: Array<KernelWorkspaceRelativePath | undefined> = [undefined];
  let invalidation: PrimaryWorkspaceDocumentControllerError | undefined;
  const assertActive = () => {
    if (invalidation !== undefined) throw invalidation;
  };
  const assertGeneration = (generation: KernelDocumentEntrySnapshot["workspaceGeneration"]) => {
    assertActive();
    if (generation === workspace.generation) return;
    invalidation ??= new PrimaryWorkspaceDocumentControllerError("workspace-generation-drift");
    throw invalidation;
  };
  const protocolMismatch = (): never => {
    invalidation ??= new PrimaryWorkspaceDocumentControllerError("protocol-mismatch");
    throw invalidation;
  };
  const storeEntry = (
    entry: KernelDocumentEntrySnapshot,
    expected: {
      readonly kind: KernelDocumentEntrySnapshot["kind"];
      readonly locator?: KernelDocumentEntrySnapshot["locator"];
      readonly name: string;
      readonly parent: KernelWorkspaceRelativePath;
      readonly relativePath: KernelWorkspaceRelativePath;
    },
    allowExisting: boolean
  ) => {
    assertGeneration(entry.workspaceGeneration);
    if (
      entry.kind !== expected.kind ||
      entry.name !== expected.name ||
      entry.parent !== expected.parent ||
      entry.relativePath !== expected.relativePath ||
      (expected.locator !== undefined && entry.locator !== expected.locator)
    ) {
      protocolMismatch();
    }
    const existingPath = pathsByLocator.get(entry.locator);
    const existingEntry = entriesByPath.get(entry.relativePath);
    if (
      (existingPath !== undefined && existingPath !== entry.relativePath) ||
      (!allowExisting && existingEntry !== undefined) ||
      (existingEntry !== undefined && existingEntry.locator !== entry.locator)
    ) {
      protocolMismatch();
    }
    entriesByPath.set(entry.relativePath, copyEntry(entry));
    pathsByLocator.set(entry.locator, entry.relativePath);
  };
  const invalidatePath = (relativePath: KernelWorkspaceRelativePath) => {
    const descendantPrefix = `${relativePath}/`;
    for (const [candidatePath, candidate] of entriesByPath) {
      if (candidatePath !== relativePath && !candidatePath.startsWith(descendantPrefix)) continue;
      entriesByPath.delete(candidatePath);
      if (pathsByLocator.get(candidate.locator) === candidatePath) {
        pathsByLocator.delete(candidate.locator);
      }
    }
  };

  while (pendingParents.length > 0) {
    const parent = pendingParents.shift();
    const seenCursors = new Set<KernelPageCursor>();
    let cursor: KernelPageCursor | undefined;

    do {
      const page = await kernel.documents.list({
        ...(cursor === undefined ? {} : { cursor }),
        ...(parent === undefined ? {} : { parent }),
        workspaceGeneration: workspace.generation
      });
      assertGeneration(page.workspaceGeneration);

      for (const entry of page.items) {
        const expectedParent = parent ?? ("" as KernelWorkspaceRelativePath);
        storeEntry(entry, {
          kind: entry.kind,
          name: entry.name,
          parent: expectedParent,
          relativePath: joinWorkspacePath(expectedParent, entry.name)
        }, false);
        if (entry.kind === "directory") {
          pendingParents.push(entry.relativePath);
        }
      }
      if (page.nextCursor !== null) {
        if (seenCursors.has(page.nextCursor)) protocolMismatch();
        seenCursors.add(page.nextCursor);
      }
      cursor = page.nextCursor ?? undefined;
    } while (cursor !== undefined);
  }

  return {
    create: async (input) => {
      assertActive();
      const created = await kernel.documents.create({
        ...input,
        workspaceGeneration: workspace.generation
      });
      storeEntry(created, {
        kind: input.kind,
        name: input.name,
        parent: input.parent,
        relativePath: joinWorkspacePath(input.parent, input.name)
      }, false);
      return { ...created };
    },
    delete: async ({ deletionPolicy, relativePath }) => {
      assertActive();
      const current = requireEntry(entriesByPath, relativePath);
      await kernel.documents.delete({
        deletionPolicy,
        expectedRevision: current.revision,
        locator: current.locator,
        workspaceGeneration: workspace.generation
      });
      invalidatePath(relativePath);
      return undefined;
    },
    entries: () => {
      assertActive();
      return [...entriesByPath.values()]
        .sort((left, right) => left.relativePath.localeCompare(right.relativePath))
        .map(copyEntry);
    },
    move: async ({ name, relativePath, targetParent }) => {
      assertActive();
      const current = requireEntry(entriesByPath, relativePath);
      const moved = await kernel.documents.move({
        expectedRevision: current.revision,
        locator: current.locator,
        name,
        targetParent,
        workspaceGeneration: workspace.generation
      });
      const targetPath = joinWorkspacePath(targetParent, name);
      assertGeneration(moved.workspaceGeneration);
      if (
        moved.kind !== current.kind ||
        moved.locator !== current.locator ||
        moved.name !== name ||
        moved.parent !== targetParent ||
        moved.relativePath !== targetPath ||
        (entriesByPath.has(targetPath) && targetPath !== relativePath) ||
        pathsByLocator.get(moved.locator) !== relativePath
      ) {
        protocolMismatch();
      }
      invalidatePath(relativePath);
      entriesByPath.set(targetPath, copyEntry(moved));
      pathsByLocator.set(moved.locator, targetPath);
      if (current.kind === "directory") {
        invalidation = new PrimaryWorkspaceDocumentControllerError("rebuild-required");
      }
      return { ...moved };
    },
    read: async (relativePath) => {
      assertActive();
      const current = requireEntry(entriesByPath, relativePath);
      const document = await kernel.documents.read({
        locator: current.locator,
        workspaceGeneration: workspace.generation
      });
      storeEntry(document, {
        kind: "file",
        locator: current.locator,
        name: current.name,
        parent: current.parent,
        relativePath
      }, true);
      return { ...document };
    },
    search: async ({ dirtyOverlay = [], query }) => {
      assertActive();
      const matches: KernelSearchMatchSnapshot[] = [];
      const seenCursors = new Set<KernelPageCursor>();
      let cursor: KernelPageCursor | undefined;

      do {
        const page = await kernel.documents.search({
          ...(cursor === undefined ? {} : { cursor }),
          query,
          workspaceGeneration: workspace.generation
        });
        assertGeneration(page.workspaceGeneration);
        for (const match of page.items) {
          const current = entriesByPath.get(match.document.relativePath) ?? protocolMismatch();
          if (
            current.kind !== "file" ||
            match.document.kind !== "file" ||
            !Number.isSafeInteger(match.line) ||
            match.line < 1 ||
            !Number.isSafeInteger(match.column) ||
            match.column < 1
          ) {
            protocolMismatch();
          }
          storeEntry(match.document, {
            kind: current.kind,
            locator: current.locator,
            name: current.name,
            parent: current.parent,
            relativePath: current.relativePath
          }, true);
          matches.push({
            ...match,
            document: { ...match.document }
          });
        }
        if (page.nextCursor !== null) {
          if (seenCursors.has(page.nextCursor)) protocolMismatch();
          seenCursors.add(page.nextCursor);
        }
        cursor = page.nextCursor ?? undefined;
      } while (cursor !== undefined);

      const overlayPaths = new Set(dirtyOverlay.map((overlay) => overlay.relativePath));
      const merged = matches.filter((match) => !overlayPaths.has(match.document.relativePath));
      const needle = query.trim();
      for (const overlay of dirtyOverlay) {
        const current = requireEntry(entriesByPath, overlay.relativePath);
        if (current.kind !== "file") protocolMismatch();
        merged.push(...findDirtyOverlayMatches(current, overlay.contents, needle));
      }
      return merged.sort(compareSearchMatches).map((match) => ({
        ...match,
        document: { ...match.document }
      }));
    },
    update: async ({ contents, relativePath }) => {
      assertActive();
      const current = requireEntry(entriesByPath, relativePath);
      const document = await kernel.documents.update({
        contents,
        expectedRevision: current.revision,
        locator: current.locator,
        workspaceGeneration: workspace.generation
      });
      storeEntry(document, {
        kind: "file",
        locator: current.locator,
        name: current.name,
        parent: current.parent,
        relativePath
      }, true);
      return { ...document };
    }
  };
}

function requireEntry(
  entriesByPath: ReadonlyMap<KernelWorkspaceRelativePath, KernelDocumentEntrySnapshot>,
  relativePath: KernelWorkspaceRelativePath
) {
  const entry = entriesByPath.get(relativePath);
  if (entry === undefined) {
    throw new PrimaryWorkspaceDocumentControllerError("document-not-indexed");
  }
  return entry;
}

function joinWorkspacePath(parent: KernelWorkspaceRelativePath, name: string) {
  return (parent === "" ? name : `${parent}/${name}`) as KernelWorkspaceRelativePath;
}

function copyEntry(entry: KernelDocumentEntrySnapshot): KernelDocumentEntrySnapshot {
  return {
    kind: entry.kind,
    locator: entry.locator,
    modifiedAt: entry.modifiedAt,
    name: entry.name,
    parent: entry.parent,
    relativePath: entry.relativePath,
    revision: entry.revision,
    sizeBytes: entry.sizeBytes,
    workspaceGeneration: entry.workspaceGeneration
  };
}

function findDirtyOverlayMatches(
  document: KernelDocumentEntrySnapshot,
  contents: string,
  needle: string
): KernelSearchMatchSnapshot[] {
  if (needle === "") return [];
  const matches: KernelSearchMatchSnapshot[] = [];
  const lines = contents.split(/\r?\n/u);

  for (const [lineIndex, line] of lines.entries()) {
    let searchStart = 0;
    while (searchStart <= line.length) {
      const matchIndex = line.indexOf(needle, searchStart);
      if (matchIndex < 0) break;
      matches.push({
        column: [...line.slice(0, matchIndex)].length + 1,
        document: { ...document },
        line: lineIndex + 1,
        preview: boundedSearchPreview(line, matchIndex, needle)
      });
      searchStart = matchIndex + needle.length;
    }
  }

  return matches;
}

function boundedSearchPreview(line: string, matchIndex: number, needle: string) {
  const context = 80;
  const matchStart = [...line.slice(0, matchIndex)].length;
  const matchLength = [...needle].length;
  const characters = [...line];
  const start = Math.max(0, matchStart - context);
  const end = Math.min(characters.length, matchStart + matchLength + context);
  return `${start > 0 ? "…" : ""}${characters.slice(start, end).join("")}${end < characters.length ? "…" : ""}`;
}

function compareSearchMatches(left: KernelSearchMatchSnapshot, right: KernelSearchMatchSnapshot) {
  if (left.document.relativePath < right.document.relativePath) return -1;
  if (left.document.relativePath > right.document.relativePath) return 1;
  return left.line - right.line || left.column - right.column;
}
