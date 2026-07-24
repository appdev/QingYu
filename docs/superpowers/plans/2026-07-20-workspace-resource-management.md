# Workspace Resource Management Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a desktop-only Resources settings page that opens immediately, scans the current workspace without blocking the settings renderer, diagnoses unused and missing managed resources, and moves freshly revalidated unused files to the operating system trash.

**Architecture:** Keep Markdown syntax extraction in `@markra/markdown`, resource resolution/classification in a pure `@markra/app` module, filesystem reads and scan orchestration in a cancellable React hook, and Markdown parsing in a dedicated web worker. Extend settings context with a workspace scan source and invoking window label so the settings window can overlay unsaved documents from the correct editor. Add a narrow Rust command that resolves the canonical scan root and independently revalidates each selected `assets` file before calling system trash.

**Tech Stack:** React 19, TypeScript 6, Vite web workers, unified/remark, Tailwind CSS, Tauri v2, Rust, `trash`, Vitest, Testing Library, pnpm workspace.

## Global Constraints

- Use `pnpm` for all JavaScript and frontend commands.
- Do not use the TypeScript `void` keyword or operator.
- Preserve the existing file-ignore contract by passing `fileIgnoreSettings.rules` into the existing background workspace enumeration.
- Treat only regular files below a real directory component named exactly `assets` as manageable resources. Do not follow or manage symbolic links.
- Ignore remote URLs, data URLs, Markdown-document links, directories, wiki links, and paths outside the canonical workspace root.
- Render the Resources page frame on the first paint. Start enumeration only in an effect after mount; never await inventory, document reads, snapshots, or worker startup before navigation completes.
- Limit concurrent Markdown reads to four and send completed content to the worker in bounded batches. Do not retain all workspace document contents in React state.
- Never expose a final unused result or enable trash actions until every required document has been analyzed successfully and a reliable editor snapshot is available.
- Before trashing, re-request the live editor snapshot and metadata-only Markdown inventory. Any document generation, dirty overlay, path set, size, or modified-time change cancels deletion and starts a new scan.
- Move files only to the operating system trash. Never permanently delete them.
- Preserve existing sync settings behavior by keeping `projectRoot` separate from the new `workspaceSourcePath` settings context.
- Keep web and unsupported runtimes capability-gated; they must not display the Resources category.
- Do not add a persistent resource index, database, preference, watcher, automatic repair, or automatic cleanup on document deletion.
- Preserve the intentional untracked `bg.png` file.

---

### Task 1: Extract local resource references with the established Markdown parser

**Files:**
- Create: `packages/markdown/src/resource-references.test.ts`
- Create: `packages/markdown/src/resource-references.ts`
- Modify: `packages/markdown/src/index.ts`

**Interfaces:**

```ts
export type MarkdownResourceReferenceKind =
  | "image"
  | "attachment"
  | "html-image"
  | "html-attachment";

export type MarkdownResourceReference = {
  columnNumber: number;
  from: number;
  href: string;
  kind: MarkdownResourceReferenceKind;
  lineNumber: number;
  text: string;
  to: number;
};

export function parseMarkdownResourceReferences(
  markdown: string
): MarkdownResourceReference[];
```

- The parser returns syntax occurrences only. It does not resolve paths or touch the filesystem.
- Reuse `unified().use(remarkParse).use(remarkGfm).use(remarkMath)` so editor and resource parsing agree.
- Traverse `image`, `link`, `imageReference`, `linkReference`, `definition`, and raw `html` nodes. Resolve reference-style nodes through a case-insensitive definition map.
- Inspect `src` only inside raw HTML `<img>` nodes and `href` only inside raw HTML `<a>` nodes. An attribute parser may be a small scanner or a regex scoped to the HTML AST node; it must support single-quoted, double-quoted, and unquoted values.
- Use the node or attribute position for `from`, `to`, `lineNumber`, and `columnNumber`. If an HTML attribute position cannot be narrowed safely, use the containing HTML node position.
- Filter syntax-level non-resource destinations before returning: blank hrefs, `http:`, `https:`, every other URI scheme, protocol-relative URLs, `data:`, fragment-only values, wiki syntax, `.md`/`.markdown` destinations after query/fragment removal, and directory destinations ending in `/`.

- [ ] **Step 1: Write the failing syntax matrix tests**

Create table-driven tests covering inline images and links, reference-style images and links, and supported raw HTML:

```ts
it.each([
  ["![Cover](assets/cover.png)", "assets/cover.png", "image"],
  ["[Report](assets/report.pdf)", "assets/report.pdf", "attachment"],
  ["![封面][cover]\n\n[cover]: ./assets/%E5%B0%81%E9%9D%A2.png?raw=1#preview", "./assets/%E5%B0%81%E9%9D%A2.png?raw=1#preview", "image"],
  ["<img alt='Cover' src=\"assets/cover.webp\">", "assets/cover.webp", "html-image"],
  ["<a href='assets/archive.zip'>Archive</a>", "assets/archive.zip", "html-attachment"]
])("extracts %s", (markdown, href, kind) => {
  expect(parseMarkdownResourceReferences(markdown)).toEqual([
    expect.objectContaining({ href, kind })
  ]);
});
```

Add assertions that offsets slice a source span containing the destination and that line/column values are 1-based.

- [ ] **Step 2: Write failing exclusion tests**

Assert that the following return no occurrences:

```ts
const ignored = [
  "`![code](assets/code.png)`",
  "```md\n![code](assets/code.png)\n```",
  "plain assets/plain.png text",
  "![remote](https://example.com/image.png)",
  "![embedded](data:image/png;base64,AAAA)",
  "[[assets/wiki.png]]",
  "[Document](notes/other.md)",
  "[Folder](assets/folder/)",
  "[Anchor](#section)"
];

for (const markdown of ignored) {
  expect(parseMarkdownResourceReferences(markdown)).toEqual([]);
}
```

- [ ] **Step 3: Run the focused test and confirm it fails**

Run:

```bash
pnpm --filter @markra/markdown test -- src/resource-references.test.ts
```

Expected: FAIL because the module and export do not exist.

- [ ] **Step 4: Implement the AST extractor**

