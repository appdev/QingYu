# Markra Core SiYuan Adapter Implementation Plan

> **For agentic workers:** Use the global `workflow` skill's existing-plan execution entry. Review this plan against current evidence; when it is sound, enter execution directly. Only when material problems are found should `workflow` return to research, ideation, and planning to supplement this same plan before continuing. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the unfinished QingYu/SiYuan Markdown live-preview implementation with the framework-independent Markra CodeMirror core while preserving the complete SiYuan UI shell and native image-resize interaction.

**Architecture:** Keep `MarkdownEditor` as the SiYuan lifecycle, file, tab, and UI owner. Vendor the framework-independent Markra editor core from a pinned, product-stripped QingYu snapshot behind a `MarkdownHostAdapter`; keep canonical Markra v2.5.6 as the comparison target, route platform behavior through a SiYuan adapter, and style generated semantics inside the existing `.markdown-editor` namespace.

**Tech Stack:** TypeScript 4.9, CodeMirror 6, Lezer Markdown/GFM, Turndown, KaTeX, Mermaid, Lowlight, JSDOM/Node test runner, Electron layout tests, existing SiYuan DOM/SCSS, Go Kernel Markdown API.

## Global Constraints

- Source the framework-independent core from `/Volumes/extendData/Data/IdeaProjects/markra` at `2b9423c5a81ba6d9c70127f72dbb16ec1fcdb1b2`; record its canonical v2.5.5 base `22f0ebe40dc4ba8fcb653ed6c0719284ef76f361` and canonical v2.5.6 comparison target `a5dec440fb3ae7c33fcd714be85f03b82814e5ef`.
- Do not introduce React, Tauri, Markra AI, custom spellcheck, Markra file-workspace, sync, theme, or product UI code.
- Preserve the current SiYuan tab, title, breadcrumb, mode buttons, save status, content width, theme, desktop split, and mobile layout.
- Keep `EditorState.doc` as the only writable Markdown state; visual/source modes share one `EditorView`, selection, and history.
- Keep `/api/markdown/get`, `/api/markdown/save`, `/api/markdown/rename`, the 800ms autosave, and revision-conflict behavior.
- Use SiYuan's `.img` and `.protyle-action__drag` image-resize UI; do not ship Markra resize controls or CSS.
- Preserve unrelated dirty and untracked user work. Do not clean, reset, stash, or overwrite it.
- Do not run `pnpm build`. Do not compile or restart a running Kernel.
- Run focused tests during development and `cd app && pnpm run lint` at the frozen integration barrier.
- Do not run `git commit` or `git push`; the repository instructions require separate explicit authorization.

---

## File Structure

### New core and provenance

- `app/src/markdown/markra-core/UPSTREAM.md`: pinned SHA, source paths, license, excluded modules, and sync procedure.
- `app/src/markdown/markra-core/index.ts`: public framework-independent editor API used by `MarkdownEditor`.
- `app/src/markdown/markra-core/adapter.ts`: platform-neutral `MarkdownHostAdapter` contract and injected adapter facet.
- `app/src/markdown/markra-core/codemirror/*.ts`: ported Markra CodeMirror modules, retaining upstream file names when possible.
- `app/src/markdown/markra-core/markdown/*.ts`: only the pure Markra Markdown helpers required by the editor.
- `app/src/markdown/markra-core/runtime/*.ts`: framework-neutral Mermaid, math, code-highlighting, resource, and clipboard types.
- `app/src/markdown/markra-core/test/*.ts`: shared JSDOM setup and adapter fakes.

### New SiYuan integration

- `app/src/markdown/siyuanAdapter.ts`: concrete adapter for resources, URLs, icons, menus, labels, diagrams, math, and errors.
- `app/src/protyle/util/imageResize.ts`: shared pointer geometry/lifecycle used by Protyle and Markdown image widgets.
- `app/src/protyle/util/imageResize.test.ts`: pure geometry and lifecycle tests.
- `app/src/markdown/markraIntegration.test.ts`: editor assembly, mode, history, save-boundary, and adapter integration tests.
- `app/scripts/testMarkdownUiParity.cjs`: Electron layout/style parity assertions.

### Modified integration files

- `app/package.json`, `app/pnpm-lock.yaml`: mature Markra runtime dependencies and test scripts.
- `app/src/markdown/MarkdownEditor.ts`: assemble Markra Core with `SiyuanMarkdownAdapter` while preserving the shell.
- `app/src/markdown/clipboard.ts`: become a thin adapter/export surface or be removed after all callers migrate.
- `app/src/markdown/keyboard.ts`: retain only SiYuan shell-level shortcuts.
- `app/src/protyle/wysiwyg/index.ts`: delegate existing image-resize geometry to the shared helper without changing `.sy` transaction behavior.
- `app/src/assets/scss/business/_markdown.scss`: map core semantic classes to SiYuan variables and native image UI.
- `app/src/mobile/markdown.ts`, `app/src/mobile/markdownState.ts`: keep mobile lifecycle and route editor behavior to the shared core.
- `app/scripts/testMarkdownLivePreview.cjs`, `app/scripts/testMarkdownLayout.cjs`: assert the new core and current UI contract.

