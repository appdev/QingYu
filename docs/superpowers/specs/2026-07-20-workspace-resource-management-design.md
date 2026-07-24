# Workspace Resource Management Design

## Status

Approved in conversation on 2026-07-20.

## Goal

Add a native settings category that helps users find two workspace resource problems without making settings navigation wait:

- files under managed `assets` directories that no Markdown document references;
- local resource references in Markdown documents whose targets no longer exist.

The page must remain responsive while it scans. Removing an unused resource must move it to the operating system trash after a fresh safety check.

## Context

QingYu currently stores copied images and generic attachments together under `assets`. A sync-enabled project uses the project-root `assets` directory. A standalone workspace can contain an `assets` directory beside a Markdown document, including nested document folders. Deleting a Markdown file currently deletes only that file, so resources that were used only by the deleted document remain on disk.

The application already has several useful boundaries:

- the file tree distinguishes Markdown documents, image assets, generic attachments, and folders;
- `@markra/markdown` already owns Markdown parsing utilities;
- the desktop runtime already supplies canonicalized workspace file operations;
- the native service already depends on the system-trash library;
- the settings window already receives the invoking editor's current workspace context and updates when that context changes.

The new feature should reuse these boundaries. It must not add a persistent resource database or a second general-purpose filesystem layer.

## Selected Approach

Use an on-demand hybrid scan:

1. The settings page appears immediately.
2. Existing native APIs enumerate the current workspace and read Markdown files asynchronously.
3. A dedicated web worker uses shared Markdown utilities to extract local resource references without blocking the settings renderer.
4. Pure TypeScript resource-graph logic compares the reference set with the resource inventory.
5. A narrowly scoped native command revalidates selected files and moves them to the system trash.

The scan has no persistent index. Opening the page or pressing Refresh starts a new scan. Leaving the page, switching workspaces, or starting a replacement scan cancels the old one.

### Rejected alternatives

- A continuously maintained in-memory index would make the page open with cached results, but adds watcher lifecycle, incremental invalidation, and multi-window consistency problems.
- A persistent database would improve repeated scans for extremely large workspaces, but introduces migrations, repair behavior, and another source of stale state. That cost does not fit the product's current simple note-taking scope.

## Scope

### Included

- A `Resources` category in the desktop settings sidebar.
- Scanning the entire current workspace.
- Recursive resource inventory below managed directories named `assets`.
- Markdown reference extraction from every supported Markdown document in the scan universe.
- `Unused resources` and `Missing resources` result tabs.
- Multi-selection and bulk move-to-trash for unused resources.
- Image preview and metadata preview for other attachments.
- Listing and opening documents that contain a missing resource reference.
- Manual refresh, progress, cancellation, partial-failure reporting, and empty states.
- Unsaved open-document contents as overlays during analysis.

### Not included

- An `Unused databases` tab.
- A persistent resource index or database.
- Automatic deletion when a document is deleted.
- Automatic replacement of missing files.
- Automatic editing or removal of broken Markdown links.
- Deduplication, content hashing, resource renaming, or resource-folder migration.
- Managing remote URLs, embedded data URLs, or files outside the current workspace.
- Managing arbitrary files outside a directory named `assets`.
- Providing this category on runtimes that cannot safely enumerate local workspace files and use the operating system trash.

## Definitions

### Managed resource location

A path within the current workspace whose directory components include a directory named `assets`. The directory may be absent when a Markdown reference is analyzed; this allows `assets/missing.png` to remain diagnosable even when the entire `assets` directory has been removed.

For existing files, every `assets` component and its parents must be real directories. The scan inventories regular files recursively below every real `assets` directory. Directories are not themselves resources. Symbolic links are neither followed nor treated as manageable resources.

This covers both current storage forms:

- `<project root>/assets/**` for a sync-enabled project;
- `<Markdown directory>/assets/**` for standalone documents, including nested document folders.

### Resource reference

A local Markdown destination that resolves inside a managed resource location. Supported references include:

- inline Markdown images;
- inline Markdown links to generic attachments;
- reference-style image and link destinations;
- local resource destinations in supported raw HTML image and anchor forms.

