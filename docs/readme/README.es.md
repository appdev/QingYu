# QingYu

[简体中文](../../README.md) · [繁體中文](README.zh-TW.md) · [English](README.en.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Français](README.fr.md) · [Deutsch](README.de.md) · **Español** · [Português do Brasil](README.pt-BR.md) · [Italiano](README.it.md) · [Русский](README.ru.md)

QingYu es una aplicación independiente de notas en Markdown que guarda tus textos en el dispositivo. Abre una carpeta y empieza a escribir. Cada nota se guarda como un archivo `.md` que puedes abrir en cualquier editor de texto.

[Descargar QingYu](https://github.com/appdev/QingYu/releases/latest)

![La misma nota de viaje en un Mac y un iPhone, con un itinerario y una lista de tareas](../../docs/images/hero.webp)

## Escribir y organizar notas

- Edita títulos, tablas y listas de tareas directamente, o escribe `/` para insertar un bloque. Las fórmulas, los diagramas de flujo de Mermaid y los mapas mentales de Markmap se muestran en el texto.
- Organiza las notas en carpetas y usa la búsqueda de texto completo para encontrar una frase que recuerdes. Anota tus planes en la nota del día o recupera un párrafo borrado desde el historial de versiones.
- En el móvil, usa la barra sobre el teclado para dar formato al texto y adjuntar archivos. Puedes copiar o guardar como imagen una nota entera o solo el pasaje seleccionado.
- En Ajustes → General → Idioma puedes elegir entre 11 idiomas de interfaz o usar el idioma del sistema. También puedes cambiar el tema, la fuente y el tamaño del texto.
- En el Mac puedes crear una nota rápida desde la barra de menús. Para que una herramienta de IA pueda leer y modificar tus notas, activa el servicio MCP en los ajustes.

## Almacenamiento y sincronización

Puedes escribir sin cuenta en tu dispositivo. Para sincronizar varios dispositivos, conecta QingYu Cloud o tu propio almacenamiento compatible con S3. La aplicación nativa cifra las notas y los archivos adjuntos antes de subirlos. Si dos dispositivos modifican el mismo pasaje, conserva una copia en conflicto.

Guarda la contraseña de sincronización. Si la olvidas y pierdes el acceso a todos los dispositivos desbloqueados, ya no podrás descifrar tus datos en la nube. [Guía de sincronización (en chino)](../../docs/s3-sync.md)

## Instalación y alojamiento propio

| Plataforma | Paquete |
| --- | --- |
| macOS · Apple Silicon / Intel | `aarch64.dmg` / `x64.dmg` |
| Windows · x64 | `x64-setup.exe` |
| Linux · x64 | `.deb` / `.AppImage` · WebKitGTK 4.1 |
| Android · arm64 | `.apk` |
| iOS | [Compilar desde el código fuente](../../docs/development.md) |
| Web en servidor propio | Docker · Linux amd64 / arm64 |

La aplicación para macOS no está certificada mediante el servicio de notarización de Apple. Al abrirla por primera vez, puede que tengas que autorizarla en Ajustes del Sistema → Privacidad y seguridad. El instalador de Windows no está firmado y puede activar una advertencia de SmartScreen.

El editor web lee y escribe notas en una carpeta montada en tu servidor. Define una contraseña de acceso al desplegarlo. [Guía de despliegue web (en chino)](../../docs/api-service.md)

## Desarrollo local

Instala Node.js, Rust 1.96.0 y las dependencias del sistema de Tauri 2. Después, ejecuta estos comandos desde la raíz del repositorio.

```sh
npm ci
npm run tauri dev
```

[Requisitos de Tauri](https://v2.tauri.app/start/prerequisites/) · [Guía de desarrollo y publicación (en chino)](../../docs/development.md)

## Licencia

[GPL-3.0](../../LICENSE) · Los componentes de terceros incluidos conservan sus respectivas licencias. [MiaoYan](../../public/miaoyan/LICENSE) · [Markra](../../src/vendor/markra/LICENSE) · [MDSync](../../src/vendor/mdsync/LICENSE)
