# QingYu

[简体中文](../../README.md) · [繁體中文](README.zh-TW.md) · [English](README.en.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Français](README.fr.md) · **Deutsch** · [Español](README.es.md) · [Português do Brasil](README.pt-BR.md) · [Italiano](README.it.md) · [Русский](README.ru.md)

QingYu ist eine unabhängige Markdown-Notiz-App, die deine Notizen lokal speichert. Öffne einen Ordner und schreib los. Jede Notiz ist eine gewöhnliche `.md`-Datei, die du auch in anderen Texteditoren bearbeiten kannst.

[QingYu herunterladen](https://github.com/appdev/QingYu/releases/latest)

![Dieselbe Reisenotiz auf Mac und iPhone mit Reiseplan und Checkliste](../../docs/images/hero.webp)

## Notizen schreiben und ordnen

- Bearbeite Überschriften, Tabellen und Checklisten direkt im Text. Mit `/` fügst du einen Block ein. Formeln, Mermaid-Flussdiagramme und Markmap-Mindmaps erscheinen direkt in der Notiz.
- Ordne deine Notizen in Ordnern und finde Textstellen über die Volltextsuche wieder. Halte deine Pläne in der Tagesnotiz fest oder hole einen gelöschten Absatz aus dem Versionsverlauf zurück.
- Auf dem Smartphone kannst du über die Leiste oberhalb der Tastatur Text formatieren und Dateien anhängen. Kopiere oder speichere eine ganze Notiz oder einen ausgewählten Abschnitt als Bild.
- Unter Einstellungen → Allgemein → Sprache stehen elf Oberflächensprachen zur Auswahl. Du kannst auch die Systemsprache übernehmen und Design, Schriftart und Schriftgröße anpassen.
- Auf dem Mac kannst du direkt über die Menüleiste eine Notiz anlegen. Wenn ein KI-Werkzeug deine Notizen lesen und bearbeiten soll, aktiviere in den Einstellungen den MCP-Dienst.

## Speicherung und Synchronisierung

Zum lokalen Schreiben brauchst du kein Konto. Für den Abgleich zwischen Geräten kannst du QingYu Cloud oder einen eigenen S3-kompatiblen Speicher anbinden. Die native App verschlüsselt Notizen und Anhänge vor dem Hochladen und legt bei widersprüchlichen Änderungen am selben Absatz eine Konfliktkopie an.

Bewahre dein Synchronisierungspasswort gut auf. Wenn du es vergisst und den Zugriff auf alle entsperrten Geräte verlierst, lassen sich deine Daten in der Cloud nicht mehr entschlüsseln. [Anleitung zur Synchronisierung (Chinesisch)](../../docs/s3-sync.md)

## Installation und Selbsthosting

| Plattform | Paket |
| --- | --- |
| macOS · Apple Silicon / Intel | `aarch64.dmg` / `x64.dmg` |
| Windows · x64 | `x64-setup.exe` |
| Linux · x64 | `.deb` / `.AppImage` · WebKitGTK 4.1 |
| Android · arm64 | `.apk` |
| iOS | [Aus dem Quellcode bauen](../../docs/development.md) |
| Selbst gehostete Webversion | Docker · Linux amd64 / arm64 |

Die macOS-App ist nicht von Apple notarisiert. Beim ersten Start kann eine Freigabe unter Systemeinstellungen → Datenschutz & Sicherheit nötig sein. Das Windows-Installationsprogramm ist unsigniert und kann einen SmartScreen-Hinweis auslösen.

Der Webeditor liest und schreibt Notizen in einem eingebundenen Ordner auf deinem Server. Lege bei der Einrichtung ein Zugangspasswort fest. [Anleitung zur Webbereitstellung (Chinesisch)](../../docs/api-service.md)

## Lokale Entwicklung

Installiere Node.js, Rust 1.96.0 und die Systemabhängigkeiten von Tauri 2. Führe anschließend im Stammverzeichnis des Repositorys diese Befehle aus.

```sh
npm ci
npm run tauri dev
```

[Systemvoraussetzungen für Tauri](https://v2.tauri.app/start/prerequisites/) · [Entwicklung und Veröffentlichung (Chinesisch)](../../docs/development.md)

## Lizenz

[GPL-3.0](../../LICENSE) · Für die mitgelieferten Drittanbieterkomponenten gelten die jeweiligen Lizenzen. [MiaoYan](../../public/miaoyan/LICENSE) · [Markra](../../src/vendor/markra/LICENSE) · [MDSync](../../src/vendor/mdsync/LICENSE)