Implement a local typed AST shape with `children`, `identifier`, `label`, `position`, `title`, `type`, `url`, and `value`. Build definitions in a first traversal, then collect references in source order in a second traversal. Normalize only for filtering; preserve the original destination in `href`.

Use these exact helpers and semantics:

```ts
const markdownDocumentPattern = /\.(?:md|markdown)(?:$|[?#])/iu;
const uriSchemePattern = /^[a-z][a-z0-9+.-]*:/iu;

function isCandidateResourceHref(href: string) {
  const value = href.trim();
  if (!value || value.startsWith("#") || value.startsWith("//")) return false;
  if (uriSchemePattern.test(value)) return false;
  if (value.endsWith("/") || markdownDocumentPattern.test(value)) return false;
  return !/^\[\[.*\]\]$/u.test(value);
}
```

For link/image nodes, use `position.start.offset ?? 0` and `position.end.offset ?? from`; use `position.start.line ?? 1` and `position.start.column ?? 1`. For reference nodes, preserve the reference node position but use the definition URL. Sort by `from`, then `to` before returning.

- [ ] **Step 5: Export and verify the parser**

Add `export * from "./resource-references.ts";` to `packages/markdown/src/index.ts`, rerun the focused test, and run:

```bash
pnpm --filter @markra/markdown typecheck:test
```

Expected: PASS.

- [ ] **Step 6: Commit the parser**

```bash
git add packages/markdown/src/resource-references.ts packages/markdown/src/resource-references.test.ts packages/markdown/src/index.ts
git commit -m "feat: extract workspace resource references"
```

---

### Task 2: Build the pure workspace resource graph

**Files:**
- Create: `packages/app/src/lib/workspace-resources.test.ts`
- Create: `packages/app/src/lib/workspace-resources.ts`

**Interfaces:**

```ts
import type { MarkdownResourceReference } from "@markra/markdown";
import type { NativeMarkdownFolderFile } from "./tauri/file";

export type WorkspaceMarkdownFile = NativeMarkdownFolderFile & { kind?: undefined };
export type WorkspaceResourceFile = NativeMarkdownFolderFile & {
  kind: "asset" | "attachment";
  modifiedAt: number;
  sizeBytes: number;
};
export type WorkspaceResourceOccurrence = MarkdownResourceReference & {
  sourceFile: WorkspaceMarkdownFile;
};
export type WorkspaceMissingResource = {
  href: string;
  occurrences: WorkspaceResourceOccurrence[];
  relativePath: string;
};
export type WorkspaceExistingResource = WorkspaceResourceFile & {
  referenceCount: number;
};
export type WorkspaceResourceFailure = {
  message: string;
  path: string;
  stage: "read" | "parse";
};
export type WorkspaceResourceGraph = {
  complete: boolean;
  existing: WorkspaceExistingResource[];
  failures: WorkspaceResourceFailure[];
  missing: WorkspaceMissingResource[];
  unused: WorkspaceExistingResource[];
};

export function isManagedResourceRelativePath(relativePath: string): boolean;
export function buildWorkspaceResourceGraph(input: {
  complete: boolean;
  failures: readonly WorkspaceResourceFailure[];
  markdownFiles: readonly WorkspaceMarkdownFile[];
  occurrences: ReadonlyMap<string, readonly MarkdownResourceReference[]>;
  resources: readonly WorkspaceResourceFile[];
  workspaceRoot: string;
}): WorkspaceResourceGraph;
```

- Use normalized slash-separated workspace-relative paths as graph keys. Preserve native absolute paths from inventory for actions and display.
- Decode percent escapes with a guarded `decodeURIComponent`, strip query/fragment, normalize `.` and `..`, and reject paths that escape the root.
- Resolve relative destinations from the source document's relative parent. Resolve Unix, drive-letter, and UNC absolute destinations only when their normalized absolute identity is within `workspaceRoot`.
- A manageable path has an exact `assets` directory component before the final file name. `my-assets`, `assets.txt`, and the directory `assets` alone are not manageable.
- Match Windows identities case-insensitively and POSIX identities case-sensitively. Never key by basename.
- `unused` must be `[]` unless `complete === true` and `failures.length === 0`.

- [ ] **Step 1: Write failing graph classification tests**

Cover these fixtures:

```ts
const resources = [
  resource("assets/logo.png"),
  resource("docs/assets/logo.png"),
  resource("docs/assets/manual.pdf"),
  resource("my-assets/ignored.png")
];
```

Assert:

- `assets/logo.png` and `docs/assets/logo.png` remain separate identities.
- references from `index.md` and `docs/guide.md` resolve relative to their own directories;
- query strings, fragments, spaces, Unicode, and percent escapes match inventory;
- multiple documents referring to the same missing target produce one missing row with multiple occurrences;
- external `../` escapes, remote targets, Markdown documents, and `my-assets` targets are ignored;
- incomplete scans retain missing diagnostics but return no unused resources;
- a successful complete scan returns unreferenced inventory files and aggregate reference counts.

- [ ] **Step 2: Run the graph test and confirm it fails**

```bash
pnpm --filter @markra/app test -- src/lib/workspace-resources.test.ts
```

Expected: FAIL because the graph module does not exist.

- [ ] **Step 3: Implement deterministic path resolution and graph construction**

Implement these exact invariants:

```ts
export function isManagedResourceRelativePath(relativePath: string) {
  const parts = normalizeRelativePath(relativePath)?.split("/") ?? [];
  return parts.length > 1 && parts.slice(0, -1).includes("assets");
}

const unused = complete && failures.length === 0
  ? existing.filter((resource) => resource.referenceCount === 0)
  : [];
```

Create one inventory map keyed by normalized identity, one missing map keyed by normalized relative target, and one occurrence list per source path. Sort existing/unused by `relativePath` with numeric, base-sensitive comparison; sort missing by `relativePath`; sort each occurrence list by source path, line, then column. Do not access `window`, native APIs, or React.

- [ ] **Step 4: Verify the graph module**

Run:

```bash
pnpm --filter @markra/app test -- src/lib/workspace-resources.test.ts
pnpm --filter @markra/app typecheck:test
```

Expected: PASS.

- [ ] **Step 5: Commit the graph**

