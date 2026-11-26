# QingYu

[简体中文](../../README.md) · [繁體中文](README.zh-TW.md) · [English](README.en.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Français](README.fr.md) · [Deutsch](README.de.md) · [Español](README.es.md) · **Português do Brasil** · [Italiano](README.it.md) · [Русский](README.ru.md)

O QingYu é um aplicativo independente de notas em Markdown que salva seus textos no próprio dispositivo. Abra uma pasta e comece a escrever. Cada nota é um arquivo `.md` comum que você pode abrir em qualquer editor de texto.

[Baixar o QingYu](https://github.com/appdev/QingYu/releases/latest)

![A mesma nota de viagem em um Mac e em um iPhone, com um roteiro e uma lista de tarefas](../../docs/images/hero.webp)

## Escrever e organizar notas

- Edite títulos, tabelas e listas de tarefas diretamente ou digite `/` para inserir um bloco. Fórmulas, fluxogramas do Mermaid e mapas mentais do Markmap aparecem no corpo da nota.
- Organize suas notas em pastas e use a busca no texto completo para encontrar uma frase de que se lembra. Registre seus planos na nota do dia ou recupere um parágrafo apagado pelo histórico de versões.
- No celular, use a barra acima do teclado para formatar o texto e anexar arquivos. Copie ou salve como imagem a nota inteira ou apenas o trecho selecionado.
- Em Configurações → Geral → Idioma, escolha um dos 11 idiomas da interface ou acompanhe o idioma do sistema. Você também pode ajustar o tema, a fonte e o tamanho do texto.
- No Mac, crie uma nota rápida pela barra de menus. Para permitir que uma ferramenta de IA leia e edite suas notas, ative o serviço MCP nas configurações.

## Armazenamento e sincronização

Você pode escrever no dispositivo sem criar uma conta. Para sincronizar vários dispositivos, conecte o QingYu Cloud ou seu próprio armazenamento compatível com S3. O aplicativo nativo criptografa as notas e os anexos antes do envio e guarda uma cópia em caso de edições conflitantes no mesmo trecho.

Guarde sua senha de sincronização. Se você a esquecer e perder o acesso a todos os dispositivos desbloqueados, não será mais possível descriptografar os dados na nuvem. [Guia de sincronização (em chinês)](../../docs/s3-sync.md)

## Instalação e hospedagem própria

| Plataforma | Pacote |
| --- | --- |
| macOS · Apple Silicon / Intel | `aarch64.dmg` / `x64.dmg` |
| Windows · x64 | `x64-setup.exe` |
| Linux · x64 | `.deb` / `.AppImage` · WebKitGTK 4.1 |
| Android · arm64 | `.apk` |
| iOS | [Compilar a partir do código-fonte](../../docs/development.md) |
| Web em servidor próprio | Docker · Linux amd64 / arm64 |

O aplicativo para macOS não passou pela notarização da Apple. Na primeira abertura, pode ser necessário autorizá-lo em Ajustes do Sistema → Privacidade e Segurança. O instalador do Windows não é assinado e pode gerar um aviso do SmartScreen.

O editor web lê e grava notas em uma pasta montada no servidor. Defina uma senha de acesso durante a implantação. [Guia de implantação web (em chinês)](../../docs/api-service.md)

## Desenvolvimento local

Instale o Node.js, o Rust 1.96.0 e as dependências de sistema do Tauri 2. Em seguida, execute estes comandos na raiz do repositório.

```sh
npm ci
npm run tauri dev
```

[Pré-requisitos do Tauri](https://v2.tauri.app/start/prerequisites/) · [Guia de desenvolvimento e lançamento (em chinês)](../../docs/development.md)

## Licença

[GPL-3.0](../../LICENSE) · Os componentes de terceiros mantêm suas respectivas licenças. [MiaoYan](../../public/miaoyan/LICENSE) · [Markra](../../src/vendor/markra/LICENSE) · [MDSync](../../src/vendor/mdsync/LICENSE)
