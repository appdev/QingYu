# AGENTS.md

## 通用操作按钮

以当前 PC Web 的按钮为基准。`src/buttons.css` 是 App 和 Web 共用的实现，由 `src/style.css` 统一引入。新增普通操作使用原生 `<button>`，组合 `.button`、语义样式和尺寸类。现有语义类可独立使用，维护时按需迁移。

| 尺寸 | 类名 | 高度 | 左右内边距 | 圆角 | 字号 |
| --- | --- | ---: | ---: | ---: | ---: |
| 紧凑 | 默认 | 28 px | 12 px（文字按钮为 9 px） | 10 px | 13 px |
| 常规 | `.button-regular` | 40 px | 18 px | 14 px | 15 px |
| 大 | `.button-big` | 50 px | 24 px | 14 px | 17 px |

- `.primary-button` 用于主操作，底色和文字使用 `--primary-fill`、`--primary-ink`。`.secondary-button` 用于次要操作，使用 `--sand`、`--ink`。`.quiet-button` 用于弱化操作，默认透明并使用 `--muted`，悬停或按下时使用 `--sand`、`--ink`。破坏性主操作叠加 `.destructive-button`，使用 `--button-destructive`。
- 通用字重为 `600`，图标与文字相隔 `6px`，无描边和阴影。主要和次要按钮统一由主题提供悬停、按下和禁用色。文字按钮禁用时透明度为 `0.5`。键盘焦点使用 `2px` 的 `--brand` 轮廓，外偏移 `2px`。按下缩放至 `0.97`，过渡 `120ms ease-out`，减少动态效果时关闭缩放。
- `.button-wide` 撑满容器。`.button-touch` 将触控高度补到至少 `44px`，`.button-pill` 使用胶囊形圆角。手机 sheet 的「完成」（设置、图片分享）统一使用 `button floating-button`：白色浮起胶囊、链接色粗体字、高 `44px`；浏览器居中弹窗的「完成」使用 `button secondary-button button-regular button-touch button-pill`。底部主操作使用 `button primary-button button-big button-wide`。
- 页面 CSS 管理按钮位置与操作区间距。通用颜色、圆角、尺寸及状态反馈在 `src/buttons.css` 和主题维护。系统提示框沿用统一的 `.alert-dialog` 和 `.alert-actions`，手机上按 iOS 提示框显示，两个按钮并排、取消在左；图标工具栏、分段控件和菜单行按各自控件规范维护。

## 独立功能面板

通过 `src/feature-panel.ts` 打开独立功能面板。App 在小于 `780px` 的移动视口使用底部面板，宽屏移动视口使用居中弹窗，原生桌面使用独立窗口。布局随视口变化。浏览器内的独立面板使用居中弹窗。

功能模块提供内容挂载和清理回调，允许提供自己的导航头部。容器负责弹窗尺寸、动画、拖动、点外关闭和焦点恢复。面板尺寸通过 `--feature-*` 变量配置，设置页保留分级导航，并分别响应返回一级和关闭整个面板。桌面窗口由 `src-tauri/src/feature_window.rs` 统一创建、显示和重开，各功能保留自己的页面入口与内容初始化。

## 弹出菜单

按钮下拉、右键和长按菜单统一调用 `src/popup-menu.ts` 的 `showMenu`。调用方只给菜单项和弹出位置，尺寸、平台、动画、焦点、点外关闭、键盘操作和触感都由菜单内部处理，不要在调用方或页面 CSS 里改菜单的尺寸和样式，也不要直接调用 `src/liquid-menu.ts` 或原生菜单命令。字段的完整说明见 `MenuItem` 和 `MenuOptions` 的注释。

```ts
import { showMenu, type MenuItem } from './popup-menu';

const items: MenuItem[] = [
  { text: '置顶', icon, shortcut: '⇧⌘P', run: pin },
  { text: '排序方式', items: [
    { text: '修改日期', checked: true, run: () => sort('updatedAt') },
    { text: '标题', checked: false, run: () => sort('title') },
  ] },
  { text: '' }, // 分隔线
  { text: '移到废纸篓', destructive: true, run: trash },
];
void showMenu(items, { anchor: moreButton, label: '笔记操作', onError: showError }); // 从按钮弹出
void showMenu(items, { at: { x: event.clientX, y: event.clientY }, label: '笔记操作' }); // 右键菜单
```

- 菜单项：`text` 为空字符串是分隔线；`items` 是子菜单；`checked` 给 true / false 时按勾选项朗读，同一组每项都要给；`destructive` 显示为红色；`enabled: false` 灰显；`shortcut` 只在桌面宽度显示，统一写 macOS 符号（`⇧⌘P`），Windows 和 Linux 上自动显示为 `Ctrl+Shift+P`；选文件、读写剪贴板的项要标 `needsUserActivation`。
- 选项：`anchor` 和 `at` 二选一；`keyboard` 不传时按按钮是否有键盘焦点判断；`restoreFocus` 默认把焦点还给 `anchor`；手机长按菜单用 `preview`、`previewAnchor` 和 `quickActions`。
- 选中后菜单先关闭、焦点先恢复，再执行 `run`。判断菜单是否还开着用 `popupMenuOpen()`，不要用 `showMenu` 返回的 Promise。

