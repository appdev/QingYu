<p align="center">
  <img src="logo.png" width="96" alt="QingYu logo" />
</p>

<p align="center">
  <strong>A clear desk, a quiet room—every word softly spoken.</strong>
  <br />
  <strong>Fully open source. Free to use. Your notes stay portable.</strong>
</p>

<p align="center">
  English | <a href="README.md">简体中文</a> | <a href="https://editor.markra.app/">Web Editor</a> | <a href="#download">Download</a> | <a href="#documentation">Docs</a> | <a href="#key-features">Key Features</a> | <a href="#contributing">Contributing</a> | <a href="#license">License</a>
</p>

<p align="center">
  <img alt="Desktop" src="https://img.shields.io/badge/Desktop-Tauri-24C8DB" />
  <img alt="Web Editor" src="https://img.shields.io/badge/Web-Editor-2563EB" />
  <img alt="WYSIWYG Markdown" src="https://img.shields.io/badge/Markdown-WYSIWYG-000000" />
  <img alt="Free" src="https://img.shields.io/badge/Free-Open_Source-16A34A" />
  <img alt="Downloads" src="https://img.shields.io/github/downloads/appdev/QingYu/total?label=Downloads&amp;color=0EA5E9&amp;cacheSeconds=3600" />
  <img alt="License" src="https://img.shields.io/badge/License-AGPL--3.0-important" />
</p>

QingYu is an open-source Markdown editor for simple, practical note recording. Write in a polished document view or switch to source mode, organize ordinary `.md` files and folders, and keep control of where your notes live.

No account is required. Files stay on disk by default. Desktop and mobile users can optionally synchronize their current notebook through WebDAV or S3-compatible storage.

## Manifesto

> We do not need another “second brain.”<br />
> We only need a place where we can write in peace.<br />
> Strip away complicated blocks and backlinks, and return to writing that simply flows.<br />
> Your data belongs in your S3; your inspiration belongs within you.<br />
> Here, there is only you and the quiet whisper of words.

## Download

Use the web editor at [editor.markra.app](https://editor.markra.app/).

On macOS, install with Homebrew:

```sh
brew install --cask markrahq/tap/markra
```

Download the latest desktop builds from [GitHub Releases](https://github.com/appdev/QingYu/releases/latest): macOS Apple Silicon/Intel, Windows installer/portable, and Linux AppImage.

## Documentation

- [QingYu MCP setup and security](docs/qingyu-mcp.md)
- [Changelog](CHANGELOG.md)
- [Privacy and data flow](docs/privacy.md)
- [Contributing guide](CONTRIBUTING.md)

## Desktop And Web

| Capability | Desktop app | Web editor |
| --- | --- | --- |
| WYSIWYG and source editing | Full editor experience | Full editor experience |
| Notebook and file access | Choose or switch the current notebook directory, plus standalone Markdown files | Browser file picker, folder picker, and file handles |
| File tree operations | Create, rename, move, delete, sort, reveal, and multi-select | Create, rename, move, and delete where browser permissions allow |
| Auto-save and restore | Existing files, tabs, drafts, and workspace windows | Browser file handles and IndexedDB state where available |
| Resource handling | Current-notebook `assets/`, document-local `assets/`, and existing local references | Browser handles and local references where permissions allow |
| Notes sync | Optional WebDAV and S3-compatible two-way sync for the current notebook | Not available in the web runtime |
| Export | HTML, PDF, and Pandoc formats when configured | HTML download and browser print/PDF |

## Key Features

### Markdown Editing

- Edit in WYSIWYG or source mode without changing the underlying Markdown format.
- Render links, images, HTML, KaTeX math, Mermaid diagrams, and GFM tables inline.
- Use slash commands, drag handles, visual table controls, callouts, and syntax-highlighted code blocks.
- Adjust writing width, font size, line height, themes, and keyboard shortcuts.

### Local Files And Folders

- Choose or switch one current notebook directory, then create, rename, move, delete, sort, reveal, and multi-select its files from the file tree.
- Open standalone Markdown files as unsynchronized editor documents. Choosing another directory switches the current notebook instead of creating a temporary external-folder session.
- Work with document tabs, side-by-side panes, quick open, workspace search, outline navigation, and double-bracket link completion.
- Auto-save existing files, restore tabs and workspace state, and view document or selected-text word counts.
- Keep pasted, dropped, imported, and downloaded images in an ordinary `assets/` folder when the document has a local destination.

### Sync And Export

- Optionally enable one application-wide WebDAV or S3-compatible configuration. It synchronizes only the current notebook below `notes/<directory-name>/`; switching notebooks keeps the same configuration and changes only that named remote directory.
- Opening a standalone Markdown file never changes the current notebook or synchronization target. On a new device, cloud restore lists notebook directory names and downloads only the one you select.
- Export to standalone HTML or PDF, with additional formats available through Pandoc when configured.

Synchronization settings and credentials are application-local data stored outside the notes workspace. Credentials remain plaintext on the device and are never included in QingYu synchronization. Portable preferences such as theme and layout can synchronize separately from notes, while device paths, sync state, and MCP runtime data stay local.

## Philosophy

- **Simple** — open a note and start writing without setup.
- **Practical** — file operations, search, history, sync, and export support everyday note work.
- **Local first** — notes remain ordinary Markdown files unless you explicitly enable synchronization for the current notebook.
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

1. Open the [web editor](https://editor.markra.app/) or [download](https://github.com/appdev/QingYu/releases/latest) the desktop app.
2. Choose a notebook directory, restore one named notebook from your configured cloud target, or defer setup and open a standalone Markdown file.
3. Write in the document view, or switch to source mode when you need the raw Markdown.
4. Save, export, or optionally synchronize the current notebook you control.

## Contributing

Contributions are welcome, including Markdown editing improvements, file reliability, cross-platform fixes, tests, and documentation. See [issues](https://github.com/appdev/QingYu/issues) for open work or start a discussion.

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
