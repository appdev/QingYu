<p align="center">
  <img src="logo.png" width="96" alt="QingYu logo" />
</p>

<p align="center">
  <strong>A clear desk, a quiet room—every word softly spoken.</strong>
  <br />
  <strong>Fully open source. Free to use. Your notes stay portable.</strong>
</p>

<p align="center">
  English | <a href="README.md">简体中文</a> | <a href="https://editor.markra.app/">Web</a> | <a href="#download">Download</a> | <a href="#documentation">Docs</a> | <a href="#key-features">Key Features</a> | <a href="#contributing">Contributing</a> | <a href="#license">License</a>
</p>

<p align="center">
  <img alt="Desktop" src="https://img.shields.io/badge/Desktop-Tauri-24C8DB" />
  <img alt="Web" src="https://img.shields.io/badge/Web-Notes-2563EB" />
  <img alt="WYSIWYG Markdown" src="https://img.shields.io/badge/Markdown-WYSIWYG-000000" />
  <img alt="Free" src="https://img.shields.io/badge/Free-Open_Source-16A34A" />
  <img alt="Downloads" src="https://img.shields.io/github/downloads/appdev/QingYu/total?label=Downloads&amp;color=0EA5E9&amp;cacheSeconds=3600" />
  <img alt="License" src="https://img.shields.io/badge/License-AGPL--3.0-important" />
</p>

QingYu is an open-source Markdown notes app for simple, practical recording. Write in a polished document view, switch to source mode on desktop and the browser-file runtime, and keep your notes as ordinary `.md` files. Desktop and browser file handles let you decide where files live; mobile and Server Web use their own fixed managed workspaces.

Desktop, mobile, and Server Web are backed by the Kernel for documents, settings, history, resources, and sync. The public browser-file runtime requires no QingYu account and focuses on browser file handles; Server Web/Docker is the rebuilt single-user notes service with an initialization token, owner password, and one persistent `/data` workspace.

## Manifesto

> We do not need another “second brain.”<br />
> We only need a place where we can write in peace.<br />
> Strip away complicated blocks and backlinks, and return to writing that simply flows.<br />
> Your data belongs in your S3; your inspiration belongs within you.<br />
> Here, there is only you and the quiet whisper of words.

## Download