Before comparison, the scanner decodes URL-escaped path segments and removes query strings and fragments. Relative destinations resolve from the containing Markdown file. Absolute local destinations count only when they resolve inside the current workspace and a managed resource location.

The scanner ignores HTTP, HTTPS, other URI schemes, `data:` sources, wiki-style document links, Markdown-document links, directories, and targets outside the current workspace.

### Unused resource

A regular file below a managed resource location for which a complete scan finds zero resource references across all readable Markdown documents, including unsaved content overlays.

Unused status is never final while a scan is still running. If any document required for the scan cannot be read or parsed, the scan is incomplete and unused-resource deletion remains disabled.

### Missing resource

A resource reference whose resolved target is inside a managed resource location but does not exist as a regular file. One missing target can contain multiple occurrence records from one or more Markdown documents.

### Markdown scan universe

All regular `.md` and `.markdown` files that belong to the current workspace according to the application's workspace file-scanning and ignore-rule contract. Content excluded by that contract is outside resource management as well. The resource scanner must use the same rules rather than maintaining a second interpretation. Symbolic links are never followed.

## User Experience

### Navigation and immediate presentation

Add `Resources` with a Lucide image icon to the settings sidebar. Selecting it completes immediately and renders the page frame during the first paint. Scanning begins only after the page is mounted.

The settings header, sidebar navigation, window controls, tab bar, Refresh action, and current workspace label stay interactive throughout the scan. No file enumeration or Markdown parsing belongs in the render path.

When no workspace is open, the page shows an explanatory empty state and does not scan. If the runtime lacks the required native capabilities, the category is hidden in the same way as other capability-gated settings categories.

### Page layout

The page follows a vertical master-detail layout similar to the supplied reference:

1. A segmented tab control for `Unused resources` and `Missing resources`, each with a result count when known.
2. A toolbar for selection and context-appropriate actions.
3. A bounded result list in the upper portion of the page.
4. A detail and preview panel below the list.

The unused-resource toolbar contains:

- selection count and Select all;
- `Move to Trash`, disabled until a complete scan is ready and at least one item is selected;
- `Show in Folder` for a single selected item;
- Refresh.

The missing-resource toolbar contains Refresh. Selecting a missing target shows every referring Markdown document in the detail panel. Each occurrence includes the document-relative path and line number when available. Activating an occurrence asks the native window service to open that document in an editor window. The first version does not edit the document or scroll to an exact source range.

### Preview

Selecting an existing image displays a large aspect-fit preview without reading it into the React component as a base64 string. The preview uses the existing safe local-asset URL path.

Selecting a non-image attachment displays:

- file name;
- relative and absolute paths;
- classified type or extension;
- size;
- modified time.

A missing resource has no file preview. Its detail panel instead lists the stored missing path and all reference occurrences.

### Loading and progressive results

The initial scan renders skeleton rows and a progress label such as `Scanning 24 of 120 documents`. Progress is phase-aware:

- collecting workspace files;
- reading documents;
- analyzing references;
- finalizing results.

Missing references may appear provisionally as documents finish analysis, with a visible `Results are still updating` status. Existing resource rows may appear with a `Checking` state. No resource is labeled conclusively unused, selectable for deletion, or included in the unused count until every required document has completed successfully.

The implementation uses a bounded document-read queue and a web worker. It must not load every Markdown file into renderer memory at once. The worker accepts bounded batches, returns occurrences and progress, and releases completed content. Cancellation terminates the worker and ignores late native responses.

### Ready, empty, incomplete, and error states

- A completed scan with no results shows `No unused resources` or `No missing resources` in the corresponding tab.
- A document read or parse failure produces an `Incomplete scan` banner with the failed document count. Missing-resource diagnostics remain visible, but moving unused files to trash is disabled.
- A top-level inventory or context failure shows an inline error with Retry. It does not replace or disable the settings sidebar.
- A workspace change cancels the scan, clears old results immediately, updates the page context, and starts a new scan only if the Resources page remains active.

## Scan Architecture

### Shared Markdown reference extraction

Add a focused, pure resource-reference module to `@markra/markdown`. It returns occurrences rather than resolving filesystem paths. Each occurrence contains at least:

