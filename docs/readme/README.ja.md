# QingYu

[简体中文](../../README.md) · [繁體中文](README.zh-TW.md) · [English](README.en.md) · **日本語** · [한국어](README.ko.md) · [Français](README.fr.md) · [Deutsch](README.de.md) · [Español](README.es.md) · [Português do Brasil](README.pt-BR.md) · [Italiano](README.it.md) · [Русский](README.ru.md)

QingYu は、端末内にノートを保存する独立開発の Markdown ノートアプリです。フォルダーを開くだけで書き始められます。ノートは通常の `.md` ファイルなので、ほかのエディターでも編集できます。

[QingYu をダウンロード](https://github.com/appdev/QingYu/releases/latest)

![Mac と iPhone で開いた旅行ノート。旅程表とチェックリストが並んでいる](../../docs/images/hero.webp)

## 書く・整理する

- 見出しや表、チェックリストを直接編集できます。`/` でブロックを挿入でき、数式や Mermaid のフローチャート、Markmap のマインドマップも本文中で確認できます。
- ノートをフォルダーで整理し、覚えている一文から全文検索できます。今日の予定はデイリーノートに書き、消してしまった段落は変更履歴から復元できます。
- スマートフォンでは、キーボード上のツールバーで書式を変更したり、ファイルを添付したりできます。ノート全体や選択した部分を画像としてコピー・保存できます。
- 「設定 → 一般 → 言語」で 11 の表示言語から選択するか、システムの言語に合わせられます。テーマ、本文のフォントと文字サイズも変更できます。
- Mac のメニューバーからすぐにメモを取れます。AI ツールからノートを読み書きするには、設定で MCP サービスを有効にしてください。

## 保存と同期

端末内で書くのにアカウントは不要です。端末間の同期には QingYu Cloud またはご自身の S3 互換ストレージを利用できます。ネイティブアプリはアップロード前にノートと添付ファイルを暗号化し、同じ箇所の編集が競合した場合はコピーを残します。

同期パスワードは大切に保管してください。パスワードを忘れ、ロックを解除したすべての端末にもアクセスできなくなると、クラウド上のデータを復号できません。 [同期ガイド（中国語）](../../docs/s3-sync.md)

## インストールとセルフホスティング

| プラットフォーム | パッケージ |
| --- | --- |
| macOS · Apple Silicon / Intel | `aarch64.dmg` / `x64.dmg` |
| Windows · x64 | `x64-setup.exe` |
| Linux · x64 | `.deb` / `.AppImage` · WebKitGTK 4.1 |
| Android · arm64 | `.apk` |
| iOS | [ソースからビルド](../../docs/development.md) |
| セルフホスト版 Web | Docker · Linux amd64 / arm64 |

macOS 版は Apple の公証を受けていないため、初回起動時に「システム設定 → プライバシーとセキュリティ」で許可が必要な場合があります。Windows 版のインストーラーは署名されておらず、SmartScreen の警告が表示されることがあります。

Web 版はサーバーにマウントしたフォルダー内のノートを読み書きします。デプロイ時にアクセス用パスワードを設定してください。 [Web 版の導入ガイド（中国語）](../../docs/api-service.md)

## ローカルでの開発

Node.js、Rust 1.96.0、Tauri 2 に必要なシステムパッケージをインストールし、リポジトリのルートで次のコマンドを実行します。

```sh
npm ci
npm run tauri dev
```

[Tauri の前提条件](https://v2.tauri.app/start/prerequisites/) · [開発・リリースガイド（中国語）](../../docs/development.md)

## ライセンス

[GPL-3.0](../../LICENSE) · 同梱のサードパーティ製コンポーネントには、それぞれのライセンスが適用されます。 [MiaoYan](../../public/miaoyan/LICENSE) · [Markra](../../src/vendor/markra/LICENSE) · [MDSync](../../src/vendor/mdsync/LICENSE)