### Removed after cutover

- `app/src/markdown/livePreview/`: delete the old implementation only after equivalent Markra Core coverage is green.

---

### Task 1: Pin Provenance and Establish the Compatibility Boundary

**Files:**
- Create: `app/src/markdown/markra-core/UPSTREAM.md`
- Create: `app/src/markdown/markra-core/adapter.ts`
- Create: `app/src/markdown/markra-core/adapter.test.ts`
- Modify: `app/package.json`
- Modify: `app/pnpm-lock.yaml`

**Interfaces:**
- Produces: `MarkdownHostAdapter`, `MarkdownIconName`, `MarkdownSavedAsset`, `MarkdownClipboardAssetRequest`, `MarkdownRenderContext`, `markdownHostAdapterFacet`.
- Consumes: existing SiYuan CodeMirror dependencies and browser DOM types.

- [ ] **Step 1: Capture the pre-migration tree and runtime baseline**

Run:

```bash
git status --short > /tmp/qingyu-markdown-migration-status-before.txt
git diff -- app/src/markdown app/src/assets/scss/business/_markdown.scss app/package.json app/pnpm-lock.yaml > /tmp/qingyu-markdown-migration-before.patch
git -C /Volumes/extendData/Data/IdeaProjects/markra show -s --format='%H %cd %s' --date=iso-strict 2b9423c5a81ba6d9c70127f72dbb16ec1fcdb1b2
```

Expected: the Markra command prints the exact pinned SHA; the two `/tmp` files record evidence only and do not change the repository.

- [ ] **Step 2: Write the adapter contract test first**

Create a test that installs a fake adapter in an `EditorState` and verifies that core code can retrieve exactly that instance:

```ts
const adapter = createFakeMarkdownHostAdapter();
const state = EditorState.create({extensions: [markdownHostAdapter(adapter)]});
assert.equal(readMarkdownHostAdapter(state), adapter);
```

Run:

```bash
cd app && node --import tsx --test src/markdown/markra-core/adapter.test.ts
```

Expected: FAIL because `markdownHostAdapter` and `readMarkdownHostAdapter` do not exist.

- [ ] **Step 3: Define the exact host contract**

Implement these stable signatures in `adapter.ts`:

```ts
export type MarkdownIconName = "add" | "remove" | "trash" | "zoomIn" | "zoomOut" | "open";

export interface MarkdownSavedAsset {
    markdownDestination: string;
    name: string;
}

export interface MarkdownClipboardAssetRequest {
    files: readonly File[];
    insertionOffset: number;
}

export interface MarkdownRenderContext {
    documentPath: string;
    ownerDocument: Document;
}

export interface MarkdownHostAdapter {
    createIcon(name: MarkdownIconName, className: string): SVGElement;
    notifyError(message: string): void;
    openLink(target: string): void;
    positionPopover(anchor: HTMLElement, popover: HTMLElement): void;
    renderMath(source: string, displayMode: boolean, context: MarkdownRenderContext): HTMLElement;
    renderMermaid(source: string, context: MarkdownRenderContext): Promise<HTMLElement>;
    resolveImageSource(source: string, documentPath: string): string | null;
    saveClipboardAssets(request: MarkdownClipboardAssetRequest): Promise<readonly MarkdownSavedAsset[]>;
}
```

Expose the adapter through a CodeMirror facet. Do not import `window.siyuan` in this file.

- [ ] **Step 4: Add only required mature dependencies**

Add the versions used by the pinned Markra snapshot for `turndown`, `katex`, `lowlight`, `mermaid`, `remark-parse`, `remark-gfm`, `remark-math`, `unified`, and their required types. Do not add `react`, `@tauri-apps/api`, `@markra/ai`, `cspell-trie-lib`, or `lucide`.

Run:

```bash
cd app && pnpm install
cd app && node --import tsx --test src/markdown/markra-core/adapter.test.ts
```

Expected: lockfile updates only for declared dependencies; adapter test passes.

- [ ] **Step 5: Record provenance and exclusions**

`UPSTREAM.md` must contain the exact SHA, source repository URL, source directories, AGPL-3.0 license, the local compatibility changes allowed by the design, and the excluded React/Tauri/AI/spellcheck/product modules. Verify:

```bash
rg -n '2b9423c5a81ba6d9c70127f72dbb16ec1fcdb1b2|a5dec440fb3ae7c33fcd714be85f03b82814e5ef|AGPL-3.0|editor-react|Tauri|spellcheck|AI' app/src/markdown/markra-core/UPSTREAM.md
```