菜单内部按平台分流，调用方不用区分：macOS 和 iOS App 用系统菜单；Web、Android、Windows、Linux 用 `src/liquid-menu.ts` 的 Liquid Glass 菜单（移植自 `@zakisheriff/liquid-glass` 的 `GlassMenu`，动画与上游逐帧一致，代码在 `src/vendor/liquid-glass/`，类名和变量用 `qlg-` 前缀）；带预览的长按菜单和键盘上方快捷栏的菜单用旧网页菜单。桌面标题栏的应用菜单使用 `presentation: 'dropdown'`，在按钮下方打开，子菜单在父项旁边展开。表格行和列手柄的菜单在 iOS App 上仍由 `src/rich-editor.ts` 直接调用 `show_table_handle_menu`。

维护菜单组件时的内部规则：

- 尺寸在 `src/liquid-menu.ts` 的 `menuMetrics` 里按视口计算。手机档（小于 `780px`）按 iPhone 17、iOS 26.5 实测的系统菜单，以 `402px` 宽为基准乘缩放比（`0.8–1`）：面板宽 `250px`（最大为视口宽减 `16px`）、圆角 `25px`、文字 `17px`、图标 `22px`、内容内边距 `8 / 12px`、单行高 `42px`、分组间隔 `21px`（从上一行底边量到下一行顶边，包含分隔线）。平板档（`780–900px`）：宽 `236px`、圆角 `25px`、文字 `16px`、图标 `18px`、内边距 `8 / 12px`、单行高 `38px`、分组间隔 `17px`。桌面档（`901px` 起）：宽 `218px`、圆角 `12px`、文字 `13px`、图标 `14px`、内边距 `6 / 8px`、单行高 `24px`、分组间隔 `11px`，内容放不下时按内容撑开。
- 菜单从按钮所在的角落长出并盖住按钮，右键菜单从点击位置长出；子菜单从被点的那一行长出、盖住父菜单，首行是「收起××」标题行。支持按住滑选、方向键、Home / End、首字母跳转和 Esc 逐级收起。鼠标或触摸打开时焦点放在菜单本身，键盘打开时放在首个可用项。
- 桌面档按 macOS 菜单的样子，悬停或键盘选中的行铺满主题高亮色 `--brand`，文字、图标、对勾和快捷键变白；手机和平板用浅灰底。勾选标记用 `--brand`。悬停底色只给精确指针，触屏点过的行不留高亮。Android 上展开动画每帧重新生成背景模糊，修部分 WebView 停在水滴形状的问题。系统要求减少动态效果时直接开合，减少透明度时用实底。
- 桌面档打开后等鼠标离开点开时的位置才给悬停高亮；用更干脆的弹簧（约 0.2 秒落定），条目只淡入不模糊，收起时 150ms 淡出。
- 精简模式：4 核以下或内存 4GB 以下的设备，或者某次展开的前 400ms 里有 3 帧以上超过 34ms（记在 `localStorage` 的 `qingyu.menu.lite`），展开时用实底、不做背景模糊和水滴变形，只用 Web Animations 动缩放和透明度，落定后再换上半透明玻璃；收起和桌面档一样淡出。
- 笔记详情「···」显示在新窗口打开（桌面 App）、置顶或取消置顶、复制笔记、移动到…、版本历史…（笔记库支持时）和移到废纸篓，仅在移到废纸篓前保留分隔线。侧栏笔记菜单也提供移动和版本历史。废纸篓中的笔记在「···」中显示恢复笔记。
- 验证时测量打开后的菜单边界、行高和分组间隔，检查焦点、子菜单和关闭交互。Web 覆盖 `779/780px` 和 `900/901px`，App 按设备核对实包。

## 玻璃材质与动效

- Apple App 的自定义浮层使用 FluidKit Liquid Blur。浏览器和非 Apple App 的分享浮层、玻璃工具栏、开关和分段指示器使用 Glisse 的 `Glass`，设置里的下拉选项使用 Glisse 菜单样式，字号使用 Glisse 的 `Slider`。支持 SVG 背景滤镜的 Chromium 浏览器可显示实时折射，其他浏览器显示磨砂和高光。系统要求减少透明度时使用实底。
- 分享浮层沿用 FluidKit Motion。编辑器工具栏的按压、展开状态和分段指示器的位置由 FluidKit Indicator 驱动，键盘操作使用焦点轮廓。系统要求减少动态效果时直接开合。
