# Desktop visual editor resources

Vditor is pinned to npm `vditor@4.0.1` (MIT). Its editor JavaScript and CSS are bundled by Vite. The files under `dist/js/` are unmodified copies of that npm package's `lute/`, `katex/`, `icons/ant.js`, and `i18n/zh_CN.js` assets; keep them in sync when upgrading the package.

The bundled Lute build is distributed under MulanPSL-2.0. Its compiled Go/GopherJS runtime notices are retained alongside it. KaTeX and its fonts use MIT; mhchem uses Apache-2.0. Vditor's diff-match-patch dependency uses Apache-2.0. Full license texts are included in this directory.

Only the assets used by this application are copied. Visual mode reuses the original highlighting, Mermaid, Markmap and PlantUML encoder resources under `/miaoyan/`, with the preview's theme configuration. Vditor retains source/display interaction. Markmap labels are sanitized before drawing; PlantUML sends diagram source to `https://www.plantuml.com/plantuml/svg/` and displays an image, with a code fallback on failure. The local Markmap loader configuration prevents relative dependency requests outside the bundled asset paths. No remote editor CDN, extra draft cache, or relaxed script policy is enabled.
