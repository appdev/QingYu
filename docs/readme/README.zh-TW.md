# 輕羽 QingYu

[简体中文](../../README.md) · **繁體中文** · [English](README.en.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Français](README.fr.md) · [Deutsch](README.de.md) · [Español](README.es.md) · [Português do Brasil](README.pt-BR.md) · [Italiano](README.it.md) · [Русский](README.ru.md)

輕羽是一款獨立開發、以本機儲存為主的 Markdown 筆記應用程式。打開資料夾就能開始寫，每篇筆記都是一般的 `.md` 檔，也可以用其他編輯器繼續修改。

[下載輕羽](https://github.com/appdev/QingYu/releases/latest)

![同一篇旅行筆記在 Mac 和 iPhone 上顯示行程表與待辦清單](../../docs/images/hero.webp)

## 寫作與整理

- 直接編輯標題、表格和待辦事項，輸入 `/` 即可插入內容。公式、Mermaid 流程圖和 Markmap 心智圖都能在內文中檢視。
- 在側邊欄用資料夾整理筆記，用全文搜尋找回內文裡的一句話。用每日筆記記下行程，也能從版本記錄找回刪除的段落。
- 手機鍵盤上方的工具列可以插入附件或調整格式。分享時可將整篇筆記或選取的段落製成圖片，再複製或儲存。
- 在「設定 → 一般 → 語言」選擇 11 種介面語言之一，也可跟隨系統設定。外觀、內文字型與字級可個別調整。
- 在 Mac 選單列就能快速新增筆記。若要讓 AI 工具讀寫筆記庫，可在設定中啟用 MCP 服務。

## 筆記與同步

在本機寫作無須帳號。跨裝置同步可選擇輕羽雲或自己的 S3 相容儲存空間。原生應用程式會在上傳前加密筆記與附件；兩部裝置修改同一處時，會保留衝突副本。

請妥善保管同步密碼。若忘記密碼，又失去所有已解鎖的裝置，就無法解密雲端資料。 [同步說明（簡體中文）](../../docs/s3-sync.md)

## 安裝與自行架設

| 平台 | 安裝套件 |
| --- | --- |
| macOS · Apple Silicon / Intel | `aarch64.dmg` / `x64.dmg` |
| Windows · x64 | `x64-setup.exe` |
| Linux · x64 | `.deb` / `.AppImage` · WebKitGTK 4.1 |
| Android · arm64 | `.apk` |
| iOS | [從原始碼建置](../../docs/development.md) |
| 自行架設網頁版 | Docker · Linux amd64 / arm64 |

macOS 安裝套件未經 Apple 公證，首次開啟時可能需要在「系統設定 → 隱私權與安全性」允許開啟。Windows 安裝程式未簽署，可能出現 SmartScreen 提示。

網頁版直接讀寫伺服器上掛載的筆記資料夾。請在部署時設定存取密碼，步驟見[網頁版部署指南（簡體中文）](../../docs/api-service.md)。

## 本機開發

安裝 Node.js、Rust 1.96.0 及 Tauri 2 所需的系統套件後，在專案根目錄執行下列指令。

```sh
npm ci
npm run tauri dev
```

[Tauri 系統需求](https://v2.tauri.app/start/prerequisites/) · [開發與發佈說明（簡體中文）](../../docs/development.md)

## 授權條款

[GPL-3.0](../../LICENSE) · 第三方元件各自適用原有的授權條款。 [MiaoYan](../../public/miaoyan/LICENSE) · [Markra](../../src/vendor/markra/LICENSE) · [MDSync](../../src/vendor/mdsync/LICENSE)