Expected: every required provenance and exclusion term is present.

### Task 2: Port the Core Infrastructure Without Product Dependencies

**Files:**
- Create: `app/src/markdown/markra-core/codemirror/changes.ts`
- Create: `app/src/markdown/markra-core/codemirror/syntax.ts`
- Create: `app/src/markdown/markra-core/codemirror/policy.ts`
- Create: `app/src/markdown/markra-core/codemirror/renderers.ts`
- Create: `app/src/markdown/markra-core/codemirror/plugin.ts`
- Create: `app/src/markdown/markra-core/codemirror/preview.ts`
- Create: `app/src/markdown/markra-core/codemirror/highlight.ts`
- Create: `app/src/markdown/markra-core/codemirror/theme.ts`
- Create: `app/src/markdown/markra-core/codemirror/index.ts`
- Create: `app/src/markdown/markra-core/test/dom.ts`
- Create: `app/src/markdown/markra-core/codemirror/core.test.ts`

**Interfaces:**
- Consumes: `markdownHostAdapterFacet` from Task 1.
- Produces: `markraRenderer`, `markraPlugins`, `revealActiveLine`, `livePreview`, `markraLanguage`, and `liveMarkdown`.

- [ ] **Step 1: Write a failing framework-independence test**

The test creates an `EditorView` containing `# Title\n\nText`, installs a fake adapter and `liveMarkdown()`, and asserts:

```ts
assert.equal(view.state.doc.toString(), "# Title\n\nText");
assert.ok(view.dom.querySelector(".cm-editor"));
assert.equal(view.dom.querySelector("[data-reactroot]"), null);
```

Also statically scan the new core directory and fail if imports contain `react`, `@tauri-apps`, `@markra/ai`, `cspell`, or `window.siyuan`.

Run:

```bash
cd app && node --import tsx --test src/markdown/markra-core/codemirror/core.test.ts
```

Expected: FAIL because core modules do not exist.

- [ ] **Step 2: Port the pinned infrastructure modules**

Copy the corresponding files from:

```text
/Volumes/extendData/Data/IdeaProjects/markra@2b9423c5a81ba6d9c70127f72dbb16ec1fcdb1b2:
packages/editor/src/codemirror/{changes,syntax,policy,renderers,plugin,preview,highlight,theme,index}.ts
```

Preserve behavior and tests. Change imports only to local core paths and Task 1 interfaces. Apply TypeScript 4.9-compatible syntax without changing runtime semantics.

- [ ] **Step 3: Assemble the public core entry**

Create `app/src/markdown/markra-core/index.ts` that exports only the supported editor surface:

```ts
export {liveMarkdown, markraLanguage} from "./codemirror";
export {markdownHostAdapter, readMarkdownHostAdapter} from "./adapter";
export type {MarkdownHostAdapter} from "./adapter";
```

Do not export AI or spellcheck seams.

- [ ] **Step 4: Run focused tests and typecheck**

Run:

```bash
cd app && node --import tsx --test src/markdown/markra-core/adapter.test.ts src/markdown/markra-core/codemirror/core.test.ts
cd app && pnpm run typecheck
```

Expected: all focused tests and typecheck pass; static forbidden-import scan is clean.

### Task 3: Port Inline, Structural, and Editing Semantics

**Files:**
- Create: `app/src/markdown/markra-core/codemirror/inline-markdown.ts`
- Create: `app/src/markdown/markra-core/codemirror/blank-lines.ts`
- Create: `app/src/markdown/markra-core/codemirror/markdown-editing.ts`
- Create: `app/src/markdown/markra-core/codemirror/markdown-shortcuts.ts`
- Create: `app/src/markdown/markra-core/codemirror/formatting.ts`
- Create: `app/src/markdown/markra-core/codemirror/insertions.ts`
- Create: `app/src/markdown/markra-core/codemirror/tasks.ts`
- Create: `app/src/markdown/markra-core/codemirror/links.ts`
- Create: `app/src/markdown/markra-core/codemirror/callout-preview.ts`
- Create: `app/src/markdown/markra-core/codemirror/footnote-preview.ts`
- Create: `app/src/markdown/markra-core/codemirror/frontmatter-preview.ts`
- Create: `app/src/markdown/markra-core/codemirror/horizontal-rule.ts`
- Test: matching `*.test.ts` files beside each module.

**Interfaces:**
- Consumes: renderer/plugin/policy APIs from Task 2 and adapter icon/link functions from Task 1.
- Produces: plugins registered by `liveMarkdown()` for inline editing and structural preview.

- [ ] **Step 1: Port the upstream tests before implementation**

