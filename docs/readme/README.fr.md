# QingYu

[简体中文](../../README.md) · [繁體中文](README.zh-TW.md) · [English](README.en.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · **Français** · [Deutsch](README.de.md) · [Español](README.es.md) · [Português do Brasil](README.pt-BR.md) · [Italiano](README.it.md) · [Русский](README.ru.md)

QingYu est une application indépendante de prise de notes en Markdown qui enregistre vos textes sur votre appareil. Ouvrez un dossier et commencez à écrire. Chaque note est un fichier `.md` que vous pouvez modifier avec l’éditeur de votre choix.

[Télécharger QingYu](https://github.com/appdev/QingYu/releases/latest)

![Une même note de voyage sur Mac et iPhone, avec un itinéraire et une liste de tâches](../../docs/images/hero.webp)

## Rédiger et classer ses notes

- Modifiez directement les titres, tableaux et listes de tâches, ou tapez `/` pour insérer un bloc. Les formules, les diagrammes Mermaid et les cartes mentales Markmap s’affichent dans le texte.
- Classez vos notes dans des dossiers et retrouvez un passage grâce à la recherche en texte intégral. Consignez votre programme dans la note du jour ou récupérez un paragraphe supprimé dans l’historique des versions.
- Sur téléphone, la barre au-dessus du clavier permet de mettre en forme le texte et de joindre un fichier. Vous pouvez copier ou enregistrer une note entière ou un passage sous forme d’image.
- Dans Réglages → Général → Langue, choisissez parmi 11 langues ou utilisez celle du système. Le thème, la police et la taille du texte sont également réglables.
- Sur Mac, prenez une note rapide depuis la barre des menus. Pour permettre à un outil d’IA de lire et de modifier vos notes, activez le service MCP dans les réglages.

## Stockage et synchronisation

Vous pouvez écrire sur votre appareil sans compte. Pour synchroniser plusieurs appareils, connectez QingYu Cloud ou votre propre stockage compatible S3. L’application native chiffre les notes et les pièces jointes avant l’envoi. Si deux appareils modifient le même passage, elle conserve une copie en cas de conflit.

Conservez votre mot de passe de synchronisation. Si vous l’oubliez et perdez l’accès à tous vos appareils déverrouillés, vous ne pourrez plus déchiffrer les données stockées en ligne. [Guide de synchronisation (en chinois)](../../docs/s3-sync.md)

## Installation et auto-hébergement

| Plateforme | Paquet |
| --- | --- |
| macOS · Apple Silicon / Intel | `aarch64.dmg` / `x64.dmg` |
| Windows · x64 | `x64-setup.exe` |
| Linux · x64 | `.deb` / `.AppImage` · WebKitGTK 4.1 |
| Android · arm64 | `.apk` |
| iOS | [Compiler à partir des sources](../../docs/development.md) |
| Web auto-hébergé | Docker · Linux amd64 / arm64 |

L’application macOS n’est pas notariée par Apple. Au premier lancement, il peut être nécessaire de l’autoriser dans Réglages Système → Confidentialité et sécurité. Sous Windows, le programme d’installation n’est pas signé et peut déclencher une alerte SmartScreen.

L’éditeur web lit et écrit les notes dans un dossier monté sur votre serveur. Définissez un mot de passe d’accès lors du déploiement. [Guide de déploiement web (en chinois)](../../docs/api-service.md)

## Développement local

Installez Node.js, Rust 1.96.0 et les dépendances système de Tauri 2, puis exécutez ces commandes à la racine du dépôt.

```sh
npm ci
npm run tauri dev
```

[Prérequis de Tauri](https://v2.tauri.app/start/prerequisites/) · [Guide de développement et de publication (en chinois)](../../docs/development.md)

## Licence

[GPL-3.0](../../LICENSE) · Les composants tiers inclus conservent leurs licences respectives. [MiaoYan](../../public/miaoyan/LICENSE) · [Markra](../../src/vendor/markra/LICENSE) · [MDSync](../../src/vendor/mdsync/LICENSE)
