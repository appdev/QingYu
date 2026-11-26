# QingYu

[简体中文](../../README.md) · [繁體中文](README.zh-TW.md) · [English](README.en.md) · [日本語](README.ja.md) · **한국어** · [Français](README.fr.md) · [Deutsch](README.de.md) · [Español](README.es.md) · [Português do Brasil](README.pt-BR.md) · [Italiano](README.it.md) · [Русский](README.ru.md)

QingYu는 기기에 노트를 저장하는 독립 Markdown 노트 앱입니다. 폴더를 열고 바로 글을 쓰세요. 노트마다 일반 `.md` 파일이 생겨 다른 텍스트 편집기에서도 이어서 쓸 수 있습니다.

[QingYu 다운로드](https://github.com/appdev/QingYu/releases/latest)

![Mac과 iPhone에서 같은 여행 노트를 연 모습. 일정표와 할 일 목록이 보인다](../../docs/images/hero.webp)

## 노트 작성과 정리

- 제목, 표, 체크리스트를 바로 편집하고 `/`를 입력해 블록을 추가하세요. 수식, Mermaid 순서도, Markmap 마인드맵도 본문에서 볼 수 있습니다.
- 폴더로 노트를 정리하고 기억나는 문장을 본문에서 검색하세요. 오늘의 노트에 하루 일정을 적거나 버전 기록에서 지운 문단을 복구할 수 있습니다.
- 휴대폰에서는 키보드 위 도구 모음으로 서식을 바꾸거나 파일을 첨부할 수 있습니다. 노트 전체 또는 선택한 부분을 이미지로 복사하거나 저장할 수 있습니다.
- 설정 → 일반 → 언어에서 표시 언어 11개 중 하나를 고르거나 시스템 언어를 따르도록 설정하세요. 테마와 본문 글꼴, 글자 크기도 바꿀 수 있습니다.
- Mac 메뉴 막대에서 바로 메모를 작성할 수 있습니다. AI 도구로 노트를 읽고 쓰려면 설정에서 MCP 서비스를 켜세요.

## 저장 및 동기화

계정 없이 기기에 노트를 작성할 수 있습니다. 여러 기기에서 동기화하려면 QingYu Cloud 또는 개인 S3 호환 스토리지를 연결하세요. 네이티브 앱은 업로드 전에 노트와 첨부 파일을 암호화하고, 두 기기에서 같은 부분을 수정하면 충돌 사본을 보관합니다.

동기화 암호를 안전하게 보관하세요. 암호를 잊고 잠금을 해제한 모든 기기에 접근할 수 없게 되면 클라우드 데이터를 복호화할 수 없습니다. [동기화 안내(중국어)](../../docs/s3-sync.md)

## 설치 및 셀프 호스팅

| 플랫폼 | 패키지 |
| --- | --- |
| macOS · Apple Silicon / Intel | `aarch64.dmg` / `x64.dmg` |
| Windows · x64 | `x64-setup.exe` |
| Linux · x64 | `.deb` / `.AppImage` · WebKitGTK 4.1 |
| Android · arm64 | `.apk` |
| iOS | [소스에서 빌드](../../docs/development.md) |
| 셀프 호스팅 웹 | Docker · Linux amd64 / arm64 |

macOS 앱은 Apple 공증을 받지 않아 처음 실행할 때 시스템 설정 → 개인정보 보호 및 보안에서 허용해야 할 수 있습니다. Windows 설치 프로그램은 서명되지 않아 SmartScreen 경고가 표시될 수 있습니다.

웹 편집기는 서버에 마운트한 폴더에서 노트를 읽고 씁니다. 배포할 때 접속 암호를 설정하세요. [웹 배포 안내(중국어)](../../docs/api-service.md)

## 로컬 개발

Node.js, Rust 1.96.0과 Tauri 2에 필요한 시스템 패키지를 설치한 뒤 저장소 루트에서 다음 명령을 실행하세요.

```sh
npm ci
npm run tauri dev
```

[Tauri 설치 요구 사항](https://v2.tauri.app/start/prerequisites/) · [개발 및 릴리스 안내(중국어)](../../docs/development.md)

## 라이선스

[GPL-3.0](../../LICENSE) · 함께 배포되는 타사 구성 요소에는 각각의 라이선스가 적용됩니다. [MiaoYan](../../public/miaoyan/LICENSE) · [Markra](../../src/vendor/markra/LICENSE) · [MDSync](../../src/vendor/mdsync/LICENSE)
