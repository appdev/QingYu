# QingYu Complete AI Removal Design

**Date:** 2026-07-17

**Scope amendment (2026-07-18):** The user explicitly approved deleting spellcheck completely during Task 5. This supersedes the original preservation decision; no migration or compatibility shell is required because the product has no users.

## Goal

Remove QingYu's AI functionality completely so the product remains focused on simple, practical note recording. The result must not hide AI behind a feature flag or preserve compatibility shells. It must remove the implementation, interfaces, settings, dependencies, and current product messaging that make AI part of the application.

## Product Principles

- QingYu is a focused Markdown note-taking application.
- The retained product experience prioritizes writing, recording, organizing, syncing, and exporting notes.
- QingYu does not position itself as a second brain and does not provide AI-assisted writing, chat, agents, model providers, or AI web search.
- Historical release and engineering records may continue to mention functionality that existed in older revisions.

## Scope

### Remove completely

- The `packages/ai` workspace package and all agent, model request, document tool, session, ACP, attachment, and web-search code inside it.
- The `packages/providers` workspace package and all model-provider catalogs, authentication, capability, compatibility, request, and settings code inside it.
- AI dependencies from package manifests and `pnpm-lock.yaml`, including AI SDKs, provider SDKs, agent runtimes, and libraries used only by AI web-search extraction.
- AI panels, command bars, model/provider controls, session menus, selection actions, hooks, state, stored settings, diagnostics, tests, and styling in `packages/app`.
- AI preview behavior from `packages/editor` and its dependency on `@markra/ai`.
- ACP, AI HTTP, and AI chat-attachment commands and bridges in the desktop Rust and TypeScript runtimes.
- AI implementations from the web runtime, including AI attachment storage.
- AI titlebar actions, menus, context-menu commands, keyboard shortcuts, settings categories, translations, and preferences.
- AI-specific content from the welcome document and current product, privacy, contributor, architecture, and repository-guideline documentation.
- The complete custom spellcheck product surface: editor plugin and suggestions, settings and stored preferences, languages and ignored words, dictionary fetching/cache/runtime commands, localization, tests, shortcuts, and CSpell dependencies.

### Preserve

- Markdown editing, file and folder operations, history, search, templates, export, backup, and S3 note-folder sync.
- Ordinary network features such as web image download, project sync, update checks, and their proxy configuration.
- Ordinary selection formatting: bold, italic, strikethrough, inline code, highlight, clear formatting, headings, paragraphs, quotes, lists, links, and copy.
- `CHANGELOG.md`, Git history, and existing `docs/superpowers` plans/specifications as historical records.
- The untracked `bg.png` file.

## Architecture

### Workspace packages

Delete `packages/ai` and `packages/providers` instead of leaving disabled packages or empty exports. Remove every workspace dependency on them. Remove dependencies that become unused after those packages disappear.

`packages/editor` remains a Markdown editor package. Its AI preview module and AI dependency are removed. Selection-hold behavior that is still required by the ordinary formatting toolbar is retained under neutral naming and neutral data types.

### Application composition

`packages/app` no longer constructs AI provider state, agent sessions, chat or inline prompt controllers, AI workspace change plans, or AI attachment flows. `App.tsx` and its supporting hooks expose only note-taking behavior.

The selection toolbar remains because it provides useful direct-formatting actions. It is renamed from `AiSelectionToolbar` to a neutral selection-toolbar component. AI quick actions and command-entry controls are removed from its props and rendering. Related state, hooks, CSS classes, tests, and editor plugins receive neutral names where they continue to serve ordinary selection formatting. A valid editable text selection reveals the toolbar immediately; cancelled selections, missing selections, source mode, and read-only mode keep it hidden.

The right-side AI panel and its layout reservation are removed. Desktop and Windows titlebars no longer accept AI panel state or shift controls around an AI panel.

### Settings and storage

The settings model removes:

- AI provider settings;
- ACP agent settings;
- AI agent preferences and sessions;
- AI quick-action prompts;
- AI quick-input, AI selection-action, and workspace-animation preferences;
- AI web-search settings;
- AI titlebar actions and shortcuts.
- Spellcheck enablement, language, ignored-word, dictionary, settings-category, and shortcut fields.

The ordinary selection-formatting toolbar has no persisted enablement preference. Its visibility follows the current editor selection and editability directly, so the settings schema retains no compatibility field or inactive switch for it.

Settings import and export operate only on retained settings. No migration, compatibility alias, or cleanup routine is required because the product has no users and compatibility was explicitly excluded from scope. Unknown fields in manually supplied JSON do not become supported settings and are not emitted again.