Bring over the pinned tests for blank lines, editing, shortcuts, formatting, links, callouts, footnotes, frontmatter, and horizontal rules. Add a SiYuan-specific regression asserting `Mod-a` selects `{from: 0, to: doc.length}` in visual mode.

Run:

```bash
cd app && node --import tsx --test src/markdown/markra-core/codemirror/{blank-lines,markdown-editing,markdown-shortcuts,formatting,links,callout-preview,footnote-preview,frontmatter-preview,horizontal-rule}.test.ts
```

Expected: FAIL because implementations and registrations are missing.

- [ ] **Step 2: Port the matching pinned modules**

Copy behavior from the exact upstream paths with the same base names. Replace Markra icon, link, and label helpers with `MarkdownHostAdapter`; do not simplify selection, composition, blank-line, or reveal rules.

- [ ] **Step 3: Register the supported union**

Update `liveMarkdown()` so its default plugin set includes these modules but excludes AI and spellcheck. Verify the source remains byte-for-byte stable across editor recreation:

```ts
const source = view.state.doc.toString();
const recreated = createView(source);
assert.equal(recreated.state.doc.toString(), source);
```

- [ ] **Step 4: Run the focused group and typecheck**

Run the Step 1 command and `cd app && pnpm run typecheck`.

Expected: all tests pass, including `Mod-a`, IME-safe reveal, blank lines, and recreation.

### Task 4: Port Code, Math, Mermaid, and Safe HTML Renderers

**Files:**
- Create: `app/src/markdown/markra-core/code-support.ts`
- Create: `app/src/markdown/markra-core/math-render.ts`
- Create: `app/src/markdown/markra-core/mermaid.ts`
- Create: `app/src/markdown/markra-core/raw-html-sanitize.ts`
- Create: `app/src/markdown/markra-core/codemirror/code-block.ts`
- Create: `app/src/markdown/markra-core/codemirror/math-preview.ts`
- Create: `app/src/markdown/markra-core/codemirror/raw-html-preview.ts`
- Test: matching upstream-derived tests.

**Interfaces:**
- Consumes: `renderMath`, `renderMermaid`, and error reporting from `MarkdownHostAdapter`.
- Produces: code block, math, Mermaid, and raw HTML preview plugins with source fallback.

- [ ] **Step 1: Add failing renderer lifecycle tests**

Cover fenced code, unknown languages, inline/display math, successful Mermaid, rejected Mermaid promise, sanitized HTML, cursor reveal, blur, and editor recreation. Every failure case must assert `view.state.doc.toString()` is unchanged.

Run:

```bash
cd app && node --import tsx --test src/markdown/markra-core/{code-support,math-render,mermaid,raw-html-sanitize}.test.ts src/markdown/markra-core/codemirror/{code-block,math-preview,raw-html-preview}.test.ts
```

Expected: FAIL because renderer modules are absent.

- [ ] **Step 2: Port renderer logic and inject host runtimes**

Preserve Markra range/version checks and source-reveal behavior. Replace direct application UI and platform access with Adapter calls. Keep raw HTML sanitization before DOM insertion.

- [ ] **Step 3: Verify renderer failure isolation**

Run the Step 1 suite with fake adapters that throw once for Mermaid and math.

Expected: the affected block displays source/error fallback; subsequent paragraphs remain editable; all source-stability assertions pass.

### Task 5: Port the Mature Table Editor

**Files:**
- Create: `app/src/markdown/markra-core/markdown/table-fragment.ts`
- Create: `app/src/markdown/markra-core/codemirror/table-fragment-merge.ts`
- Create: `app/src/markdown/markra-core/codemirror/table.ts`
- Test: `table-fragment.test.ts`, `table-fragment-merge.test.ts`, `table.test.ts`.

**Interfaces:**
- Consumes: adapter icons/popover positioning and renderer APIs.
- Produces: `tablePreviewPlugin`, `parseGfmTableFragment`, `readCodeMirrorTableShape`.

- [ ] **Step 1: Port tests for the user's failure modes first**

In addition to upstream tests, add the following source fixture:

```md
| 版本 | 日期 | 作者 | 变更 |
| --- | --- | --- | --- |
| v1.8 | 2026-04-27 | 架构组 | **NAS 固件侧接口确认落地。**<br/>`POST /ugreen/v1/desktop/message/push` |
```

Assert it renders one table with four columns, preserves `<br/>`, allows cell editing, and serializes without losing bold/code syntax.

Run:

```bash
cd app && node --import tsx --test src/markdown/markra-core/markdown/table-fragment.test.ts src/markdown/markra-core/codemirror/{table-fragment-merge,table}.test.ts
```

Expected: FAIL before the table modules exist.

- [ ] **Step 2: Port parser, serializer, Widget, and transaction commands**