```bash
git add packages/app/src/lib/workspace-resources.ts packages/app/src/lib/workspace-resources.test.ts
git commit -m "feat: classify workspace resource health"
```

---

### Task 3: Add a narrow native resource root and system-trash boundary

**Files:**
- Create: `apps/desktop/src-tauri/src/markdown_files/resource.rs`
- Modify: `apps/desktop/src-tauri/src/markdown_files.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `packages/app/src/lib/tauri/file.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `apps/desktop/src/runtime/tauri/file.ts`
- Modify: `apps/desktop/src/runtime/tauri/file.test.ts`
- Modify: `apps/desktop/src/runtime/index.ts`
- Modify: `apps/web/src/runtime/index.ts`

**Interfaces:**

```ts
export type TrashWorkspaceResourceInput = {
  modifiedAt: number;
  relativePath: string;
  sizeBytes: number;
};

export type TrashWorkspaceResourceResult = {
  error?: string;
  relativePath: string;
  status: "trashed" | "failed";
};

// AppFileRuntime additions
confirmWorkspaceResourceTrash(
  labels: { cancelLabel: string; message: string; okLabel: string }
): Promise<boolean>;
resolveWorkspaceResourceRoot(sourcePath: string): Promise<string>;
trashWorkspaceResources(
  rootPath: string,
  resources: readonly TrashWorkspaceResourceInput[]
): Promise<TrashWorkspaceResourceResult[]>;
```

Rust request/response payloads use `#[serde(rename_all = "camelCase")]` and the same field names. Register commands:

```rust
resolve_workspace_resource_root(source_path: String) -> Result<String, String>
trash_workspace_resources(
    root_path: String,
    resources: Vec<TrashWorkspaceResourceInput>,
) -> Vec<TrashWorkspaceResourceResult>
```

- `resolve_workspace_resource_root` must reuse `markdown_tree_root_for_path`, canonicalize the returned directory, authorize it through the existing asset-scope path, and return the canonical native path.
- The trash command accepts only relative paths. Reject absolute components, `..`, empty paths, non-UTF-8 ambiguity, paths without an exact parent component named `assets`, directories, missing files, and symbolic links anywhere between root and file.
- Canonicalize the root once, then open it with `cap_std::fs::Dir::open_ambient_dir`. Walk every parent with `cap_fs_ext::DirExt::open_dir_nofollow` and open the leaf with `OpenOptionsFollowExt::follow(FollowSymlinks::No)`; each parent must be a real directory and the leaf a regular file. Keep the leaf handle open, compare its metadata with the requested metadata, then repeat the no-follow lookup and compare file identity immediately before trashing. Reject the item if the directory entry was replaced between those checks.
- Compare `metadata.len()` and modified time in the same milliseconds representation already used by `markdown_folder_file`.
- Validate every batch item independently, then call an injected `SystemTrash = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>` only for valid items. The production constructor uses `trash::delete`.
- Do not reuse the arbitrary tree delete command and do not expose a generic absolute-path trash method.

- [ ] **Step 1: Write failing Rust safety tests**

Inside `resource.rs`, create temp fixtures and an injected trash recorder. Add tests for:

- a valid root `assets/cover.png` and nested `docs/assets/manual.pdf`;
- traversal and absolute replacement;
- a path outside `assets`;
- a directory and missing file;
- symlinked `assets`, intermediate directory, and leaf where the platform supports symlinks;
- modified size and modified time;
- an injected pre-trash hook that replaces the validated destination, proving the second no-follow identity check rejects the race;
- an independently mixed batch with one success and several failures;
- a root source passed as a Markdown file resolves to its parent directory.

The success test must assert that the mock received the canonical file path and the real fixture still exists because the mock does not delete it.

- [ ] **Step 2: Run the focused Rust tests and confirm they fail**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml markdown_files::resource
```

Expected: FAIL because the module and commands do not exist.

- [ ] **Step 3: Implement and register the Rust service**

Use a private `WorkspaceResourceService` with `Default` for production, `with_system_trash` under tests, and a test-only pre-trash hook for replacement-race fixtures. Reuse the repository's established `cap_std`/`cap_fs_ext` no-follow patterns from `markdown_files/attachment.rs`. Compare stable native file identity where the platform exposes it and always repeat no-follow type/size/modified-time checks immediately before calling trash. Convert all validation errors into per-file `{ status: Failed, error: Some(...) }` values; a bad item must not abort the remaining batch. Export both commands from `markdown_files.rs`, import them into `lib.rs`, and add them to `tauri::generate_handler!`.

- [ ] **Step 4: Write failing desktop runtime mapping tests**

In `apps/desktop/src/runtime/tauri/file.test.ts`, mock `invokeNative` and the existing dialog layer. Assert these exact native calls:

```ts
expect(invokeNative).toHaveBeenCalledWith("resolve_workspace_resource_root", {
  sourcePath: "/vault/note.md"
});
expect(invokeNative).toHaveBeenCalledWith("trash_workspace_resources", {
  resources: [{ modifiedAt: 100, relativePath: "assets/unused.png", sizeBytes: 42 }],
  rootPath: "/vault"
});
```

Also assert response normalization drops malformed native rows rather than treating them as successes.

- [ ] **Step 5: Add the runtime methods and capability**

- Add `resources: boolean` to `AppFeatureRuntime`.
- Set `resources: false` in `createDefaultAppRuntime()` and `createWebRuntime()`.
- Set `resources: true` in `desktopRuntime`.
- Default file-runtime methods reject with the existing unsupported-runtime error style; they must not silently succeed.
- Map desktop methods to `invokeNative` and the native confirmation dialog. The confirmation helper receives already translated labels and does not own product copy.

- [ ] **Step 6: Verify native and runtime layers**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml markdown_files::resource
pnpm --filter @markra/desktop test -- src/runtime/tauri/file.test.ts
pnpm --filter @markra/app typecheck:test
```

Expected: PASS.

- [ ] **Step 7: Commit the native boundary**

