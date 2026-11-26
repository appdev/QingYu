# QingYu

[简体中文](../../README.md) · [繁體中文](README.zh-TW.md) · **English** · [日本語](README.ja.md) · [한국어](README.ko.md) · [Français](README.fr.md) · [Deutsch](README.de.md) · [Español](README.es.md) · [Português do Brasil](README.pt-BR.md) · [Italiano](README.it.md) · [Русский](README.ru.md)

QingYu is an independent, local-first Markdown notes app. Open a folder and start writing. Each note is a plain `.md` file you can open in any text editor.

[Download QingYu](https://github.com/appdev/QingYu/releases/latest)

![The same travel note on a Mac and an iPhone, with an itinerary table and a checklist](../../docs/images/hero.webp)

## Writing and organizing

- Edit headings, tables and checklists directly, or type `/` to insert a block. Read equations, Mermaid flowcharts and Markmap mind maps alongside your text.
- Keep notes in folders and use full-text search to find a line you remember. Open a daily note for today's plans or use version history to recover a deleted paragraph.
- On your phone, format text and add attachments from the toolbar above the keyboard. Copy or save an entire note or a selected passage as an image.
- Choose from 11 interface languages in **Settings → General → Language**, or follow the system language. You can also adjust the theme, body font and text size.
- Capture a quick note from the Mac menu bar. To give an AI tool access to your library, enable the MCP service in Settings.

## Storage and sync

Write locally without an account. To sync across devices, connect QingYu Cloud or your own S3-compatible storage. The native app encrypts notes and attachments before upload and keeps a conflict copy if two devices edit the same passage.

Keep your sync password safe. If you forget it and lose access to every unlocked device, you can no longer decrypt your cloud data. [Sync guide (Chinese)](../../docs/s3-sync.md)

## Installation and self-hosting

| Platform | Package |
| --- | --- |
| macOS · Apple Silicon / Intel | `aarch64.dmg` / `x64.dmg` |
| Windows · x64 | `x64-setup.exe` |
| Linux · x64 | `.deb` / `.AppImage` · WebKitGTK 4.1 |
| Android · arm64 | `.apk` |
| iOS | [Build from source](../../docs/development.md) |
| Self-hosted web | Docker · Linux amd64 / arm64 |

The macOS app is not notarized by Apple. On first launch, you may need to allow it in System Settings → Privacy & Security. The Windows installer is unsigned and may trigger a SmartScreen prompt.

The web editor reads and writes notes in a folder mounted on your server. Set an access password during deployment. [Web deployment guide (Chinese)](../../docs/api-service.md)

## Local development

Install Node.js, Rust 1.96.0 and the Tauri 2 system dependencies, then run these commands from the repository root.

```sh
npm ci
npm run tauri dev
```

[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) · [Development and release guide (Chinese)](../../docs/development.md)

## License

[GPL-3.0](../../LICENSE) · Bundled third-party components retain their own licenses. [MiaoYan](../../public/miaoyan/LICENSE) · [Markra](../../src/vendor/markra/LICENSE) · [MDSync](../../src/vendor/mdsync/LICENSE)
