# Markra editor core provenance

- Canonical repository: `https://github.com/markrahq/markra`
- Canonical comparison target: annotated tag `v2.8.0`, peeled commit `fd14b08ba0bc2414452abdea1c38190a4d4888a1`
- Imported framework-independent snapshot: `https://github.com/appdev/QingYu` at `2b9423c5a81ba6d9c70127f72dbb16ec1fcdb1b2`
- Imported snapshot's canonical base: `22f0ebe40dc4ba8fcb653ed6c0719284ef76f361` (`v2.5.5`)
- Source paths: `packages/editor/src`, selected pure helpers from `packages/markdown/src` and `packages/shared/src`
- License: `AGPL-3.0-only`

The imported snapshot retains Markra's framework-independent CodeMirror architecture and includes the downstream editor fixes for image attributes, resizing, tables, clipboard conversion, and the removal of product AI and spellcheck features. SiYuan compatibility changes are limited to TypeScript 4.9 import syntax, local helper paths, SiYuan host adapters, and product-specific UI integration.

Editor behavior from upstream PRs `#657`, `#664`, `#669`, `#672`, `#677`, `#678`, `#679`, and `#680` is selectively ported. The tag and its commit range were not merged wholesale; each change was reconciled with the downstream editor and host adapters.

The following upstream areas are intentionally excluded: documentation, logging, AI, Tauri APIs, Windows Explorer integration, release tooling, `packages/editor-react`, React application settings and components, workspaces, sync, product themes, and product UI. Later updates must be reviewed against this exclusion list and must keep SiYuan platform behavior in the adapter layer.