```bash
git add apps/desktop/src-tauri/src/markdown_files/resource.rs apps/desktop/src-tauri/src/markdown_files.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src/runtime/index.ts apps/desktop/src/runtime/tauri/file.ts apps/desktop/src/runtime/tauri/file.test.ts apps/web/src/runtime/index.ts packages/app/src/lib/tauri/file.ts packages/app/src/runtime/index.ts
git commit -m "feat: safely trash managed workspace resources"
```

---

### Task 4: Carry workspace and invoking-window context into settings

**Files:**
- Modify: `apps/desktop/src-tauri/src/windows.rs`
- Modify: `apps/desktop/src/runtime/tauri/window.ts`
- Modify: `apps/desktop/src/runtime/tauri/window.test.ts`
- Modify: `apps/desktop/src/runtime/index.ts`
- Modify: `packages/app/src/runtime/index.ts`
- Modify: `packages/app/src/lib/tauri/window.ts`
- Modify: `packages/app/src/hooks/useSettingsWindowState.ts`
- Modify: `packages/app/src/hooks/useSettingsWindowState.test.ts`
- Modify: `packages/app/src/App.tsx`
- Modify: `packages/app/src/App.test.tsx`

**Interfaces:**

```ts
export type NativeSettingsWindowContext = {
  projectRoot: string | null;
  sourceWindowLabel: string | null;
  workspaceSourcePath: string | null;
};

openSettingsWindow(
  target?: NativeSettingsWindowTarget,
  projectRoot?: string | null,
  workspaceSourcePath?: string | null
): Promise<unknown>;

listenSettingsWindowTarget(
  onTarget: (target: NativeSettingsWindowTarget) => unknown,
  onContext?: (context: NativeSettingsWindowContext) => unknown
): Promise<RuntimeCleanup>;
```

- The Tauri `open_settings_window` command receives the invoking `tauri::WebviewWindow` implicitly and captures `window.label()` as `sourceWindowLabel`; JavaScript must never supply or spoof it.
- Add URL parameters `settingsWorkspaceContext=1`, `settingsWorkspaceSourcePath`, and `settingsSourceWindowLabel` for a newly created visible settings window. Add the same fields to `SettingsWindowTargetPayload` for a reused/prewarmed window.
- Extend `SettingsWindowRuntimeState` with pending workspace source and source label. Clear/take them everywhere pending target/project root are currently cleared/taken.
- Keep `projectRoot` unchanged for sync. `workspaceSourcePath` is the file-tree source path: a folder for folder workspaces or a Markdown file for standalone-file workspaces.
- `useSettingsWindowState` exposes `settingsWorkspaceSourcePath`, `settingsWorkspaceContextResolved`, and `settingsSourceWindowLabel`. A new context event replaces all three atomically.
- Workspace `App.tsx` calls:

```ts
openSettingsWindow(
  undefined,
  blankWorkspace ? null : fileTree.settingsProjectRoot,
  blankWorkspace ? null : fileTreeSourcePath
).catch(() => {});
```

- [ ] **Step 1: Write failing native window-state tests**

Extend existing `windows.rs` tests to assert:

- the settings URL percent-encodes both project and workspace paths and includes the invoking label;
- an existing settings window receives all three context fields;
- a prewarmed window takes the newest pending source label/workspace source;
- hide, destroy, canceled creation, and idle destroy clear the added pending fields;
- sync project root is still independent from a standalone workspace source.

- [ ] **Step 2: Write failing TypeScript context tests**

Extend `window.test.ts` and `useSettingsWindowState.test.ts` so the initial URL and later event both produce:

```ts
expect(result.current.settingsWorkspaceSourcePath).toBe("/vault/note.md");
expect(result.current.settingsSourceWindowLabel).toBe("markra-editor-2");
```

Then publish a second context and assert the old source and label are replaced together.

- [ ] **Step 3: Run focused context tests and confirm they fail**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml windows::tests
pnpm --filter @markra/desktop test -- src/runtime/tauri/window.test.ts
pnpm --filter @markra/app test -- src/hooks/useSettingsWindowState.test.ts
```

Expected: FAIL because workspace source and invoking label are not propagated.

- [ ] **Step 4: Implement the context extension end to end**

Use one context object callback rather than independent callbacks so a reused settings window cannot briefly combine an old source label with a new workspace. In `useSettingsWindowState`, maintain a single state object and derive the three exposed fields. Preserve the existing sync session's project-root transition and `prepareForProjectRootChange()` sequence.

- [ ] **Step 5: Verify settings open calls and context changes**

Add an `App.test.tsx` assertion that opening Settings passes both the sync project root and the current file-tree source. Cover a standalone Markdown file with `projectRoot === null` and `workspaceSourcePath === "/notes/standalone.md"`.

Run:

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml windows::tests
pnpm --filter @markra/desktop test -- src/runtime/tauri/window.test.ts
pnpm --filter @markra/app test -- src/hooks/useSettingsWindowState.test.ts src/App.test.tsx
```

Expected: PASS.

- [ ] **Step 6: Commit the settings context**

```bash
git add apps/desktop/src-tauri/src/windows.rs apps/desktop/src/runtime/tauri/window.ts apps/desktop/src/runtime/tauri/window.test.ts apps/desktop/src/runtime/index.ts packages/app/src/runtime/index.ts packages/app/src/lib/tauri/window.ts packages/app/src/hooks/useSettingsWindowState.ts packages/app/src/hooks/useSettingsWindowState.test.ts packages/app/src/App.tsx packages/app/src/App.test.tsx
git commit -m "feat: pass workspace context to resource settings"
```

---

### Task 5: Overlay unsaved documents from the correct editor window

**Files:**
- Create: `packages/app/src/lib/workspace-resource-snapshots.test.ts`
- Create: `packages/app/src/lib/workspace-resource-snapshots.ts`
- Create: `packages/app/src/hooks/useWorkspaceResourceSnapshotResponder.test.tsx`
- Create: `packages/app/src/hooks/useWorkspaceResourceSnapshotResponder.ts`
- Modify: `packages/app/src/App.tsx`

**Interfaces:**

