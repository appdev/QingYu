# Markra editor core provenance

- Canonical repository: `https://github.com/markrahq/markra`
- Canonical comparison target: `a5dec440fb3ae7c33fcd714be85f03b82814e5ef` (`v2.5.6`)
- Imported framework-independent snapshot: `https://github.com/appdev/QingYu` at `2b9423c5a81ba6d9c70127f72dbb16ec1fcdb1b2`
- Imported snapshot's canonical base: `22f0ebe40dc4ba8fcb653ed6c0719284ef76f361` (`v2.5.5`)
- Source paths: `packages/editor/src`, selected pure helpers from `packages/markdown/src` and `packages/shared/src`
- License: `AGPL-3.0-only`

The imported snapshot retains Markra's framework-independent CodeMirror architecture and includes the downstream editor fixes for image attributes, resizing, tables, clipboard conversion, and the removal of product AI and spellcheck features. SiYuan compatibility changes are limited to TypeScript 4.9 import syntax, local helper paths, SiYuan host adapters, and product-specific UI integration.

The following upstream areas are intentionally excluded: `packages/editor-react`, React application components, Tauri APIs, AI preview and selection features, custom spellcheck, Markra workspaces, sync, themes, and product UI. Later updates must be reviewed against this exclusion list and must keep SiYuan platform behavior in the adapter layer.
