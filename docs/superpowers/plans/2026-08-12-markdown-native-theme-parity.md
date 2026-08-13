# Markdown Native Theme Parity Implementation Plan

> **For agentic workers:** Use the global `workflow` skill's existing-plan execution entry. Review this plan against current evidence; when it is sound, enter execution directly. Only when material problems are found should `workflow` return to research, ideation, and planning to supplement this same plan before continuing. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Markdown code blocks visually and behaviorally match SiYuan native code blocks, and make every Markdown editor surface inherit the active SiYuan theme.

**Architecture:** Keep CodeMirror and Markra responsible for Markdown editing, but inject a host-owned snapshot of SiYuan code settings into the generic code-block plugin. Expand the existing computed-style bridge so CodeMirror decorations and auxiliary popovers consume native theme semantics without reusing Protyle event-hook classes. Refresh open Markdown editors from the existing editor-config apply path and let stylesheet observation handle theme and Highlight.js changes.

**Tech Stack:** TypeScript, CodeMirror 6, Markra, Lowlight/Highlight.js class names, SCSS, Node test runner with tsx, Electron runtime layout tests.

## Global Constraints

- Preserve all existing dirty-worktree changes, especially unfinished-fence behavior in `app/src/markdown/markra-core/codemirror/code-block.ts` and its integration tests.
- Do not reuse `.protyle-action__*` event-hook classes inside Markdown code-block controls.
- Use symbols from `app/appearance/icons/litheness/icon.js`; do not hand-write SVG paths.
- Do not modify generated files, `app/stage/build/**`, or `app/stage/protyle/js/lute/lute.min.js`.
- Do not change Markdown text, fenced-code parsing, Lute paste conversion, asset localization, or Mermaid source semantics.
- Do not run `pnpm build`; use Markdown tests, `pnpm run lint`, `pnpm run test:markdown-layout`, the development compiler, and the running application.
- Do not commit or push unless the user explicitly authorizes it.

---

### Task 1: Synchronize Native Code Settings

**Files:**
- Create: `app/src/markdown/codeBlockConfig.ts`
- Create: `app/src/markdown/codeBlockConfig.test.ts`
- Modify: `app/src/markdown/markraExtension.ts`
- Modify: `app/src/markdown/MarkdownEditor.ts`
- Modify: `app/src/config/tabs/editorRuntime.ts`
- Modify: `app/src/markdown/markra-core/codemirror/code-block.ts`

**Interfaces:**
- Produces: `readSiyuanCodeBlockConfig(config: Pick<Config.IEditor, "codeLineWrap" | "codeLigatures" | "codeSyntaxHighlightLineNum">): SiyuanCodeBlockConfig`.
- Produces: `MarkdownEditor.refreshEditorConfig(): void`, which reconfigures the current visual/source extension without changing the document or selection.
- Extends: `CodeBlockPreviewPluginOptions` with `lineWrap?: boolean` and `ligatures?: boolean`; `showLineNumbers` remains the line-number input.
- Consumes: `getAllModels().markdown` from `app/src/layout/getAll.ts` after `window.siyuan.config.editor` is updated.

- [ ] **Step 1: Write the failing configuration mapping test**

```ts
test("maps SiYuan code settings without forcing line numbers", () => {
    assert.deepEqual(readSiyuanCodeBlockConfig({
        codeLigatures: false,
        codeLineWrap: true,
        codeSyntaxHighlightLineNum: false,
    }), {
        ligatures: false,
        lineWrap: true,
        showLineNumbers: false,
    });
});
```

- [ ] **Step 2: Run the focused test and verify the missing module failure**

Run: `cd app && pnpm test -- src/markdown/codeBlockConfig.test.ts`

Expected: FAIL because `readSiyuanCodeBlockConfig` does not exist.

- [ ] **Step 3: Implement the host configuration snapshot**

```ts
export interface SiyuanCodeBlockConfig {
    ligatures: boolean;
    lineWrap: boolean;
    showLineNumbers: boolean;
}

export const readSiyuanCodeBlockConfig = (config: Pick<Config.IEditor,
"codeLigatures" | "codeLineWrap" | "codeSyntaxHighlightLineNum">): SiyuanCodeBlockConfig => ({
    ligatures: config.codeLigatures,
    lineWrap: config.codeLineWrap,
    showLineNumbers: config.codeSyntaxHighlightLineNum,
});
```

