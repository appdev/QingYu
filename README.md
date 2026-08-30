<p align="center">
<img alt="QingYu" src="logo.png" width="128">
<br>
<strong>QingYu</strong>
<br>
<em>QingYu · Sunlit windows, an uncluttered desk, words in a gentle voice.</em>
<br><br>
A calm, clear place to write—and truly your own.
</p>

<p align="center">
<b>English</b>
| <a href="README.zh-CN.md">中文</a>
| <a href="README.ja.md">日本語</a>
| <a href="README.tr.md">Türkçe</a>
</p>

> QingYu is an independent project built on the open-source [SiYuan](https://github.com/siyuan-note/siyuan) codebase and distributed under [AGPL-3.0](LICENSE). It is not an official SiYuan release. Its design, feature direction, releases, and support are all handled by the QingYu project.

## Why QingYu

Your notes should not become one more system demanding your attention.

QingYu lets the interface, structure, and tools recede until only the writing remains. Start with a single sentence. Let ideas find one another, gather sources around them, and allow something lasting to take shape over time—without first inventing the perfect method or entrusting your knowledge to an account.

There is no race to collect the most features here. What matters is that writing feels effortless, your material remains clear, and everything you create stays yours.

## Core experience

### Write in quiet

The block-based editor gives free-form writing a gentle sense of structure. Markdown WYSIWYG, outlines, mathematics, diagrams, and large documents are there when called for; the rest of the time, the words stay at the center.

### Let ideas meet again

Block references, backlinks, virtual references, and full-text search allow connections to surface in their own time. There is no need to build a perfect taxonomy in advance; an old thought can find its way back precisely when it matters.

### Keep sources in context

Table databases, PDF reading and annotation, web clipping, OCR, assets, and flexible import and export bring collected material back into context—where it can become part of reading, thinking, and writing.

### Shape your own workspace

Document trees, tags, bookmarks, templates, snippets, themes, icons, and plugins give the workspace room to become your own. QingYu provides a steady foundation, but never dictates a single correct way to think or take notes.

### Go further when you need to

A local API, built-in MCP Server, command-line tools, and self-hosted access leave the door open to automation and extension. They remain quietly in the background, ready when needed without crowding the everyday act of writing.

## Your data, your space

QingYu keeps your content in a local workspace of your choosing. Where your data lives, how it moves, and how it can be restored are meant to remain visible and understandable.

- Encrypted notebooks add a separate layer of protection for sensitive material.
- Local repository snapshots, history, and recovery help safeguard work accumulated over time.
- Sync through S3, WebDAV, or the local file system lets you choose and manage where copies are stored.
- The essentials work without a QingYu cloud account or any official cloud-sync service.
- The current code review found no QingYu developer-operated telemetry backend; optional network features and third-party extensions may transmit data to services you choose.
- Export to Markdown, PDF, Word, HTML, and other formats keeps your writing from being locked inside a single interface.

Privacy is not a line of copy. It is a lasting discipline, shaping every decision about accounts, networking, storage, and features.

## For those who

- Want to keep writing for years instead of endlessly rebuilding a note-taking system.
- Work with research, literature, project records, or a growing body of personal knowledge.
- Care about local data, open formats, dependable backups, and the freedom to move.
- Appreciate clear structure but do not want their tools to interrupt a train of thought.
- Like having plugins, automation, and self-hosting available when the need arises.

## Project status

QingYu remains under active development. Its feature boundaries, compatibility guarantees, and release process are still taking shape, and official distribution channels are not yet ready. SiYuan installers, app-store editions, and cloud services belong to SiYuan and should not be treated as QingYu releases or services.

For now, this repository is best suited to development, reviewing the product direction, and building from source. Recorded changes are available in the [changelog](CHANGELOG.md).

## Docker

The official Docker image is `apkdv/qingyu`. Replace the access authorization code before starting the container:

```bash
docker run -d \
  --name qingyu \
  --restart unless-stopped \
  -p 9806:9806 \
  -v /absolute/path/to/qingyu-workspace:/qingyu/workspace \
  -e PUID=1000 \
  -e PGID=1000 \
  -e QINGYU_ACCESS_AUTH_CODE=change-this-password \
  apkdv/qingyu:latest \
  /opt/qingyu/QingYu-Kernel serve
```

For Docker Compose or a container management panel, use the following complete command. The entrypoint intentionally rejects the abbreviated `serve` command and legacy kernel paths:

```text
/opt/qingyu/QingYu-Kernel serve
```

```yaml
services:
  qingyu:
    image: apkdv/qingyu:latest
    container_name: qingyu
    restart: unless-stopped
    command: ["/opt/qingyu/QingYu-Kernel", "serve"]
    ports:
      - "9806:9806"
    environment:
      PUID: "1000"
      PGID: "1000"
      QINGYU_ACCESS_AUTH_CODE: "change-this-password"
    volumes:
      - /absolute/path/to/qingyu-workspace:/qingyu/workspace
```

Set `PUID` and `PGID` to the owner of the host workspace directory. QingYu stores persistent data under `/qingyu/workspace` by default; it can be changed with `QINGYU_WORKSPACE_PATH` or the `--workspace` option.

## For developers

QingYu pairs a Go kernel with a TypeScript frontend. This README stays focused on the product; for implementation details, begin with:

- [API documentation](docs/API.md)
- [Contribution guide](.github/CONTRIBUTING.md)
- [Changelog](CHANGELOG.md)
- [Product identity design](docs/superpowers/specs/2026-08-10-qingyu-product-identity-design.md)
- [Feature-boundary design](docs/superpowers/specs/2026-08-10-feature-removal-design.md)
- macOS, Linux, and Windows build entry points under `scripts/`

Treat `kernel/go.mod`, `app/package.json`, and the project workflows as the source of truth for tool versions.

## Built on SiYuan

SiYuan's mature block editor, data format, and open-source ecosystem form QingYu's foundation. From there, QingYu follows its own product identity, feature boundaries, and approach to the everyday writing experience.

QingYu preserves the data and plugin compatibility it needs, while maintaining its own application identifiers, configuration directories, protocols, ports, kernel name, and product decisions. It is neither an official SiYuan distribution nor a representative of the SiYuan team. Questions, builds, releases, and support concerning QingYu remain the responsibility of this project.

Our thanks go to the SiYuan team, Lute and the other upstream projects, and every open-source contributor whose patient work made this foundation possible. Upstream project: [github.com/siyuan-note/siyuan](https://github.com/siyuan-note/siyuan).

## Open source and acknowledgements

QingYu is open source under the [GNU Affero General Public License v3.0](LICENSE). Any distribution or modification must continue to honor the license and preserve the copyright and attribution of the original project and its contributors.

See the [modified-version and brand notice](NOTICE.md), [Privacy Policy](docs/legal/privacy.en.md), and [User Agreement](docs/legal/terms.en.md). QingYu's website is [apkdv.com](https://apkdv.com/), and support is available at [lengyue@apkdv.com](mailto:lengyue@apkdv.com).

May every note rest a little lighter, and every thought come gently into focus.