```ts
export const workspaceResourceSnapshotRequestEvent =
  "qingyu://workspace-resource-snapshot-request";
export const workspaceResourceSnapshotResponseEvent =
  "qingyu://workspace-resource-snapshot-response";

export type WorkspaceResourceSnapshotRequest = {
  requestId: string;
  sourceWindowLabel: string;
  workspaceSourcePath: string;
};

export type WorkspaceResourceDirtyDocument = {
  content: string;
  path: string;
  revision: number;
};

export type WorkspaceResourceSnapshotResponse = {
  documentGeneration: number;
  dirtyDocuments: WorkspaceResourceDirtyDocument[];
  requestId: string;
  sourceWindowLabel: string;
  workspaceSourcePath: string;
};

export function requestWorkspaceResourceSnapshot(input: {
  events: AppEventsRuntime;
  sourceWindowLabel: string;
  timeoutMs?: number;
  workspaceSourcePath: string;
}): Promise<WorkspaceResourceSnapshotResponse>;
```

- Register the response listener before emitting the request.
- Default timeout is 1,500 ms. Always remove the listener and timeout after success, timeout, malformed response, or cancellation.
- The responder listens only in editor windows. It obtains its own runtime window label and responds only when both source label and workspace source match.
- Include only open, dirty, non-deleted, saved document tabs (`path !== null`). Sort by path for stable freshness comparison.
- Maintain a monotonic `documentGeneration` in a ref. Increment when the relevant workspace source changes or any saved tab's path, dirty flag, deleted flag, revision, or content changes. Do not use timestamps.
- [ ] **Step 1: Write failing request protocol tests**

Use an in-memory fake `AppEventsRuntime`. Assert listener-before-emit ordering, correlation by `requestId`, source/workspace filtering, malformed payload rejection, timeout cleanup, and no cross-window response acceptance.

- [ ] **Step 2: Write failing responder tests**

Render the hook with two tabs: one dirty saved file and one untitled draft. Publish a matching request and assert only the saved dirty file is returned. Rerender with a revision/content change and assert the next response has a larger generation. Publish a mismatched label and assert no response.

- [ ] **Step 3: Run the snapshot tests and confirm they fail**

```bash
pnpm --filter @markra/app test -- src/lib/workspace-resource-snapshots.test.ts src/hooks/useWorkspaceResourceSnapshotResponder.test.tsx
```

Expected: FAIL because the protocol and responder do not exist.

- [ ] **Step 4: Implement the request helper and responder hook**

The request helper must reject with typed error codes `unavailable`, `timeout`, or `invalid-response` so the coordinator can distinguish read-only fallback from top-level failure. The responder emits a plain serializable payload through `getAppRuntime().events`.

Call the hook in `WorkspaceApp` after `documentTabs` and `fileTreeSourcePath` are available:

```ts
useWorkspaceResourceSnapshotResponder({
  documentTabs,
  workspaceSourcePath: blankWorkspace ? null : fileTreeSourcePath
});
```

- [ ] **Step 5: Verify and commit the snapshot protocol**

```bash
pnpm --filter @markra/app test -- src/lib/workspace-resource-snapshots.test.ts src/hooks/useWorkspaceResourceSnapshotResponder.test.tsx src/App.test.tsx
pnpm --filter @markra/app typecheck:test
git add packages/app/src/lib/workspace-resource-snapshots.ts packages/app/src/lib/workspace-resource-snapshots.test.ts packages/app/src/hooks/useWorkspaceResourceSnapshotResponder.ts packages/app/src/hooks/useWorkspaceResourceSnapshotResponder.test.tsx packages/app/src/App.tsx
git commit -m "feat: expose live document snapshots to settings"
```

Expected: tests and typecheck PASS before commit.

---

### Task 6: Parse incrementally in a worker and coordinate cancellable scans

**Files:**
- Create: `packages/app/src/lib/workspace-resource-worker.test.ts`
- Create: `packages/app/src/lib/workspace-resource-worker.ts`
- Create: `packages/app/src/workers/workspace-resource-scan.worker.ts`
- Create: `packages/app/src/hooks/useWorkspaceResources.test.tsx`
- Create: `packages/app/src/hooks/useWorkspaceResources.ts`

**Interfaces:**

```ts
export type WorkspaceResourceWorkerRequest = {
  documents: Array<{ content: string; path: string }>;
  scanId: number;
  type: "analyze";
};

export type WorkspaceResourceWorkerResponse =
  | {
      occurrences: Array<{
        path: string;
        references: MarkdownResourceReference[];
      }>;
      scanId: number;
      type: "analyzed";
    }
  | {
      error: string;
      path: string;
      scanId: number;
      type: "failed";
    };

export type WorkspaceResourceScanPhase =
  | "inventory"
  | "reading"
  | "analyzing"
  | "finalizing";

export type WorkspaceResourceScanState = {
  canTrash: boolean;
  graph: WorkspaceResourceGraph | null;
  progress: { completed: number; phase: WorkspaceResourceScanPhase; total: number };
  snapshotGeneration: number | null;
  status: "idle" | "scanning" | "ready" | "incomplete" | "error";
  warning: "snapshot-unavailable" | null;
};

export function useWorkspaceResources(input: {
  active: boolean;
  globalIgnoreRules: string;
  sourceWindowLabel: string | null;
  workspaceSourcePath: string | null;
  workerFactory?: () => Worker;
}): WorkspaceResourceScanState & {
  refresh: () => unknown;
};
```

- `workspace-resource-worker.ts` owns message guards and the default worker factory using `new Worker(new URL("../workers/workspace-resource-scan.worker.ts", import.meta.url), { type: "module" })`.
- The worker calls `parseMarkdownResourceReferences` for each document and posts results per bounded batch. It catches per-document errors and continues.
- The coordinator owns a monotonically increasing scan ID, an `AbortController`, and one worker instance per scan. Replacement scans abort enumeration, terminate the worker, clear old actionable state immediately, and ignore every late result.
- Inventory uses `resolveWorkspaceResourceRoot(workspaceSourcePath)` then `loadMarkdownFilesForPath(root, { globalIgnoreRules, onBatch, signal })` with no `managedAttachmentFolder` so generic attachments are included. Filter Markdown documents by `kind === undefined` and resources by `kind === "asset" || kind === "attachment"`, then retain only exact managed `assets` paths.
- Request the live snapshot in parallel with inventory. Disk-read every Markdown file except a matching dirty overlay. Use the overlay content in its place.
- Implement a four-worker promise queue for reads. Accumulate at most four completed documents per worker message and clear their content references immediately after `postMessage`.
- The initial state and the first active render are `scanning` with phase `inventory`; no inventory promise is awaited in render or in a category-change handler.
- Publish provisional missing diagnostics after analyzed batches. Keep `unused: []` and `canTrash: false` until inventory, all reads, and all worker replies complete without failures and the snapshot is reliable.