Keep Markra's table parser and row/column editing behavior. Route icons and popover geometry through Adapter. Do not import Markra table CSS; emit stable semantic classes for Task 11.

- [ ] **Step 3: Add width containment assertions**

Create an eight-column fixture and assert its widget wrapper has `min-width: 0` ownership and a local overflow container rather than increasing `.markdown-editor` scroll width.

- [ ] **Step 4: Run table tests and typecheck**

Run the Step 1 command and `cd app && pnpm run typecheck`.

Expected: all table parsing, editing, source round-trip, and width tests pass.

### Task 6: Share SiYuan Native Image-Resize Interaction

**Files:**
- Create: `app/src/protyle/util/imageResize.ts`
- Create: `app/src/protyle/util/imageResize.test.ts`
- Modify: `app/src/protyle/wysiwyg/index.ts:1196-1270`
- Create: `app/src/markdown/markra-core/codemirror/image-attributes.ts`
- Create: `app/src/markdown/markra-core/codemirror/image.ts`
- Create: `app/src/markdown/markra-core/codemirror/image-resize.test.ts`
- Create: `app/src/markdown/markra-core/codemirror/image-preview.test.ts`

**Interfaces:**
- Produces: `startSiyuanImageResize(options: SiyuanImageResizeOptions): () => void`, `imagePreviewPlugin`, `imageAttributeDetails`, and `replaceImageWidth`.
- Consumes: Adapter image URL resolution and CodeMirror Transaction dispatch.

- [ ] **Step 1: Define failing geometry and lifecycle tests**

Use this contract:

```ts
export interface SiyuanImageResizeOptions {
    centerResize: boolean;
    initialClientX: number;
    initialWidth: number;
    maxRight: number;
    minWidth: number;
    onPreview(width: number): void;
    onCommit(width: number): void;
    onCancel(): void;
}
```

Test one-sided and centered multipliers, 17px Protyle minimum, right-bound clamping, pointer cleanup, cancel, and exactly one commit callback.

Run:

```bash
cd app && node --import tsx --test src/protyle/util/imageResize.test.ts
```

Expected: FAIL because the helper does not exist.

- [ ] **Step 2: Extract the existing Protyle behavior without changing persistence**

Move only pointer geometry and listener lifecycle from `wysiwyg/index.ts` into `imageResize.ts`. Keep the existing `updateTransaction(protyle, nodeElement, html)` call in the Protyle caller. Run the new test and the existing frontend typecheck.

- [ ] **Step 3: Port Markra image parsing and range logic**

Port image source safety, Markdown/HTML image parsing, attribute parsing, width replacement, async insertion target mapping, and relevant tests. Do not port Markra resize controls or Lucide UI.

- [ ] **Step 4: Build the Markdown Widget with native SiYuan DOM**

The Widget DOM must have this stable shape:

```html
<span class="img cm-markra-image">
  <span>
    <img>
    <span class="protyle-action__drag"></span>
  </span>
</span>
```

Connect the handle to `startSiyuanImageResize`. Preview with DOM width only while dragging; on commit, dispatch one CodeMirror change using Markra's image-width serializer. Do not call Protyle `.sy` transactions.

- [ ] **Step 5: Verify parity and undo**

Assert handle class/DOM parity, ordinary Markdown remaining unchanged until first resize, HTML width-only updates, `max-width: 100%`, and one `undo` restoring the exact original source.

Run:

```bash
cd app && node --import tsx --test src/protyle/util/imageResize.test.ts src/markdown/markra-core/codemirror/{image-resize,image-preview}.test.ts
```

Expected: all tests pass.

### Task 7: Port Clipboard Conversion and Async Asset Insertion

**Files:**
- Create: `app/src/markdown/markra-core/clipboard-asset-types.ts`
- Create: `app/src/markdown/markra-core/codemirror/html-paste.ts`
- Create: `app/src/markdown/markra-core/codemirror/code-paste.ts`
- Create: `app/src/markdown/markra-core/codemirror/clipboard-assets.ts`
- Create: `app/src/markdown/markra-core/codemirror/clipboard-assets.test.ts`
- Modify: `app/src/markdown/clipboard.ts`
- Modify: `app/src/markdown/clipboard.test.ts`

**Interfaces:**
- Consumes: `saveClipboardAssets`, `resolveImageSource`, and error reporting from Adapter.
- Produces: `convertCodeMirrorClipboardHtml`, `codeMirrorClipboardAssetsPlugin`.

- [ ] **Step 1: Add failing clipboard fixtures**

Test all of these independent inputs:

- raw Markdown with headings, 33 tables, and Mermaid fences;
- `text/plain` plus syntax-highlighted HTML from VS Code;
- a semantic HTML table;
- rich HTML with headings, lists, links, bold, inline code, and images;
- image/file clipboard entries;
- async asset save while the user types before the original insertion point.