- the raw destination;
- source syntax kind;
- source offsets;
- line and column;
- display text when present.

The module must use the repository's established Markdown parser and extensions so resource analysis matches editor syntax. Raw HTML support must be syntax-aware and limited to the supported local image and anchor attributes. It must not interpret paths from fenced code, inline code, or plain text.

### Resource graph

Add a pure application module that accepts:

- workspace root;
- resource inventory;
- Markdown file inventory;
- extracted occurrences per source document;
- read and parse failures.

It resolves occurrences relative to their source documents and produces:

- existing resources with reference counts;
- provisional and final missing targets with occurrence lists;
- final unused resources;
- warnings and scan completeness;
- aggregate file counts and byte sizes.

Paths are keyed by normalized absolute identity, not by basename. This prevents collisions between separate nested `assets` directories. Filesystem existence, workspace containment, platform case behavior, and symlink status remain native responsibilities; the TypeScript layer consumes native-normalized paths rather than guessing filesystem identity.

### Scan coordinator

Add `useWorkspaceResources` as the page-facing coordinator. It owns a monotonic scan generation and the following states:

- `idle`;
- `scanning`;
- `ready`;
- `incomplete`;
- `error`.

Every asynchronous response carries or closes over its generation. Results from a canceled or superseded generation are ignored.

The coordinator requests a live workspace snapshot from the invoking editor window. That snapshot includes the workspace root, a document-state generation, and the contents of dirty saved documents in that workspace. Disk reads are used for all other documents. If a reliable live snapshot cannot be obtained, the scan can show diagnostics but cannot enable unused-resource deletion.

Before deletion, the coordinator requests both a fresh live snapshot and a fresh metadata-only Markdown inventory. A changed document-state generation, changed dirty overlay, changed workspace, added or removed Markdown path, changed Markdown size or modified time, or incomplete reply invalidates the deletion attempt and starts a fresh scan. This prevents newly added unsaved or externally written references from being missed.

The live snapshot extends the existing settings-window context channel rather than placing document contents in URL parameters or persistent settings.

### Worker boundary

The worker receives only the data required for parsing and graph construction. It cannot access the filesystem or invoke native commands. The main settings thread owns native reads, batching, scan generations, and user-visible state.

Worker messages are explicit typed requests and responses. A scan cancellation terminates the worker instance; the next scan creates a fresh instance. Worker crashes transition the page to an error or incomplete state and never enable deletion.

### Native boundary

Reuse the existing background file-tree enumeration for workspace entries and metadata. Extend the runtime surface only where the resource manager needs a capability that does not already exist:

- native-normalized managed resource inventory and path identity where existing entries are insufficient;
- move verified managed resources to the system trash;
- reveal one existing resource in the platform file manager;
- open a referring Markdown document in an editor window.

Do not expose arbitrary absolute-path trash operations. The delete request includes:

- workspace root;
- resource path relative to that root;
- expected size;
- expected modified time;
- scan generation or equivalent request identity.

For each requested file, native code reopens the canonical workspace root, verifies the lexical and canonical resource path, rejects symlinks, confirms the file is a regular file below a real `assets` directory, and compares current metadata with the expected values. Only then may it invoke the existing system-trash function.

Batch deletion returns one result per requested file. One failure does not prevent other independently verified files from being moved to trash.

## Deletion Flow

1. The user selects one or more final unused resources.
2. The page displays a confirmation dialog with file count and total size.
3. After confirmation, the coordinator requests a fresh live document snapshot and Markdown metadata inventory.
4. If the workspace, document generation, Markdown path set, or Markdown metadata changed, deletion stops and a rescan begins.
5. The settings runtime sends expected metadata for the selected files to the native resource-trash command.
6. Native code validates every file independently and moves valid files to the operating system trash.
7. The page reports succeeded and failed counts, keeps failed items selected, and starts a full rescan.

The application never permanently deletes resources through this page.

## Settings Integration

Add `resources` to the settings category type, sidebar definitions, translated category labels, and settings-window content routing. Add a runtime capability flag so unsupported runtimes hide the category.