- [ ] **Step 1: Write failing worker protocol tests**

Test the message guards and a worker handler extracted as a pure `analyzeWorkspaceResourceBatch` function. Assert one malformed document produces `failed` while valid siblings still produce `analyzed`, and that returned content does not echo Markdown bodies.

- [ ] **Step 2: Write failing coordinator lifecycle tests**

With a deferred fake inventory, fake file reads, fake snapshot request, and fake worker, assert:

- `renderHook` returns the scanning shell state immediately before inventory resolves;
- inventory progress arrives through `onBatch`;
- no more than four `readMarkdownFile` promises are simultaneously pending;
- a dirty overlay replaces the disk read for its path;
- provisional missing results can appear while `canTrash` remains false;
- a complete scan becomes `ready` and exposes final unused resources;
- read/parse failure becomes `incomplete`, retains missing diagnostics, and never exposes unused resources;
- snapshot timeout permits diagnostics but sets `warning: "snapshot-unavailable"` and `canTrash: false`;
- worker error becomes retryable `error` and leaves `canTrash: false`;
- refresh, deactivation, workspace replacement, and unmount terminate the worker and ignore late results.

- [ ] **Step 3: Run worker/coordinator tests and confirm they fail**

```bash
pnpm --filter @markra/app test -- src/lib/workspace-resource-worker.test.ts src/hooks/useWorkspaceResources.test.tsx
```

Expected: FAIL because the worker protocol and hook do not exist.

- [ ] **Step 4: Implement the worker and scan coordinator**

Keep large content out of reducer state. The reducer/state may retain inventories, occurrences, failure metadata, and counts, but not full disk document bodies. Batch posting must transfer ordinary structured-clone strings and release the array after posting.

Use this concurrency skeleton, preserving the limit of four:

```ts
const pending = new Set<Promise<unknown>>();
for (const file of markdownFiles) {
  const task = analyzeFile(file).finally(() => pending.delete(task));
  pending.add(task);
  if (pending.size >= 4) await Promise.race(pending);
}
await Promise.all(pending);
```

Do not call `.finally` without handling its returned promise. The concrete implementation must attach errors inside `analyzeFile` and await every queued task.

- [ ] **Step 5: Verify and commit scanning**

```bash
pnpm --filter @markra/app test -- src/lib/workspace-resource-worker.test.ts src/hooks/useWorkspaceResources.test.tsx src/lib/workspace-resources.test.ts
pnpm --filter @markra/app typecheck:test
git add packages/app/src/lib/workspace-resource-worker.ts packages/app/src/lib/workspace-resource-worker.test.ts packages/app/src/workers/workspace-resource-scan.worker.ts packages/app/src/hooks/useWorkspaceResources.ts packages/app/src/hooks/useWorkspaceResources.test.tsx
git commit -m "feat: scan workspace resources incrementally"
```

Expected: PASS.

---

### Task 7: Add freshness-gated bulk trashing to the coordinator

**Files:**
- Modify: `packages/app/src/hooks/useWorkspaceResources.test.tsx`
- Modify: `packages/app/src/hooks/useWorkspaceResources.ts`
- Modify: `packages/app/src/lib/workspace-resource-snapshots.ts`

**Interfaces:**

Extend the hook result:

```ts
type TrashSelectionResult =
  | { kind: "canceled" }
  | { kind: "stale" }
  | {
      failed: TrashWorkspaceResourceResult[];
      kind: "completed";
      trashed: TrashWorkspaceResourceResult[];
    };

trashResources(
  resources: readonly WorkspaceExistingResource[],
  labels: { cancelLabel: string; message: string; okLabel: string }
): Promise<TrashSelectionResult>;
```

Store a compact freshness token on completed scans:

```ts
type WorkspaceResourceFreshness = {
  dirtyDocuments: Array<{ content: string; path: string; revision: number }>;
  documentGeneration: number;
  markdownFiles: Array<{ modifiedAt: number | null; path: string; sizeBytes: number | null }>;
  workspaceRoot: string;
  workspaceSourcePath: string;
};
```

- Compare arrays after stable path sorting. Exact content comparison is required for dirty overlays; a matching generation alone is insufficient.
- After the user confirms, request a new snapshot and call `listMarkdownFilesForPath(workspaceRoot, { globalIgnoreRules })` for metadata. Do not use cached file-tree rows.
- If any freshness field differs or either refresh fails, call `refresh()`, return `{ kind: "stale" }`, and invoke no trash command.
- Send only files that are still present in the completed unused graph with the exact scanned `relativePath`, `sizeBytes`, and `modifiedAt`.
- After a native batch result, retain failed rows in the result, start a full refresh, and return independent success/failure arrays.

- [ ] **Step 1: Add failing deletion freshness tests**

Cover confirmation cancellation, changed generation, changed dirty content with the same generation, added/removed Markdown path, changed size, changed modified time, metadata read failure, native partial failure, and success. For every stale case assert `trashWorkspaceResources` was not called and `refresh` started.

- [ ] **Step 2: Run the coordinator test and confirm it fails**

```bash
pnpm --filter @markra/app test -- src/hooks/useWorkspaceResources.test.tsx
```

Expected: FAIL because the hook has no trash flow.

- [ ] **Step 3: Implement freshness and trash result handling**

Create pure `workspaceResourceFreshnessMatches(left, right)` and unit-test it in the hook test file or a focused lib test. Freeze trash eligibility while confirmation/freshness/native calls are running so double activation cannot enqueue duplicate batches.

- [ ] **Step 4: Verify and commit the safe deletion flow**