The AI, Providers, Web Search, and Spellcheck settings categories are deleted. The general Network category stays because it supports non-AI network operations. Its description is updated to list only retained consumers.

### Runtime boundaries

The shared application runtime removes AI HTTP, ACP process, and chat-attachment interfaces. Desktop and web runtime implementations remove their matching modules, commands, event listeners, storage, tests, and exports.

The desktop Rust application removes AI modules and Tauri command registration. Dependencies used only by those modules are removed from `Cargo.toml` and `Cargo.lock` through normal Cargo dependency resolution.

The spellcheck runtime, dictionary cache/fetch command, and generic web-resource request path are also deleted. `web_http.rs` and its frontend bridge remain only for ordinary web image download, retaining SSRF, redirect, size, and image content-type protections. Remote sync remains unchanged apart from any mechanical type or copy updates needed after removal.

### Menus, shortcuts, and localization

Remove AI commands from native menus, simulated Windows menus, editor context menus, command handlers, shortcut bindings, titlebar action types, and every full shortcut fixture. Keep all non-AI editing and navigation commands unchanged.

Delete AI-only and spellcheck-only localization keys and their translations from every locale. Remove the spelling-suggestion shortcut from every full shortcut fixture. Preserve generic words such as "provider" when they refer to sync providers or other retained, non-AI concepts.

### Current documentation and product copy

Rewrite the initial Markdown document and current README, product, design, privacy, contributing, and repository-guideline text so they describe the focused note-taking product. Do not rewrite `CHANGELOG.md` or prior `docs/superpowers` work records.

## Data Flow After Removal

1. The application starts and loads only retained appearance, editor, storage, backup, sync, export, network, and workspace settings.
2. A document opens into the Markdown editor without initializing model providers, agent sessions, chat attachments, AI runtimes, or a custom spellchecker.
3. Selecting valid text in the editable visual editor reveals the ordinary formatting toolbar immediately. Formatting actions operate directly on the editor selection; cancelling the selection or entering read-only mode hides the toolbar.
4. Saving writes the Markdown document through the existing file runtime. Project sync continues through the retained `SyncProvider` and remote-sync engine.
5. Settings import/export contains only retained application preferences.

No application path sends note content, prompts, attachments, or credentials to an AI service or local agent process.

## Error Handling

AI-specific errors, unsupported placeholders, disabled-feature messages, and fallback stubs are deleted with the functionality. Retained features keep their existing error handling. Removing AI must not broaden network, sync, file, or editor failure handling beyond changes required to compile and preserve current behavior.

## Testing and Acceptance

### Test maintenance

- Delete tests that only cover removed AI behavior.
- Delete tests that only cover removed spellcheck behavior; do not replace them with absence-only tests.
- Update tests and fixtures for retained components whose interfaces lose AI fields.
- Preserve or rename tests for the ordinary selection formatting toolbar and selection-hold behavior.
- Do not add tests whose only assertion is that a deleted feature is absent; verify removal through repository scans and the remaining integration surface.

### Static acceptance

- `packages/ai` and `packages/providers` do not exist.
- Package manifests and the pnpm lockfile contain no `@markra/ai`, `@markra/providers`, AI SDK, model-provider SDK, or agent-runtime dependency retained solely for AI.
- Active source contains no AI runtime, panel, provider/model settings, ACP, AI web search, AI menu, AI shortcut, or AI session implementation.
- Current product and engineering documentation contains no claim that AI is a QingYu capability.
- Historical references are limited to `CHANGELOG.md`, Git history, and existing `docs/superpowers` work records.

### Automated verification

Run:

```bash
pnpm test
pnpm typecheck:test
pnpm build
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Run focused repository scans for the removed package names, dependencies, runtime interfaces, settings keys, menu commands, and user-facing AI copy. Review every remaining hit and confirm that it is either an approved historical record or a non-AI use of a generic term.

### Live verification

Start the desktop application with `pnpm tauri dev` and confirm:

- the application launches and a Markdown note can be opened and edited;
- the selection formatting toolbar still works without AI actions;
- settings contain no AI, provider-model, or AI web-search category;
- native and Windows-style menus contain no AI commands;
- titlebars contain no AI action and reserve no AI panel space;
- sync and other retained settings remain available.

## Out of Scope

- Rewriting `CHANGELOG.md`, Git history, or prior Superpowers plans/specifications.
- Migrating or deleting stored AI data from prerelease development installations.
- Replacing AI with another assistant, recommendation system, or second-brain workflow.
- Unrelated refactors or changes to sync behavior.
- Pushing commits to any remote.