The long-document fixture must be loaded from `/Users/ying/Downloads/推送通知端到端技术方案.md` when available; keep a small repository fixture for deterministic CI.

Run:

```bash
cd app && node --import tsx --test src/markdown/clipboard.test.ts src/markdown/markra-core/codemirror/clipboard-assets.test.ts
```

Expected: FAIL on the current regex-only routing for at least rich HTML and async target mapping.

- [ ] **Step 2: Port Markra HTML and code-paste conversion exactly**

Use the pinned `html-paste.ts` and `code-paste.ts` behavior, including GFM table conversion, preformatted clipboard handling, line-ending normalization, and fallback to accompanying plain text.

- [ ] **Step 3: Port asset placeholders and mapped insertion targets**

Replace Tauri/Markra asset calls with `MarkdownHostAdapter.saveClipboardAssets`. Map placeholder positions through every CodeMirror transaction and verify a stale or deleted target does not insert into another document location.

- [ ] **Step 4: Remove heuristic document rewriting**

Delete `normalizeDegradedMarkdownDocument` and heading inference from the active paste path. Pasted Markdown must be inserted as authored; only line endings and proven clipboard-format degradation may be normalized by the mature converter.

- [ ] **Step 5: Run clipboard tests and source round-trip assertions**

Run the Step 1 suite.

Expected: all formats insert correctly, the long Markdown source is not reauthored, and async resources land at their captured mapped target.

### Task 8: Implement the Concrete SiYuan Adapter

**Files:**
- Create: `app/src/markdown/siyuanAdapter.ts`
- Create: `app/src/markdown/siyuanAdapter.test.ts`
- Modify: `app/src/markdown/fileActions.ts`
- Modify: `app/src/types/index.d.ts`

**Interfaces:**
- Consumes: `MarkdownHostAdapter` from Task 1 and existing SiYuan fetch/menu/icon/runtime helpers.
- Produces: `createSiyuanMarkdownAdapter(options): MarkdownHostAdapter`.

- [ ] **Step 1: Write adapter contract tests with mocked SiYuan boundaries**

Assert icon output uses existing `<svg><use xlink:href="#icon..."></use></svg>`, links use the existing open policy, errors use existing feedback, asset saves call the current Markdown resource endpoint, and resource URLs remain notebook/path safe.

Run:

```bash
cd app && node --import tsx --test src/markdown/siyuanAdapter.test.ts
```

Expected: FAIL because the concrete adapter is absent.

- [ ] **Step 2: Implement the adapter without editor semantics**

Create:

```ts
export interface SiyuanMarkdownAdapterOptions {
    notebookId: string;
    documentPath(): string;
}

export const createSiyuanMarkdownAdapter = (
    options: SiyuanMarkdownAdapterOptions
): MarkdownHostAdapter => ({/* platform implementations only */});
```

Keep Markdown parsing, range mapping, and CodeMirror dispatch out of this file.

- [ ] **Step 3: Reuse existing SiYuan render and security boundaries**

Wire Mermaid, KaTeX, code highlighting, URL safety, icons, menu placement, and error feedback through existing functions where their contracts are safe. If a current helper mutates Protyle content, wrap only its pure renderer and do not pass a Markdown Widget into `.sy` transaction code.

- [ ] **Step 4: Run adapter tests and typecheck**

Run:

```bash
cd app && node --import tsx --test src/markdown/siyuanAdapter.test.ts
cd app && pnpm run typecheck
```

Expected: adapter tests and typecheck pass.

### Task 9: Assemble Markra Core in the Existing MarkdownEditor Shell

**Files:**
- Modify: `app/src/markdown/MarkdownEditor.ts`
- Modify: `app/src/markdown/keyboard.ts`
- Modify: `app/src/markdown/keyboard.test.ts`
- Create: `app/src/markdown/markraIntegration.test.ts`

**Interfaces:**
- Consumes: `liveMarkdown`, adapter facet, clipboard-assets plugin, and `createSiyuanMarkdownAdapter`.
- Preserves: existing `MarkdownEditor` constructor, `save()`, `rename()`, `destroy()`, shell DOM, and mode controls.

- [ ] **Step 1: Write integration tests before switching**

Test one `EditorView` across visual/source/visual transitions, continuous history, `Cmd/Ctrl+A`, `Cmd/Ctrl+Z`, 800ms save scheduling, dirty-while-saving follow-up, revision conflict refresh, title rename, and destroy-time save. Snapshot the shell HTML selectors before the switch.

Run:

```bash
cd app && node --import tsx --test src/markdown/markraIntegration.test.ts src/markdown/keyboard.test.ts
```

Expected: FAIL because the Markra Core assembly is not active.

- [ ] **Step 2: Replace extension assembly only**

