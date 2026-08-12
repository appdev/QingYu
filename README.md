<p align="center">
<img alt="QingYu" src="logo.png" width="128">
<br>
<strong>QingYu</strong>
<br>
<em>QingYu · Clear windows, a quiet desk, words softly spoken.</em>
<br><br>
A quiet, clear writing space that remains under your control.
</p>

<p align="center">
<b>English</b>
| <a href="README.zh-CN.md">中文</a>
| <a href="README.ja.md">日本語</a>
| <a href="README.tr.md">Türkçe</a>
</p>

> QingYu is based on the open-source project [SiYuan](https://github.com/siyuan-note/siyuan) and follows [AGPL-3.0](LICENSE). It is not an official SiYuan distribution; QingYu's product design, feature choices, releases, and support are maintained independently.

## Why QingYu

Notes should not become another system you have to maintain.

QingYu keeps the interface, structure, and tools in their proper place so your attention can return to the words. Begin with a sentence, connect ideas as they grow, organize source material, and build knowledge over time—without designing a perfect method first or handing your work to an account system.

QingYu is not a contest to collect the most features. It cares about whether writing feels natural, whether your material stays understandable, and whether your knowledge remains yours.

## Core experience

### Write without noise

The block-based editor combines free-form writing with visible structure. Markdown WYSIWYG, outlines, mathematics, diagrams, and large documents are available when needed and quiet when they are not.

### Let ideas meet again

Block references, backlinks, virtual references, and full-text search help connections emerge naturally. You do not need a perfect taxonomy before writing; earlier ideas can return when they become relevant again.

### Keep sources in context

Table databases, PDF reading and annotation, web clipping, OCR, assets, and flexible import and export turn collected material into something you can think and write with.

### Shape your own workspace

Document trees, tags, bookmarks, templates, snippets, themes, icons, and plugins offer room to adapt. QingYu provides a stable foundation without prescribing one correct way to take notes.

### Go further when you need to

A local API, built-in MCP Server, command-line tools, and self-hosted access leave space for automation and extension. These capabilities sit behind the product rather than in the middle of everyday writing.

## Your data, your space

QingYu stores content in a local workspace you choose and aims to keep its data boundaries understandable, portable, and recoverable.

- Encrypted notebooks provide separate protection for sensitive material.
- Local repository snapshots, history, and recovery help preserve long-term work.
- S3, WebDAV, and local-file-system sync let you choose and manage the storage provider.
- Core features do not require a QingYu cloud account or an official cloud-sync service.
- QingYu does not proactively send usage behavior, diagnostics, installation events, device identifiers, or similar telemetry.
- Markdown, PDF, Word, HTML, and other export paths help keep content from being trapped in one interface.

Privacy is not a tagline. It is an ongoing constraint on accounts, networking, storage, and product decisions.

## Made for

- People who want to write for years instead of repeatedly rebuilding a note-taking system.
- Researchers organizing literature, source material, projects, and evolving ideas.
- Knowledge workers who value local data, open formats, backups, and migration freedom.
- Writers who appreciate visible structure but do not want tools interrupting thought.
- Users who may extend their workspace with plugins, automation, or self-hosting.

## Project status

QingYu is under active development while its product boundaries, compatibility, and release process continue to stabilize. Official distribution channels are still being prepared. SiYuan's official installers, app-store editions, and cloud services are not QingYu releases or services.

This repository is currently intended for development, product review, and source builds. See the [changelog](CHANGELOG.md) for recorded changes.

## For developers

QingYu combines a Go kernel with a TypeScript frontend, but this README deliberately stays at product level. Start with these references when you need implementation details:

- [API documentation](docs/API.md)
- [Contribution guide](.github/CONTRIBUTING.md)
- [Changelog](CHANGELOG.md)
- [Product identity design](docs/superpowers/specs/2026-08-10-qingyu-product-identity-design.md)
- [Feature-boundary design](docs/superpowers/specs/2026-08-10-feature-removal-design.md)
- macOS, Linux, and Windows build entry points under `scripts/`

Use the tool versions recorded in `kernel/go.mod`, `app/package.json`, and the project workflows.

## Built on SiYuan

QingYu builds on SiYuan's mature block editor, data format, and open-source ecosystem while reshaping product identity, feature boundaries, and everyday experience.

QingYu retains necessary data and plugin compatibility, but it has independent application identifiers, configuration directories, protocols, ports, kernel naming, and product decisions. It is not an official SiYuan distribution and does not represent the SiYuan team. QingYu issues, builds, releases, and support are the responsibility of the QingYu project.

We are grateful to the SiYuan team, Lute, the wider upstream ecosystem, and every open-source contributor whose work made this foundation possible. Upstream project: [github.com/siyuan-note/siyuan](https://github.com/siyuan-note/siyuan).

## Open source and acknowledgements

QingYu is distributed under the [GNU Affero General Public License v3.0](LICENSE). Distributions and modifications must continue to follow the license and preserve the copyright and attribution of the original project and its contributors.

May every note feel a little lighter, and every thought become a little clearer.