Pass this value from `createSiyuanMarkraExtension` into `codeBlockPreviewPlugin`. Add `data-code-line-wrap` and `data-code-ligatures` attributes to code content lines and style them with `white-space`, `word-break`, and `font-variant-ligatures`. Keep generic plugin fallbacks explicit, but never let the SiYuan host fall back to `showLineNumbers ?? true`.

- [ ] **Step 4: Add failing DOM assertions for all three settings**

Extend `app/src/markdown/codeBlockHeader.test.ts` with one enabled and one disabled case. Assert the content line attributes exactly equal the supplied values and assert that `data-code-line-number` is absent when `showLineNumbers` is false.

```ts
const line = view.dom.querySelector<HTMLElement>(".cm-markra-code-content-line");
assert.equal(line?.dataset.codeLineWrap, "false");
assert.equal(line?.dataset.codeLigatures, "true");
assert.equal(line?.hasAttribute("data-code-line-number"), false);
```

- [ ] **Step 5: Run the focused DOM test and verify it fails**

Run: `cd app && pnpm test -- src/markdown/codeBlockHeader.test.ts`

Expected: FAIL because the wrap and ligature attributes are not rendered and line-number behavior is still independently defaulted.

- [ ] **Step 6: Implement runtime refresh for open Markdown editors**

Refactor `MarkdownEditor.setPreview` so extension creation is shared by `setPreview` and a new public `refreshEditorConfig()` method. The method must dispatch only a `modeCompartment.reconfigure(...)` effect and preserve `state.doc`, `state.selection`, scroll position, dirty state, and current source/visual mode.

After `applyEditorConfig` assigns `window.siyuan.config.editor = data`, call:

```ts
getAllModels().markdown.forEach((editor) => editor.refreshEditorConfig());
```

Do not reload the Markdown document or recreate its `EditorView`.

- [ ] **Step 7: Run both focused tests and verify they pass**

Run: `cd app && pnpm test -- src/markdown/codeBlockConfig.test.ts src/markdown/codeBlockHeader.test.ts`

Expected: PASS, with explicit coverage for line numbers, wrapping, and ligatures.

---

### Task 2: Replace Browser Chrome with Native Code-Block Controls

**Files:**
- Modify: `app/src/markdown/markra-core/codemirror/code-block.ts`
- Modify: `app/src/markdown/markraExtension.ts`
- Modify: `app/src/markdown/codeBlockHeader.test.ts`
- Modify: `app/src/markdown/markra-core/code-support.ts`

**Interfaces:**
- Extends: `CodeBlockPreviewPluginOptions` with `icons?: Partial<Record<"check" | "copy" | "more", string>>`.
- Consumes: host values `#iconCheck`, `#iconCopy`, and `#iconMore` supplied by `createSiyuanMarkraExtension`.
- Preserves: the existing language `<select>` as the accessible value/input control, but makes its browser chrome visually transparent and presents a native-style language label.
- Changes: unknown languages return no highlight spans, matching native plain-text fallback.

- [ ] **Step 1: Replace the current header test with native-control expectations**

The test must assert all of the following:

```ts
assert.equal(header?.querySelector(".markra-code-language-label")?.textContent, "text");
assert.equal(select?.value, "text");
assert.equal(select?.classList.contains("markra-code-language-select"), true);
assert.equal(header?.querySelector(".markra-code-copy-icon use")?.getAttribute("href"), "#iconCopy");
assert.equal(header?.querySelector(".markra-code-copy-check-icon use")?.getAttribute("href"), "#iconCheck");
assert.equal(header?.querySelector(".markra-code-more-icon use")?.getAttribute("href"), "#iconMore");
assert.equal(header?.querySelector('[class*="protyle-action__"]'), null);
assert.equal(header?.querySelectorAll("path, rect").length, 0);
```

Also assert controls are disabled in read-only mode and that the visible language label updates after a language selection change.

- [ ] **Step 2: Run the header test and verify the structural failure**

Run: `cd app && pnpm test -- src/markdown/codeBlockHeader.test.ts`

Expected: FAIL because the widget still hand-builds SVG paths, lacks a native language label, and lacks the native more icon.

- [ ] **Step 3: Implement sprite-based controls and native header layout**

Replace `createCodeControlIcon` and its path arrays with a helper that creates only `<svg aria-hidden="true"><use href="..."></use></svg>`. Render:

- A visible `.markra-code-language-label` using the normalized language or plain-text label.
- An accessible `<select>` positioned over that label with `appearance: none`, transparent background/border, and no persistent browser field chrome.
- A copy icon button with check-state feedback.
- A more icon button that opens the same language selector through `showPicker()` where supported and focuses the selector as the fallback; this keeps every displayed action operable without inventing persistent per-block settings that Markdown cannot store.

Keep all Markdown class names outside the `.protyle-action__*` namespace.

- [ ] **Step 4: Add the unknown-language highlighter regression test**

```ts
test("treats unknown fenced languages as plain text", () => {
    assert.deepEqual(highlightMarkraCode("not-a-real-language", "const value = 1;"), []);
});
```

- [ ] **Step 5: Run the highlighter test and verify it fails because auto-detection is used**

Run: `cd app && pnpm test -- src/markdown/codeBlockHeader.test.ts`

Expected: FAIL until unknown languages stop using `lowlight.highlightAuto`.

- [ ] **Step 6: Match native plain-text fallback and complete styling**

Change `highlightMarkraCode` to return `[]` when the normalized language is empty or unregistered. Style the header and buttons with `--b3-markdown-code-*`, `--b3-theme-on-surface`, and native spacing/radius fallbacks. Controls must be visually hidden or weak at rest and visible on code-block hover, `:focus-within`, or keyboard focus.

- [ ] **Step 7: Run the focused code-block tests**

Run: `cd app && pnpm test -- src/markdown/codeBlockHeader.test.ts src/markdown/markraIntegration.test.ts`

Expected: PASS, including existing unfinished-fence and Mermaid behavior.

---

### Task 3: Expand the Computed-Style Theme Bridge

**Files:**
- Modify: `app/src/markdown/markdownThemeBridge.ts`
- Modify: `app/src/markdown/markdownThemeBridge.test.ts`
- Modify: `app/src/assets/scss/business/_markdown.scss`

**Interfaces:**
- Produces: additional `--b3-markdown-code-*`, `--b3-markdown-popover-*`, `--b3-markdown-table-*`, and state-color variables on `document.documentElement.style`.
- Preserves: reference-counted one-probe-per-document lifecycle.
- Observes: theme root attributes, theme stylesheet changes, and `#protyleHljsStyle` insertion or `href` changes through the existing head observer.

- [ ] **Step 1: Extend the bridge fixture with native code and popover semantics**

Add native-shaped probe elements for `.code-block`, `.hljs`, `.protyle-action__language`, `.b3-menu`, `.b3-menu__item`, `.table th`, and `.table td`. In the test stylesheet assign unique computed values for code background/radius/font/padding, secondary control color, menu background/border/shadow, table border, hover, primary, error, and selection semantics.

- [ ] **Step 2: Add failing assertions for the new variables**

```ts
assert.equal(rootStyle.getPropertyValue("--b3-markdown-code-background-color"), "rgb(24, 25, 26)");
assert.equal(rootStyle.getPropertyValue("--b3-markdown-code-border-radius"), "8px");
assert.equal(rootStyle.getPropertyValue("--b3-markdown-code-font-family"), "Theme Code");
assert.equal(rootStyle.getPropertyValue("--b3-markdown-popover-background-color"), "rgb(31, 32, 33)");
assert.equal(rootStyle.getPropertyValue("--b3-markdown-table-cell-border-bottom-color"), "rgb(71, 72, 73)");
```

- [ ] **Step 3: Run the bridge test and verify it fails**

Run: `cd app && pnpm test -- src/markdown/markdownThemeBridge.test.ts`

Expected: FAIL because the probes and computed properties do not yet exist.

- [ ] **Step 4: Add focused probes and computed properties**

Extend `STYLE_PROPERTIES` only with properties actually consumed by Markdown components, including `border*`, `boxShadow`, `padding*`, `opacity`, `whiteSpace`, and `wordBreak`. Split the probe definition into named native fixtures if the current template becomes hard to review, but keep all probe creation owned by `markdownThemeBridge.ts`.

Write every extracted value to a stable `--b3-markdown-<probe>-<property>` name, track it in `variables`, and remove it when the last editor releases the bridge.

- [ ] **Step 5: Add a Highlight.js stylesheet refresh assertion**

Insert a `<link id="protyleHljsStyle">`, acquire the bridge, change its `href`, call `refreshMarkdownThemeBridge(document)`, and assert the code-related variables still resolve from the current probe. This provides deterministic coverage while the existing MutationObserver supplies runtime scheduling.

- [ ] **Step 6: Map editor shell and semantic SCSS to the expanded variables**