Keep `renderShell`, load/save/rename/status code, and public class shape. Replace `createMarkdownModeExtension`/old preview assembly with the new core, adapter, and mode compartment. Continue exposing `__markdownEditorView` in development for actual-operation tests.

- [ ] **Step 3: Remove duplicate keyboard interception**

Keep shell-level `Mod-s` only. Let CodeMirror own select-all, undo/redo, editing shortcuts, clipboard, and mode-independent commands.

- [ ] **Step 4: Run integration and existing Markdown API tests**

Run:

```bash
cd app && node --import tsx --test src/markdown/markraIntegration.test.ts src/markdown/keyboard.test.ts src/markdown/clipboard.test.ts
```

Expected: all tests pass; shell selector snapshot remains unchanged.

### Task 10: Map Markra Semantics to the Existing SiYuan Style

**Files:**
- Modify: `app/src/assets/scss/business/_markdown.scss`
- Modify: `app/scripts/testMarkdownLayout.cjs`
- Create: `app/scripts/testMarkdownUiParity.cjs`
- Modify: `app/package.json`

**Interfaces:**
- Consumes: stable semantic DOM classes emitted by Tasks 3-7.
- Produces: no TypeScript API; provides scoped layout and visual contract.

- [ ] **Step 1: Extend the Electron test with failing parity assertions**

Assert shell geometry, content width, font family, font size, line height, theme colors, table containment, code overflow, image handle position, empty-document height, and dark-mode contrast. Add an assertion that no `.markra-app`, Markra toolbar, or Lucide application button exists.

Run:

```bash
cd app && pnpm run test:markdown-layout
```

Expected: FAIL for classes introduced by the new core that are not yet mapped.

- [ ] **Step 2: Scope every editor rule under `.markdown-editor`**

Reuse SiYuan variables and `b3-typography`. Do not copy Markra app CSS or hard-code Markra colors. Preserve `min-width: 0`, local overflow for tables/code, and existing body/content width ownership.

- [ ] **Step 3: Preserve native image UI exactly**

Keep `.img` and `.protyle-action__drag` values aligned with `app/src/assets/scss/protyle/_wysiwyg.scss`. The Markdown stylesheet may provide only container-specific positioning needed by CodeMirror; handle size, colors, shadows, hover, touch behavior, and cursor must reuse the SiYuan definitions.

- [ ] **Step 4: Add and run UI parity scripts**

Add `test:markdown-ui-parity` and run:

```bash
cd app && pnpm run test:markdown-layout
cd app && pnpm run test:markdown-ui-parity
```

Expected: both Electron tests pass at desktop light/dark and 480px narrow widths.

### Task 11: Preserve Mobile Lifecycle and Touch Behavior

**Files:**
- Modify: `app/src/mobile/markdown.ts`
- Modify: `app/src/mobile/markdownState.ts`
- Modify: `app/src/mobile/markdownState.test.ts`
- Modify: `app/src/assets/scss/mobile.scss`
- Test: `app/src/markdown/markra-core/codemirror/image-preview.test.ts`

**Interfaces:**
- Consumes: the same `MarkdownEditor`, Markra Core, adapter, and native image resize helper as desktop.
- Produces: no separate mobile Markdown engine.

- [ ] **Step 1: Add failing narrow/touch tests**

Cover mobile open/back/reopen state, visual default, keyboard focus, `max-width: 100%`, always-available touch image handle, pointer cancel, and no document mutation from viewport clamping.

Run:

```bash
cd app && node --import tsx --test src/mobile/markdownState.test.ts src/markdown/markra-core/codemirror/image-preview.test.ts
```

Expected: any missing shared-core or touch behavior fails explicitly.

- [ ] **Step 2: Route mobile through the same core**

Keep mobile navigation/state ownership but remove any duplicate Markdown conversion or preview behavior. Use responsive CSS and the shared resize helper for touch differences.

- [ ] **Step 3: Run mobile tests and typecheck**

Run the Step 1 command and `cd app && pnpm run typecheck`.

Expected: mobile and typecheck pass.

### Task 12: Remove the Old Live-Preview Core

**Files:**
- Delete: `app/src/markdown/livePreview/*.ts`
- Delete: superseded `app/src/markdown/livePreview/*.test.ts`
- Modify: imports throughout `app/src/markdown/`
- Modify: `app/scripts/testMarkdownLivePreview.cjs`

**Interfaces:**
- Consumes: complete Markra Core integration from Tasks 1-11.
- Produces: exactly one Markdown editor core in the product.

- [ ] **Step 1: Prove there are no remaining consumers**

Run:

```bash
rg -n 'markdown/livePreview|from "\./livePreview|from "\.\/livePreview' app/src app/scripts
```

Expected before deletion: only known integration/test references remain; migrate each to `markra-core` first.