Use the browser-file runtime at [editor.markra.app](https://editor.markra.app/).

For the self-hosted Server Web/Docker runtime, start with the [single-user Docker deployment guide](deploy/docker/README.md). It runs the same Web app and Kernel API, fixes data under `/data/workspace`, `/data/config`, and `/data/state`, and is intended for deploying QingYu as a single-user web notes service.

On macOS, install with Homebrew:

```sh
brew install --cask markrahq/tap/markra
```

Download the latest desktop builds from [GitHub Releases](https://github.com/appdev/QingYu/releases/latest): macOS, Windows, and Linux packages. Linux builds include AppImage, DEB, RPM, and Arch Linux x64 packages.

On Arch Linux, download the x64 package from the release page and run:

```sh
sudo pacman -U ./QingYu_<version>_linux_x64.pkg.tar.zst
```

## Documentation

- User-facing guides:
  - [Privacy and data flow](docs/privacy.md)
  - [QingYu MCP setup and security](docs/qingyu-mcp.md)
  - [Theme authoring guide and boundaries](docs/theme-authoring.md)
  - [Default ZhenKai theme design case study](docs/default-theme-zhenkai.md)
- Development and release:
  - [Contributing guide](CONTRIBUTING.md)
  - [Single-user Docker deployment](deploy/docker/README.md)
  - [Kernel runtime migration status](docs/kernel-runtime-migration-status.md)
  - [Changelog](CHANGELOG.md)

## Runtime Matrix

| Capability | Desktop app | Mobile app | Browser-file runtime | Server Web/Docker |
| --- | --- | --- | --- | --- |
| Product shape | Local-first desktop notes app | Native mobile notes app | Lightweight file-note entry in the browser | Single-user self-hosted web notes service |
| Editing surface | Full desktop layout, WYSIWYG, source/split modes, tabs, and panes | Compact full-screen editor, mobile formatting toolbar, system Back, and keyboard safe-area handling | Browser editor with responsive layout | Full notes workspace reached through a browser |
| Workspace model | Choose or switch a local notebook directory, plus standalone Markdown files | Fixed application-managed workspace; no local directory picker, standalone file window, or remote notebook catalog | Browser file picker, folder picker, and file handles | One deployment serves one user, fixed to `/data/workspace`, with no directory picker or switching |
| Permissions and sign-in | Native app permissions | Mobile app sandbox permissions | No QingYu account; depends on browser-granted file handles | Initialization token sets the owner password; later access uses same-origin cookie sessions |
| File tree operations | Create, rename, move, delete, sort, reveal, and multi-select | Create, rename, move, delete, search, and full-screen file browsing | Create, rename, move, and delete where browser permissions allow | Kernel manages Markdown files, history, and search inside `/data/workspace` |
| Auto-save and restore | Kernel AppConfig restores files, tabs, drafts, and workspace windows | Embedded Kernel AppConfig restores the managed workspace, current document, and drafts across mobile lifecycle events | Browser file handles and IndexedDB state where available | Kernel AppConfig writes `/data/config/settings.json`; refreshes and different browsers restore the same state |
| Images and attachments | Images go into notebook or document-local `assets/`; local attachments and containing folders can be opened | System image picker imports images into managed-workspace `assets/`; non-image local attachments are not opened | Browser handles and local references where permissions allow | Images and resources are served by Kernel from `/data/workspace`; the browser is not the file authority |
| Notes sync | WebDAV and S3-compatible sync for the selected notebook, with desktop cloud notebook restore | WebDAV and S3-compatible sync for the fixed managed workspace; no local root picker or remote notebook switching | Not available in the browser-file runtime | WebDAV and S3-compatible sync for fixed `/data/workspace` |
| Export | HTML, PDF, portable Markdown with attachments, and more Pandoc formats when configured | The current mobile runtime does not provide export or Pandoc | HTML download and browser print/PDF | Browser-side export is available; Pandoc is not provided |
| Desktop-only integration | MCP, native Settings window, menus/shortcuts, updater, system fonts, and Pandoc | Not provided | Not provided | Not provided; server operations are handled through Docker/Kernel configuration |

## Key Features

### Markdown Notes

- Desktop and the browser-file runtime can switch between WYSIWYG and source editing; mobile and Server Web provide document views designed around the notes workspace. The underlying files remain Markdown.
- Render links, images, HTML, KaTeX math, Mermaid diagrams, and GFM tables inline.
- Use slash commands, drag handles, visual table controls, callouts, and syntax-highlighted code blocks.
- Adjust writing width, font size, line height, themes, and keyboard shortcuts.

### Desktop Files And Workspaces

- Choose or switch one current notebook directory, then create, rename, move, delete, sort, reveal, and multi-select its files from the file tree.
- Open standalone Markdown files as unsynchronized editor documents. Choosing another directory switches the current notebook instead of creating a temporary external-folder session.
- Work with document tabs, side-by-side panes, quick open, workspace search, outline navigation, double-bracket link completion, the independent Settings window, and desktop menus.
- Auto-save existing files, restore tabs and workspace state, and view document or selected-text word counts.
- Keep pasted, dropped, imported, and downloaded images in an ordinary `assets/` folder when the document has a local destination.

### Mobile Workspace

- Android and iOS use an embedded Kernel and one fixed application-managed workspace. They do not expose a local directory picker, standalone file windows, desktop MCP, system font selection, Pandoc, or the desktop updater.
- Use compact full-screen Editor, Files, Settings, and Sync screens. System Back, page hide, foreground/background transitions, and keyboard safe areas are handled by the mobile runtime.
- Create, rename, move, delete, and search Markdown files inside the managed workspace, and restore older versions through document history.
- Import images from the system picker into managed-workspace `assets/`; imported images synchronize as ordinary workspace files. Non-image local attachments are not opened directly on mobile.

### Web And Self-Hosted Service

- The browser-file runtime remains a lightweight entry point: no QingYu account, browser file/folder authorization, and IndexedDB state for what the browser can remember.
- Server Web/Docker is the rebuilt web notes service: one deployment serves one user, first use requires an initialization token to set the owner password, and later browsers connect to Kernel through same-origin cookie sessions.
- Server Web does not treat browser storage as authoritative. Settings, open files, drafts, layout, and synchronization configuration are committed by Kernel under `/data/config`, so refreshes, another browser, or a replacement container attached to the same volume all read the same state.
- Server Web has no local directory picker, standalone file windows, MCP, system fonts, or Pandoc. It fixedly manages `/data/workspace` and talks to Kernel over same-origin HTTP/WebSocket.

### Sync And Export

- Desktop can optionally enable one application-wide WebDAV or S3-compatible configuration. It synchronizes only the current notebook below `notes/<directory-name>/`; switching notebooks keeps the same configuration and changes only that named remote directory.
- Mobile uses the same Kernel sync engine, but the sync target is always the fixed managed workspace. It does not expose a local workspace selector, remote root selector, or remote notebook catalog.
- Server Web uses the same Kernel sync engine, but the sync target is always `/data/workspace`; the browser-file runtime does not provide QingYu sync.
- Opening a desktop standalone Markdown file never changes the current notebook or synchronization target. On a new desktop device, cloud restore lists notebook directory names and downloads only the one you select.
- Desktop exports to standalone HTML, PDF, or portable Markdown with attachments, with additional formats available through Pandoc when configured. Mobile currently does not provide export or Pandoc, and Server Web does not provide Pandoc.

Synchronization settings and credentials are application-local data stored outside the notes workspace. Credentials remain plaintext on the device and are never included in QingYu synchronization. Portable preferences such as theme and layout can synchronize separately from notes, while device paths, sync state, and MCP runtime data stay local.

## Philosophy

- **Simple** — open a note and start writing without setup.
- **Practical** — file operations, search, history, sync, and runtime-available export support everyday note work.
- **Local first** — unless you explicitly enable synchronization for the current desktop folder, mobile managed workspace, or Server Web persistent volume, notes remain local Markdown files.
- **Portable** — no proprietary document format or hosted workspace is required.

## Selected Slogans

### Literary And Minimal

- “A clear desk, a quiet room—every word softly spoken.”
- “Leave complex formatting to the poetry of instant rendering.”

### Geek And Unburdened

- “No second brain. No patchwork. Today, just write a page or two.”
- “Your notes belong in your own storage bucket (S3).”

### Across Desktop And Mobile

- “Craft at your desk. Capture in your palm.”

## Getting Started

1. Open the [Web runtime](https://editor.markra.app/), [download](https://github.com/appdev/QingYu/releases/latest) the desktop app, or deploy Server Web from the [Docker guide](deploy/docker/README.md).
2. On desktop, choose a notebook directory, restore one named directory from your configured cloud target, or defer setup and open a standalone Markdown file. On mobile, start in the application-managed workspace. On Server Web, initialize the owner and enter `/data/workspace`.
3. Write in the document view; desktop and the browser-file runtime can switch to source mode when needed.
4. Save, export, or synchronize the notebook workspace supported by the current runtime.

## Contributing

Contributions are welcome, including Markdown editing improvements, file reliability, cross-platform polish, synchronization, export, themes, MCP, tests, and documentation. Start with the [contributing guide](CONTRIBUTING.md) for pnpm workspace commands, testing boundaries, and release notes.

## Contributors

Thanks to everyone who has helped shape QingYu through code, documentation, design, testing, and feedback.

<p align="center">
  <a href="https://github.com/appdev/QingYu/graphs/contributors">
    <img src="https://contrib.rocks/image?repo=appdev/QingYu" alt="QingYu contributors" />
  </a>
</p>

## Sponsors

[![Sponsors](https://raw.githubusercontent.com/murongg/sponsorskit/main/public/sponsors.svg)](https://sponsors.mrong.me/)

## Star History

<p align="center">
  <a href="https://star-history.com/#appdev/QingYu&Date">
    <img alt="QingYu star history chart" src="https://api.star-history.com/svg?repos=appdev/QingYu&type=Date" />
  </a>
</p>

## License

QingYu is licensed under AGPL-3.0.
