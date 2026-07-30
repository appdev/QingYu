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
  | "directory-required"
  | "document-not-indexed"
  | "invalid-dirty-overlay"
  | "operation-superseded"
  | "protocol-mismatch"
  | "rebuild-required"
  | "workspace-generation-drift";

const ERROR_MESSAGES: Record<PrimaryWorkspaceDocumentControllerErrorCode, string> = {
  "directory-required": "The primary workspace operation requires an indexed directory.",
  "document-not-indexed": "The primary workspace document is not indexed.",
  "invalid-dirty-overlay": "The primary workspace dirty overlay contains duplicate document paths.",
  "operation-superseded": "The primary workspace document changed while the operation was in flight.",
  "protocol-mismatch": "The primary workspace document response did not match its request.",
  "rebuild-required": "The primary workspace document controller must be rebuilt.",
  "workspace-generation-drift": "The primary workspace changed and the document controller must be rebuilt."
};

const MAX_SEARCH_MATCHES = 10_000;

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

/**
 * This controller remains staged behind the primary UI activation gate until the Kernel contract
 * provides snapshot-consistent multi-page initialization, explicit search case/truncation metadata,
 * event/resource/history integration, and typed reconciliation for uncertain mutation outcomes.
 */
export async function createPrimaryWorkspaceDocumentController({
  kernel,
  workspace
}: PrimaryWorkspaceDocumentControllerOptions): Promise<PrimaryWorkspaceDocumentController> {
  const entriesByPath = new Map<KernelWorkspaceRelativePath, KernelDocumentEntrySnapshot>();
  const pathEpochs = new Map<KernelWorkspaceRelativePath, number>();
  const pathsByLocator = new Map<string, KernelWorkspaceRelativePath>();
  const pendingParents: Array<KernelWorkspaceRelativePath | undefined> = [undefined];
  let invalidation: PrimaryWorkspaceDocumentControllerError | undefined;
  let sidecarEpoch = 0;
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
  const markRebuildRequired = () => {
    invalidation ??= new PrimaryWorkspaceDocumentControllerError("rebuild-required");
  };
  const hasCachedAncestor = (relativePath: KernelWorkspaceRelativePath) => {
    let ancestor = parentWorkspacePath(relativePath);
    while (ancestor !== "") {
      if (entriesByPath.has(ancestor)) return true;
      ancestor = parentWorkspacePath(ancestor);
    }
    return false;
  };
  const pathEpoch = (relativePath: KernelWorkspaceRelativePath) => pathEpochs.get(relativePath) ?? 0;
  const advancePathEpoch = (relativePath: KernelWorkspaceRelativePath) => {
    pathEpochs.set(relativePath, pathEpoch(relativePath) + 1);
    sidecarEpoch += 1;
  };
  const captureEntry = (relativePath: KernelWorkspaceRelativePath) => {
    assertActive();
    const entry = requireEntry(entriesByPath, relativePath);
    if (pathsByLocator.get(entry.locator) !== relativePath) protocolMismatch();
    return {
      entry,
      epoch: pathEpoch(relativePath),
      relativePath
    };
  };
  const captureDirectory = (relativePath: KernelWorkspaceRelativePath) => {
    const captured = captureEntry(relativePath);
    if (captured.entry.kind !== "directory") {
      throw new PrimaryWorkspaceDocumentControllerError("directory-required");
    }
    return captured;
  };
  const capturedEntryIsCurrent = (captured: ReturnType<typeof captureEntry>) => {
    const latest = entriesByPath.get(captured.relativePath);
    return pathEpoch(captured.relativePath) === captured.epoch &&
      latest?.locator === captured.entry.locator &&
      latest.revision === captured.entry.revision &&
      pathsByLocator.get(captured.entry.locator) === captured.relativePath;
  };
  const assertReadStillCurrent = (captured: ReturnType<typeof captureEntry>) => {
    assertActive();
    if (!capturedEntryIsCurrent(captured)) {
      throw new PrimaryWorkspaceDocumentControllerError("operation-superseded");
    }
  };
  const assertMutationStillCurrent = (captured: ReturnType<typeof captureEntry>) => {
    assertActive();
    if (capturedEntryIsCurrent(captured)) return;
    markRebuildRequired();
    throw invalidation;
  };
  const mutationSuperseded = (): never => {
    markRebuildRequired();
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
    const storedEntry = copyEntry(entry);
    if (existingEntry === undefined || !entriesAreEqual(existingEntry, storedEntry)) {
      entriesByPath.set(entry.relativePath, storedEntry);
      advancePathEpoch(entry.relativePath);
    }
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
      advancePathEpoch(candidatePath);
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
      const targetPath = joinWorkspacePath(input.parent, input.name);
      const capturedParent = input.parent === "" ? undefined : captureDirectory(input.parent);
      const capturedTarget = entriesByPath.get(targetPath);
      const capturedTargetEpoch = pathEpoch(targetPath);
      const created = await kernel.documents.create({
        ...input,
        workspaceGeneration: workspace.generation
      });
      assertGeneration(created.workspaceGeneration);
      if (capturedParent !== undefined) assertMutationStillCurrent(capturedParent);
      const latestTarget = entriesByPath.get(targetPath);
      if (
        pathEpoch(targetPath) !== capturedTargetEpoch ||
        latestTarget?.locator !== capturedTarget?.locator ||
        latestTarget?.revision !== capturedTarget?.revision
      ) {
        mutationSuperseded();
      }
      if (input.kind === "file" && (created.kind !== "file" || created.contents !== input.contents)) {
        protocolMismatch();
      }
      storeEntry(created, {
        kind: input.kind,
        name: input.name,
        parent: input.parent,
        relativePath: targetPath
      }, false);
      if (hasCachedAncestor(created.relativePath)) markRebuildRequired();
      return { ...created };
    },
    delete: async ({ deletionPolicy, relativePath }) => {
      const captured = captureEntry(relativePath);
      await kernel.documents.delete({
        deletionPolicy,
        expectedRevision: captured.entry.revision,
        locator: captured.entry.locator,
        workspaceGeneration: workspace.generation
      });
      assertMutationStillCurrent(captured);
      const invalidatedCachedAncestor = hasCachedAncestor(relativePath);
      invalidatePath(relativePath);
      if (invalidatedCachedAncestor) markRebuildRequired();
      return undefined;
    },
    entries: () => {
      assertActive();
      return [...entriesByPath.values()]
        .sort((left, right) => left.relativePath.localeCompare(right.relativePath))
        .map(copyEntry);
    },
    move: async ({ name, relativePath, targetParent }) => {
      const captured = captureEntry(relativePath);
      const targetPath = joinWorkspacePath(targetParent, name);
      const capturedTargetParent = targetParent === "" ? undefined : captureDirectory(targetParent);
      const capturedTarget = entriesByPath.get(targetPath);
      const capturedTargetEpoch = pathEpoch(targetPath);
      const moved = await kernel.documents.move({
        expectedRevision: captured.entry.revision,
        locator: captured.entry.locator,
        name,
        targetParent,
        workspaceGeneration: workspace.generation
      });
      const invalidatedCachedAncestor = hasCachedAncestor(relativePath) || hasCachedAncestor(targetPath);
      assertGeneration(moved.workspaceGeneration);
      assertMutationStillCurrent(captured);
      if (capturedTargetParent !== undefined) {
        assertMutationStillCurrent(capturedTargetParent);
      }
      const latestTarget = entriesByPath.get(targetPath);
      if (
        pathEpoch(targetPath) !== capturedTargetEpoch ||
        latestTarget?.locator !== capturedTarget?.locator ||
        latestTarget?.revision !== capturedTarget?.revision
      ) {
        mutationSuperseded();
      }
      if (
        moved.kind !== captured.entry.kind ||
        moved.name !== name ||
        moved.parent !== targetParent ||
        moved.relativePath !== targetPath ||
        (targetPath !== relativePath && moved.locator === captured.entry.locator) ||
        (entriesByPath.has(targetPath) && targetPath !== relativePath) ||
        pathsByLocator.get(captured.entry.locator) !== relativePath ||
        (pathsByLocator.has(moved.locator) && pathsByLocator.get(moved.locator) !== relativePath)
      ) {
        protocolMismatch();
      }
      invalidatePath(relativePath);
      entriesByPath.set(targetPath, copyEntry(moved));
      pathsByLocator.set(moved.locator, targetPath);
      advancePathEpoch(targetPath);
      if (captured.entry.kind === "directory" || invalidatedCachedAncestor) markRebuildRequired();
      return { ...moved };
    },
    read: async (relativePath) => {
      const captured = captureEntry(relativePath);
      const document = await kernel.documents.read({
        locator: captured.entry.locator,
        workspaceGeneration: workspace.generation
      });
      assertReadStillCurrent(captured);
      storeEntry(document, {
        kind: "file",
        locator: captured.entry.locator,
        name: captured.entry.name,
        parent: captured.entry.parent,
        relativePath
      }, true);
      return { ...document };
    },
    search: async ({ dirtyOverlay = [], query }) => {
      assertActive();
      const overlayPaths = new Set<KernelWorkspaceRelativePath>();
      for (const overlay of dirtyOverlay) {
        if (overlayPaths.has(overlay.relativePath)) {
          throw new PrimaryWorkspaceDocumentControllerError("invalid-dirty-overlay");
        }
        overlayPaths.add(overlay.relativePath);
      }
      const capturedSidecarEpoch = sidecarEpoch;
      const matches: KernelSearchMatchSnapshot[] = [];
      const seenCursors = new Set<KernelPageCursor>();
      let cursor: KernelPageCursor | undefined;

      do {
        const page = await kernel.documents.search({
          ...(cursor === undefined ? {} : { cursor }),
          query,
          workspaceGeneration: workspace.generation
        });
        assertActive();
        if (sidecarEpoch !== capturedSidecarEpoch) {
          throw new PrimaryWorkspaceDocumentControllerError("operation-superseded");
        }
        assertGeneration(page.workspaceGeneration);
        for (const match of page.items) {
          const current = entriesByPath.get(match.document.relativePath) ?? protocolMismatch();
          assertGeneration(match.document.workspaceGeneration);
          if (
            current.kind !== "file" ||
            match.document.kind !== "file" ||
            match.document.locator !== current.locator ||
            match.document.name !== current.name ||
            match.document.parent !== current.parent ||
            match.document.relativePath !== current.relativePath ||
            pathsByLocator.get(current.locator) !== current.relativePath ||
            !Number.isSafeInteger(match.line) ||
            match.line < 1 ||
            !Number.isSafeInteger(match.column) ||
            match.column < 1
          ) {
            protocolMismatch();
          }
          matches.push({
            ...match,
            document: { ...match.document }
          });
          if (matches.length >= MAX_SEARCH_MATCHES) break;
        }
        if (matches.length >= MAX_SEARCH_MATCHES) break;
        if (page.nextCursor !== null) {
          if (seenCursors.has(page.nextCursor)) protocolMismatch();
          seenCursors.add(page.nextCursor);
        }
        cursor = page.nextCursor ?? undefined;
      } while (cursor !== undefined);

      const merged = matches.filter((match) => !overlayPaths.has(match.document.relativePath));
      const needle = query.trim();
      let overlayMatchCount = 0;
      const sortedDirtyOverlay = [...dirtyOverlay].sort((left, right) =>
        left.relativePath.localeCompare(right.relativePath));
      for (const overlay of sortedDirtyOverlay) {
        const current = requireEntry(entriesByPath, overlay.relativePath);
        if (current.kind !== "file") protocolMismatch();
        const overlayMatches = findDirtyOverlayMatches(
          current,
          overlay.contents,
          needle,
          MAX_SEARCH_MATCHES - overlayMatchCount
        );
        overlayMatchCount += overlayMatches.length;
        merged.push(...overlayMatches);
        if (overlayMatchCount >= MAX_SEARCH_MATCHES) break;
      }
      return merged
        .sort(compareSearchMatches)
        .slice(0, MAX_SEARCH_MATCHES)
        .map((match) => ({
          ...match,
          document: { ...match.document }
        }));
    },
    update: async ({ contents, relativePath }) => {
      const captured = captureEntry(relativePath);
      const document = await kernel.documents.update({
        contents,
        expectedRevision: captured.entry.revision,
        locator: captured.entry.locator,
        workspaceGeneration: workspace.generation
      });
      assertGeneration(document.workspaceGeneration);
      assertMutationStillCurrent(captured);
      if (document.contents !== contents) protocolMismatch();
      storeEntry(document, {
        kind: "file",
        locator: captured.entry.locator,
        name: captured.entry.name,
        parent: captured.entry.parent,
        relativePath
      }, true);
      if (hasCachedAncestor(relativePath)) markRebuildRequired();
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

function parentWorkspacePath(relativePath: KernelWorkspaceRelativePath) {
  const separator = relativePath.lastIndexOf("/");
  return (separator < 0 ? "" : relativePath.slice(0, separator)) as KernelWorkspaceRelativePath;
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

function entriesAreEqual(
  left: KernelDocumentEntrySnapshot,
  right: KernelDocumentEntrySnapshot
) {
  return left.kind === right.kind &&
    left.locator === right.locator &&
    left.modifiedAt === right.modifiedAt &&
    left.name === right.name &&
    left.parent === right.parent &&
    left.relativePath === right.relativePath &&
    left.revision === right.revision &&
    left.sizeBytes === right.sizeBytes &&
    left.workspaceGeneration === right.workspaceGeneration;
}

function findDirtyOverlayMatches(
  document: KernelDocumentEntrySnapshot,
  contents: string,
  needle: string,
  limit: number
): KernelSearchMatchSnapshot[] {
  if (needle === "" || limit <= 0) return [];
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
      if (matches.length >= limit) return matches;
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