```bash
pnpm --filter @markra/app test -- src/hooks/useWorkspaceResources.test.tsx
pnpm --filter @markra/app typecheck:test
git add packages/app/src/hooks/useWorkspaceResources.ts packages/app/src/hooks/useWorkspaceResources.test.tsx packages/app/src/lib/workspace-resource-snapshots.ts
git commit -m "feat: revalidate resources before trashing"
```

Expected: PASS.

---

### Task 8: Build the immediate-loading Resources page

**Files:**
- Create: `packages/app/src/components/settings/ResourcesSettings.test.tsx`
- Create: `packages/app/src/components/settings/ResourcesSettings.tsx`
- Create: `packages/app/src/components/settings/resources/ResourcePreview.tsx`
- Create: `packages/app/src/components/settings/resources/ResourceResultsList.tsx`
- Modify: `packages/app/src/components/SettingsSections.tsx`

**Props:**

```ts
export type ResourcesSettingsProps = {
  active: boolean;
  globalIgnoreRules: string;
  sourceWindowLabel: string | null;
  translate: (key: I18nKey) => string;
  workspaceSourcePath: string | null;
};
```

- The component calls `useWorkspaceResources`; presentational children receive data/actions through props and do not invoke filesystem APIs.
- Use tabs with `role="tablist"`, `role="tab"`, `aria-selected`, and keyboard-safe native buttons.
- Tab IDs are `unused` and `missing`. Do not add an unused-databases tab.
- While scanning, render the page layout immediately: tab bar, toolbar, bounded list with skeleton/checking rows, preview panel, Refresh, and a polite progress live region. Do not cover the settings page with a blocking modal or spinner.
- Missing rows can update provisionally. Unused rows appear only after final completion.
- Unused list rows use accessible checkboxes and keyboard selection. `Select all` selects final unused rows only. Selection is pruned on rescan/result replacement.
- Move to Trash is disabled unless `canTrash`, one or more rows are selected, and no trash operation is active. Confirmation copy includes selected count and formatted total size.
- A single existing selection enables Show in Folder via `openContainingFolder(resource.path)`.
- Missing detail lists target path and all occurrences. Activating an occurrence calls `openMarkdownFileInNewWindow(occurrence.sourceFile.path)` and shows a toast on failure.
- Image extensions are `avif`, `bmp`, `gif`, `jpeg`, `jpg`, `png`, `svg`, and `webp`. Render `<img src={localFileUrlFromPath(resource.path)} alt={resource.name}>` with aspect-fit. Non-images show name, relative path, absolute path, extension/type, size, and modified time.
- Preview failure changes only the preview to `Preview unavailable`; it must not invalidate the scan or selection.
- Completed empty, no-workspace, unsupported snapshot, incomplete scan, retryable error, stale-before-delete, and partial trash failure all need distinct inline states.

- [ ] **Step 1: Write failing immediate-render and state tests**

Mock `useWorkspaceResources` and assert:

- the heading, both tabs, Refresh, result region, preview region, and `Collecting workspace files` status render during the first scanning state;
- no blocking dialog or full-page replacement hides the settings shell;
- provisional missing rows show `Results are still updating`;
- incomplete state retains missing rows and disables Move to Trash;
- completed empty tabs show their own empty copy;
- no workspace shows explanatory copy and performs no scan action.

- [ ] **Step 2: Write failing interaction and preview tests**

Assert selection, Select all, total-size confirmation labels, trash result summary, failed-row reselection, refresh, show-in-folder, image alt text, attachment metadata, missing occurrence line numbers, and open-document errors.

- [ ] **Step 3: Run component tests and confirm they fail**

```bash
pnpm --filter @markra/app test -- src/components/settings/ResourcesSettings.test.tsx
```

Expected: FAIL because the page does not exist.

- [ ] **Step 4: Implement the master-detail page**

Use Tailwind and existing color/design tokens. Keep the result list bounded with `min-h-0`, `overflow-auto`, and a stable lower preview panel so progressive batches do not resize the entire settings window. Use `Intl.NumberFormat`/`Intl.DateTimeFormat` with the current language only for display formatting; do not store formatted values in scan state.

- [ ] **Step 5: Verify accessibility and commit the page**

```bash
pnpm --filter @markra/app test -- src/components/settings/ResourcesSettings.test.tsx
pnpm --filter @markra/app typecheck:test
git add packages/app/src/components/settings/ResourcesSettings.tsx packages/app/src/components/settings/ResourcesSettings.test.tsx packages/app/src/components/settings/resources/ResourcePreview.tsx packages/app/src/components/settings/resources/ResourceResultsList.tsx packages/app/src/components/SettingsSections.tsx
git commit -m "feat: add workspace resources settings page"
```

Expected: PASS.

---

### Task 9: Route, capability-gate, and translate the Resources category

**Files:**
- Modify: `packages/app/src/components/SettingsShell.test.tsx`
- Modify: `packages/app/src/components/SettingsShell.tsx`
- Modify: `packages/app/src/components/SettingsWindow.tsx`
- Modify: `packages/app/src/hooks/useSettingsWindowState.ts`
- Modify: `packages/app/src/App.test.tsx`
- Modify: `packages/shared/src/i18n/locales/types.ts`
- Modify: all locale files under `packages/shared/src/i18n/locales/`

**Interfaces:**

- Add `"resources"` to `SettingsCategory`.
- Add the Lucide `Image` icon and `settings.categories.resources` sidebar definition after MCP/logs and before appearance, matching the approved visual grouping.
- Hide the category when `appFeatures.resources === false`.
- Render `ResourcesSettings` only when `activeSettingsCategory === "resources"` and pass:

```tsx
<ResourcesSettings
  active
  globalIgnoreRules={fileIgnoreSettings.rules}
  sourceWindowLabel={settingsSourceWindowLabel}
  translate={translate}
  workspaceSourcePath={settingsWorkspaceSourcePath}
/>
```

- When another category is active, do not keep the scanner mounted. Leaving Resources must abort the active scan through hook cleanup.
- Add every `settings.resources.*` key used by the component to `I18nKey` and every locale. Provide complete Simplified Chinese and English copy; use the English text as an explicit fallback value in other locale maps unless a confident translation is supplied.

Required keys:

```text
settings.categories.resources
settings.resources.unused
settings.resources.missing
settings.resources.refresh
settings.resources.selectAll
settings.resources.selectedCount
settings.resources.moveToTrash
settings.resources.showInFolder
settings.resources.collecting
settings.resources.readingProgress
settings.resources.analyzingProgress
settings.resources.finalizing
settings.resources.resultsUpdating
settings.resources.noWorkspaceTitle
settings.resources.noWorkspaceDescription
settings.resources.noUnused
settings.resources.noMissing
settings.resources.incompleteTitle
settings.resources.incompleteDescription
settings.resources.snapshotUnavailable
settings.resources.retry
settings.resources.preview
settings.resources.previewUnavailable
settings.resources.fileName
settings.resources.relativePath
settings.resources.absolutePath
settings.resources.fileType
settings.resources.fileSize
settings.resources.modifiedAt
settings.resources.references
settings.resources.referenceLine
settings.resources.confirmTrash
settings.resources.confirmTrashAction
settings.resources.trashSucceeded
settings.resources.trashPartialFailure
settings.resources.staleRescan
settings.resources.scanFailed
settings.resources.openDocumentFailed
```

- [ ] **Step 1: Write failing sidebar and integration tests**

Assert Resources is shown and selectable when `features.resources` is true, hidden when false, the selected category renders the immediate-loading page, a workspace context replacement unmounts/cancels the old scan and mounts a new one, and existing Sync still receives only `projectRoot`.

- [ ] **Step 2: Run focused settings tests and confirm they fail**

```bash
pnpm --filter @markra/app test -- src/components/SettingsShell.test.tsx src/App.test.tsx
```

Expected: FAIL because the category and translation keys do not exist.

- [ ] **Step 3: Add routing, feature gating, and locale entries**

Use `settings.categories.resources: "资源"` and user-facing Simplified Chinese terminology consistent with the approved design: `未引用资源`, `丢失资源`, `移到废纸篓`, `扫描未完成`, and `结果仍在更新`. Keep status text concise enough for the fixed settings width.

- [ ] **Step 4: Run all affected feature tests**

```bash
pnpm --filter @markra/markdown test -- src/resource-references.test.ts
pnpm --filter @markra/app test -- src/components/SettingsShell.test.tsx src/components/settings/ResourcesSettings.test.tsx src/hooks/useSettingsWindowState.test.ts src/hooks/useWorkspaceResources.test.tsx src/App.test.tsx
pnpm --filter @markra/desktop test -- src/runtime/tauri/window.test.ts src/runtime/tauri/file.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit settings integration**

```bash
git add packages/app/src/components/SettingsShell.tsx packages/app/src/components/SettingsShell.test.tsx packages/app/src/components/SettingsWindow.tsx packages/app/src/hooks/useSettingsWindowState.ts packages/app/src/App.test.tsx packages/shared/src/i18n/locales
git commit -m "feat: expose resource diagnostics in settings"
```

---

### Task 10: Verify responsiveness, safety, and the complete repository

**Files:**
- Verify all files changed in Tasks 1-9.
- Modify only focused tests if verification reveals a missing assertion; do not broaden product scope.

**Interfaces:**
- Consumes all previous tasks.
- Produces evidence that the approved design and repository gates pass.

- [ ] **Step 1: Run targeted responsiveness and deletion-safety tests**

```bash
pnpm --filter @markra/app test -- src/hooks/useWorkspaceResources.test.tsx src/components/settings/ResourcesSettings.test.tsx
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml markdown_files::resource
```

Expected: PASS, including first-render-before-inventory, four-read bound, cancellation, stale-scan rejection, and symlink/traversal rejection.

- [ ] **Step 2: Scan for forbidden or incomplete implementation patterns**

```bash
rg -n "TODO|FIXME|placeholder|delete_workspace_resource|remove_file|remove_dir_all" packages/app/src/lib/workspace-resource* packages/app/src/hooks/useWorkspaceResources.ts packages/app/src/components/settings/ResourcesSettings.tsx apps/desktop/src-tauri/src/markdown_files/resource.rs
rg -n "new Worker|Promise\.all\(markdownFiles|readMarkdownFile" packages/app/src/hooks/useWorkspaceResources.ts packages/app/src/lib/workspace-resource-worker.ts
```

Expected: no placeholders, no permanent-delete fallback, exactly one dedicated worker construction path, and no unbounded `Promise.all` over all Markdown files.

- [ ] **Step 3: Run the required Rust suite**

```bash
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Expected: PASS.

- [ ] **Step 4: Run the required frontend suite and test typecheck**

```bash
pnpm test
pnpm typecheck:test
```

Expected: PASS.

- [ ] **Step 5: Run the production build**

```bash
pnpm build
```

Expected: PASS, including worker bundling and `verify:chunks`.

- [ ] **Step 6: Review the final diff and workspace hygiene**

```bash
git diff --check 6c3faf7..HEAD
git status --short
git log --oneline 6c3faf7..HEAD
```

Expected: no whitespace errors; implementation commits are focused; no generated directories or extra lockfiles are tracked; the only unrelated untracked file remains `bg.png`.

- [ ] **Step 7: Perform a real desktop smoke test**

Run:

```bash
pnpm tauri dev
```

In the live desktop app:

1. Open a workspace containing several Markdown files and `assets` files.
2. Open Settings > Resources and confirm the page frame appears immediately while progress changes in place.
3. Confirm sidebar navigation and Refresh remain responsive during scanning.
4. Confirm an unsaved reference in an open document prevents that resource from being listed as unused.
5. Confirm a broken local resource displays its referring document and opens that document.
6. Select a disposable unused fixture, confirm Move to Trash, verify it appears in the operating system trash, and verify the page rescans.
7. Change a Markdown reference externally between scan completion and confirmation, then confirm deletion is refused and a rescan starts.

Stop the dev process after the checks. If the smoke test requires temporary workspace fixtures, create them outside the repository and remove them after verification.

- [ ] **Step 8: Commit any verification-only test corrections**

Only if Step 1-7 required test corrections, inspect `git diff --name-only`, stage only the exact test files that were corrected, and run:

```bash
git commit -m "test: cover resource management safety"
```

Do not stage product changes in this verification-only commit, and do not create an empty commit.