- [ ] **Step 2: Delete the superseded implementation**

Remove the old directory only after its required behavior has a passing Markra Core or integration test. Do not delete `MarkdownEditor.ts`, file actions, mobile lifecycle, or Go Kernel Markdown APIs.

- [ ] **Step 3: Add a single-core guard**

Update the integration test to scan `app/src/markdown` and fail if a second preview registry, second editable HTML surface, or `Lute.HTML2Md` input round-trip appears.

- [ ] **Step 4: Run all Markdown-focused tests**

Run:

```bash
cd app && node --import tsx --test src/markdown/**/*.test.ts src/mobile/markdownState.test.ts src/protyle/util/imageResize.test.ts
cd app && pnpm run test:markdown-layout
```

Expected: all focused tests pass with no old-core imports.

### Task 13: Actual Desktop Operation and Long-Document Verification

**Files:**
- Modify: `app/scripts/testMarkdownLivePreview.cjs`
- Create: `app/scripts/testMarkdownClipboard.cjs`
- Modify: `app/package.json`
- Test fixture: `/Users/ying/Downloads/推送通知端到端技术方案.md`

**Interfaces:**
- Consumes: a running QingYu Electron client with remote debugging and an open Markdown tab.
- Produces: runtime evidence for source fidelity, visible renderers, selection, paste, save, reload, and width.

- [ ] **Step 1: Extend the live client test before manual operation**

Using `window.__markdownEditorView`, assert visual default, same view across modes, `Cmd+A` full selection, undo/redo, no horizontal tab overflow, nonzero rendered table/Mermaid/image counts, and no uncaught page errors.

- [ ] **Step 2: Add a clipboard client test**

Read the reference file outside the renderer, inject clipboard-compatible plain text through the Chrome DevTools Protocol, paste into a new unnamed Markdown document, then assert the editor source equals the reference text after line-ending normalization. Verify rendered table and Mermaid counts are consistent with source counts.

- [ ] **Step 3: Run actual desktop operations**

With the user-authorized running client:

```bash
cd app && pnpm run test:markdown-live-preview
cd app && node scripts/testMarkdownClipboard.cjs /Users/ying/Downloads/推送通知端到端技术方案.md
```

Expected: both scripts pass; the tab width stays fixed; source survives visual/source switching.

- [ ] **Step 4: Exercise native image resizing**

Insert an image, resize it using the visible SiYuan handle, undo, redo, save, and reopen. Record the source before resize, after resize, after undo, after redo, and after reopen. Expected: one resize transaction, exact undo, stable persisted integer width, and UI parity with a `.sy` image.

- [ ] **Step 5: Capture UI evidence**

Capture same-size screenshots for macOS light/dark, normal tab, narrow split, long table, image selected/resizing, and mobile-width viewport. Compare shell geometry and theme values against the pre-migration baseline.

### Task 14: Frozen Integration Barrier

**Files:**
- Review all files changed by Tasks 1-13.
- Update: `app/src/markdown/markra-core/UPSTREAM.md` only if the actual source SHA or exclusion inventory differs from Task 1 evidence.

**Interfaces:**
- Consumes: the complete frozen implementation.
- Produces: final verification evidence without committing or pushing.

- [ ] **Step 1: Review scope and forbidden dependencies**

Run:

```bash
git diff --check
rg -n -g '*.ts' 'react|@tauri-apps|@markra/ai|cspell|window\.siyuan' app/src/markdown/markra-core
rg -n 'Lute\.HTML2Md|contenteditable="true".*markdown-editor__preview' app/src/markdown
```

Expected: `git diff --check` passes; forbidden core imports and editable-HTML round trips return no matches except explicit negative-test strings.

- [ ] **Step 2: Run the full frontend test and typecheck barrier**

Run:

```bash
cd app && pnpm test
cd app && pnpm run typecheck
cd app && pnpm run test:markdown-layout
cd app && pnpm run test:markdown-ui-parity
```

Expected: every command exits 0.

- [ ] **Step 3: Run repository-required lint carefully**

Record `git status --short` immediately before and after:

```bash
cd app && pnpm run lint
```

Expected: exits 0. Inspect any auto-fix diff and confirm it is limited to task-owned files; do not discard or overwrite unrelated user changes.

- [ ] **Step 4: Re-run behavior affected by lint auto-fixes**

If lint changed production Markdown files, rerun only the focused suites covering those files plus `pnpm run typecheck`. If it changed no production behavior, reuse the passing Step 2 evidence.

- [ ] **Step 5: Verify final worktree and handoff evidence**

Run:

```bash
git status --short
git diff --stat
git diff --check
```

Report the pinned Markra SHA, exact commands and results, actual desktop/mobile UI evidence, remaining unrelated dirty files, and the fact that no commit or push occurred.