Update `_markdown.scss` so the Markdown paper, body, headings, quote, inline code, table, selections, and active states use bridge variables first and documented SiYuan semantic variables second. Do not add new raw color literals.

- [ ] **Step 7: Run the bridge tests**

Run: `cd app && pnpm test -- src/markdown/markdownThemeBridge.test.ts`

Expected: PASS for extraction, refresh, reference counting, and cleanup.

---

### Task 4: Migrate Markdown-Only Components to Theme Tokens

**Files:**
- Create: `app/src/markdown/markdownThemeTokens.test.ts`
- Modify: `app/src/markdown/markra-core/codemirror/theme.ts`
- Modify: `app/src/markdown/markra-core/codemirror/code-block.ts`
- Modify: `app/src/markdown/markra-core/codemirror/callout-preview.ts`
- Modify: `app/src/markdown/markra-core/codemirror/selection-hold.ts`
- Modify: `app/src/markdown/markra-core/codemirror/horizontal-rule.ts`
- Modify: `app/src/markdown/markra-core/codemirror/typewriter.ts`
- Modify: `app/src/markdown/markra-core/codemirror/footnote-preview.ts`
- Modify: `app/src/markdown/markra-core/codemirror/table.ts`
- Modify: `app/src/markdown/markra-core/codemirror/search.ts`

**Interfaces:**
- Consumes: variables produced by Task 3, with fallbacks limited to existing `--b3-*` semantic variables.
- Produces: no independent Markdown color palette.
- Excludes: `app/src/markdown/markra-core/mermaid.ts` black/white contrast constants, because they are calculated SVG legibility inputs rather than editor chrome.

- [ ] **Step 1: Write a source-level theme-token guard**

The test reads the listed CodeMirror theme files and rejects `Canvas`, `CanvasText`, `#[0-9a-fA-F]{3,8}`, and `rgb(`/`rgba(` inside style declarations. It must report the file and matched literal. The allowlist contains only explanatory comments and no editor-style declarations.

```ts
const forbidden = /\bCanvas(?:Text)?\b|#[\da-f]{3,8}\b|rgba?\(/giu;
for (const file of themedFiles) {
    assert.deepEqual([...readFileSync(file, "utf8").matchAll(forbidden)], [], file);
}
```

- [ ] **Step 2: Run the guard and capture the expected failing files**

Run: `cd app && pnpm test -- src/markdown/markdownThemeTokens.test.ts`

Expected: FAIL for current callout, selection, footnote, table, search, and base-theme literals.

- [ ] **Step 3: Replace base, selection, divider, and typewriter literals**

Use `--b3-markdown-root-*`, `--b3-theme-primary`, `--b3-theme-surface`, `--b3-theme-on-surface`, `--b3-border-color`, and `--b3-list-hover`. `color-mix` may remain only when its base is one of these semantic variables.

- [ ] **Step 4: Replace code-block and search literals**

Use the native code variables from Task 3 for background, radius, font, secondary text, action hover, and line numbers. Map search matches to existing mark/highlight and primary variables so current and non-current matches remain distinguishable in both modes.

- [ ] **Step 5: Replace footnote and table popover literals**

Use `--b3-markdown-popover-background-color`, `--b3-markdown-popover-border-*`, `--b3-markdown-popover-box-shadow`, `--b3-list-hover`, and `--b3-theme-primary`. Preserve focus outlines and keyboard-visible states.

- [ ] **Step 6: Replace Callout literals with semantic theme bases**

Map neutral Callout chrome to theme variables. Map info/success/warning/error accents to existing SiYuan primary/success/warning/error variables and derive translucent backgrounds through `color-mix`; do not retain a Markra-only hex palette.

- [ ] **Step 7: Run the token guard and affected Markdown tests**

Run: `cd app && pnpm test -- src/markdown/markdownThemeTokens.test.ts src/markdown/markraIntegration.test.ts`

Expected: PASS without changing Markdown parsing or interaction behavior.

---

### Task 5: Add Deterministic Native-vs-Markdown Layout Checks

**Files:**
- Modify: `app/scripts/testMarkdownLayout.cjs`
- Modify: `app/src/markdown/codeBlockHeader.test.ts`

**Interfaces:**
- Consumes: compiled `app/src/assets/scss/base.scss` and synthetic native/Markdown code-block fixtures.
- Produces: numeric comparison evidence for container radius/background/font, language/action geometry, line-number visibility, wrapping, ligatures, and light/dark theme variables.