The settings window already receives the invoking editor's workspace path at creation and through live context updates. Generalize that context naming from sync-only usage where necessary, while preserving current sync-settings behavior. The Resources page uses this current workspace root and must not fall back silently to a stale recent folder.

The resource page does not add persisted user preferences. Tab selection, row selection, progress, and scan results are session state only and reset when the settings window is destroyed.

## Error Handling

- Enumeration failure: show an inline error and Retry; no stale results remain actionable.
- Individual read or parse failure: mark the scan incomplete, list the failure count, and disable unused-resource deletion.
- Worker failure: terminate the worker, show a retryable error, and disable deletion.
- Preview failure: keep the selection and show `Preview unavailable`; it does not invalidate the scan.
- Editor snapshot failure: allow read-only diagnostics from disk but mark deletion unavailable.
- Workspace change: cancel and clear immediately.
- Freshness failure before deletion: delete nothing and rescan.
- Per-file native validation or trash failure: skip that file, continue other files, and show a summarized result with the failed file names available in details.
- Referring-document open failure: retain the result and show a non-destructive error toast.

## Accessibility

- The segmented tabs use tab semantics or the repository's accessible segmented-control pattern.
- Result rows support keyboard selection and expose selected state.
- Bulk actions have explicit labels and disabled reasons.
- Loading progress uses a polite live region and does not announce every individual file.
- The confirmation dialog receives initial focus and supports the existing Escape and keyboard-navigation behavior.
- Image previews include the resource file name as alternative text.
- Status is never communicated only through color.

## Testing

### Markdown package tests

- inline image and attachment destinations;
- reference-style images and links;
- supported raw HTML destinations;
- escaped punctuation, spaces, Unicode, percent encoding, queries, and fragments;
- fenced code, inline code, plain text, remote schemes, data URLs, wiki links, and Markdown-document links are ignored.

### Resource graph tests

- one resource referenced by one or several documents;
- duplicate basenames in different nested `assets` roots;
- standalone nested `assets` directories and project-root `assets`;
- unused resources are withheld until completion;
- missing-target occurrences are grouped across documents;
- unsaved overlays replace disk contents for matching paths;
- unreadable or unparseable documents force incomplete status;
- workspace-external and symlinked paths never become manageable resources.

### Coordinator and worker tests

- the page shell renders before inventory and parsing finish;
- progress advances by phase and document count;
- bounded batches do not enqueue the entire workspace at once;
- cancellation, category changes, workspace changes, and replacement scans ignore stale results;
- worker failure is retryable and never enables deletion;
- a changed live document generation invalidates deletion and triggers a rescan.
- an externally added, removed, or modified Markdown file invalidates deletion and triggers a rescan.

### Native tests

- valid unused resources below root and nested `assets` directories reach a mocked system-trash function;
- traversal, absolute-path substitution, files outside the workspace, non-`assets` files, directories, and symlinks are rejected;
- changed size or modified time is rejected;
- root or destination replacement races are rejected;
- batch operations return independent success and failure results;
- reveal and referring-document open operations stay inside their intended boundaries.

### Component and integration tests

- Resources appears only when the runtime capability is available;
- immediate loading layout, progressive missing results, final counts, empty states, incomplete state, and retry;
- image preview, attachment metadata, missing-reference occurrence list, and open-document action;
- selection, Select all, confirmation content, disabled states, partial trash failure, and post-delete rescan;
- settings context changes replace the workspace without leaking old results.

### Repository verification

Run the focused package and component tests during implementation, followed by:

- `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`
- `pnpm test`
- `pnpm typecheck:test`
- `pnpm build`

## Success Criteria

- Selecting Resources never waits for a completed scan before showing the page.
- Scanning and Markdown parsing do not make the settings window feel blocked.
- A complete scan correctly separates unused existing resources from missing referenced resources across the current workspace.
- No resource can be moved to trash from a partial, stale, or workspace-mismatched scan.
- All deletion performed by the page goes through the operating system trash.
- Missing-resource results identify and open every referring Markdown document without modifying it.
- The feature adds no persistent resource database and does not change normal document deletion behavior.
