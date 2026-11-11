# 羽毛笔线稿候选

已选用 B，并整理为正式 SVG：`public/brand/qingyu-mark.svg`。侧边栏与单密码入口共用此文件，使用 `currentColor` 随主题变色。

使用内置 `image_gen` 生成四款透明背景的轮廓候选，尚未替换应用资源。输入身份参考为用户提供的 `/Users/evan/IdeaProjects/QingYu/macos-icon.icns`，样式参考为用户圈出的绿色简笔画图标；生成时使用经过 `image-to-webp.sh` 转换的参考图片。保留羽毛和钢笔尖，去掉黄色底板。

预览：在项目的 Vite 开发服务上打开 `/prototypes/icon-candidates/`。四个 WebP 是本次生成结果的格式转换副本，原始生成文件保留在 Codex 默认生成目录。

| 候选 | 文件 | 方向 |
| --- | --- | --- |
| A | `a-faithful.webp` | 贴近原始羽毛轮廓 |
| B | `b-minimal.webp` | 减少分叉，轮廓简练 |
| C | `c-soft.webp` | 柔和曲线 |
| D | `d-compact.webp` | 单侧分叉，更偏几何 |

这批文件用于选择造型方向，不是最终矢量资源。选定后需要整理为可随主题变色的 SVG，并针对 20–24px 调整笔画、留白和笔尖细节。

## Prompt set

Shared: Use case `logo-brand`. Image 1 provides identity: white feather quill ending in a fountain-pen nib on a yellow rounded square; use only the quill and ignore right-edge raster artifacts. Image 2 provides only the style of the green outlined bird, not its subject or UI. Exactly one centered monochrome jade/mint-green `#54C59F` quill per transparent square canvas. Feather points upper right, nib lower left. Uniform rounded strokes, clear silhouette, intended for small UI use. No tile, frame, bird, words, labels, gradient, shadow or texture.

- A: Faithful outline of the elongated original silhouette, three broad asymmetric feather barbs, clean central shaft and outlined fountain-pen nib with a short slit.
- B: Ultra-minimal pointed outer contour, only two broad barb notches, central diagonal spine continuing into a small diamond-shaped nib; avoid tiny details.
- C: Soft monoline with a curved feather, rounded outer contour, flowing barb cuts, curved central shaft and an open outlined fountain-pen nib.
- D: Compact geometric feather, strong tapered tip, two angular barb cuts, straight diagonal spine, distinct compact outlined nib and softly rounded joins.