- [ ] **Step 1: Add paired native and Markdown code-block fixtures**

Create one `.protyle-wysiwyg .code-block .hljs` fixture and one `.cm-markra-code-line` fixture using the same semantic variables. Add light and dark fixture roots and code long enough to exercise wrapping.

- [ ] **Step 2: Add metric extraction and failing parity assertions**

Compare computed background, border radius, font family, font size, line height, action opacity rules, and line-number state. Geometry comparisons use a one-pixel tolerance:

```js
const near = (left, right) => Math.abs(left - right) <= 1;
assert.equal(metrics.markdownBackground, metrics.nativeBackground);
assert.equal(metrics.markdownRadius, metrics.nativeRadius);
assert.equal(metrics.markdownFontFamily, metrics.nativeFontFamily);
assert.ok(near(metrics.markdownActionTop, metrics.nativeActionTop));
```

- [ ] **Step 3: Run the Electron layout test and verify the new assertions fail**

Run: `cd app && pnpm run test:markdown-layout`

Expected: FAIL on current browser-style header and independent code-block theme.

- [ ] **Step 4: Adjust only token mapping and CodeMirror geometry needed for parity**

Fix discrepancies in `code-block.ts` or `_markdown.scss`; do not weaken assertions or copy raw native colors into the test subject.

- [ ] **Step 5: Run the layout test and focused unit tests**

Run: `cd app && pnpm run test:markdown-layout && pnpm test -- src/markdown/codeBlockHeader.test.ts src/markdown/markdownThemeBridge.test.ts src/markdown/markdownThemeTokens.test.ts`

Expected: PASS in both synthetic light and dark fixtures.

---

### Task 6: Full Regression and Real Application Comparison

**Files:**
- Create: `app/scripts/testMarkdownCodeBlockParity.cjs`
- Modify: `app/package.json`
- Runtime artifacts only: a temporary directory created with `mktemp -d` for screenshots; do not add screenshots to the repository unless the user requests it.

**Interfaces:**
- Consumes: a running development app with Electron remote debugging enabled and one open native document plus one open Markdown document containing identical fenced code.
- Produces: a JSON metrics report and paired screenshots for each tested theme/configuration combination.

- [ ] **Step 1: Add a CDP parity script without document mutation**

Follow `app/scripts/testMarkdownLivePreview.cjs` connection code. Locate the active native `.protyle-wysiwyg .code-block` and Markdown `.cm-markra-code-line`, read computed styles and bounding rectangles, then capture clipped screenshots through `Page.captureScreenshot`. The script must fail with an actionable message when either comparison fixture is not open.

- [ ] **Step 2: Add the package script**

```json
"test:markdown-code-parity": "node ./scripts/testMarkdownCodeBlockParity.cjs"
```

- [ ] **Step 3: Run the complete automated regression suite**

Run: `cd app && pnpm test`

Expected: all Markdown and existing application unit tests PASS.

- [ ] **Step 4: Run project-required lint**

Run: `cd app && pnpm run lint`

Expected: PASS with no new warnings or errors.

- [ ] **Step 5: Compile through the approved development route**

Run the repository's existing development command used by the current app session. Do not run `pnpm build`. Wait for a successful compile and record the exact command and final compiler output.

- [ ] **Step 6: Launch or reuse the app with remote debugging and prepare equal fixtures**

Use the same code content, language, font size, theme, and editor settings in a native document and Markdown document. Include known language, unknown language, empty block, long line, and the user-provided pasted Markdown sample. Any document creation or paste must be confined to the test workspace selected by the user or an existing disposable test document.

- [ ] **Step 7: Run the real parity matrix**

Run: `cd app && pnpm run test:markdown-code-parity -- --output <temporary-directory>`

Cover default light, default dark, and one installed third-party theme when available. For each, test line numbers on/off, wrapping on/off, and ligatures on/off; pairwise combinations are acceptable provided every setting is observed in both states.

- [ ] **Step 8: Perform interaction checks in the running application**

Verify language selection, copy/check feedback, more control, code editing, undo, paste, source/visual mode switching, theme switching, and setting changes without reopening the document. Confirm unfinished fences remain source and Mermaid preview still opens and exits correctly.

- [ ] **Step 9: Freeze and report evidence**

Run `git diff --check`, record `git status --short`, report the automated commands and outcomes, list screenshot paths, and identify any third-party-theme limitation. Do not claim visual parity if runtime comparison could not be completed.
