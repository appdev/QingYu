# 轻语官网 Workbench 重设计

## 状态

本设计已于 2026-07-20 获得用户批准。用户要求按照推荐方案继续执行并且后续无需询问。

本设计是 `2026-07-20-qingyu-product-site-design.md` 的视觉补充。原设计中的产品事实、双语内容、SEO、下载地址、同步边界、移动端未发布状态和独立静态应用架构继续有效；下列视觉决策取代原设计中的暖金营销导航、HTML/CSS 编辑器模型、功能卡片网格和 CSS 手机模型。

## 目标

- 让真实轻语产品界面成为官网的主要证明，而不是手工重画产品 UI；
- 消除粘性六链接导航、三列图标卡片、每节 eyebrow 和重复圆角卡片等模板化结构；
- 把官网纳入根 `design.md`，作为 QingYu 设计系统的营销站点变体；
- 保留现有双语、SSR、SEO、下载识别、移动菜单、外部链接和产品事实；
- 在 320、375、414、768 和桌面视口都无横向溢出，所有主要可点击目标适合触控。

## 受众、行动与基调

- 受众：重视本地文件、长期写作、普通 Markdown 和自主管理存储的人；
- 主要行动：下载桌面版；
- 次要行动：打开 Web 编辑器；
- 基调：克制、编辑型、可信，表现为一件真实的本地写作工具，而不是一张 SaaS 模板。

## Hallmark 结构

- Genre：editorial；
- Macrostructure：Workbench；
- Navigation：N9 Edge-aligned minimal；
- Feature voice：F5 Annotated screenshot；
- Footer：Ft2 Inline single line；
- Enrichment：真实产品截图；
- Motion：仅按钮按压、颜色变化和移动菜单状态，不做滚动揭示。

### 页面顺序

1. Edge-aligned 导航：左侧品牌，右侧只有语言与主要行动；窄屏保留可访问菜单；
2. 首屏：简短产品定位、下载/Web 入口、真实轻语预览界面截图；
3. 产品个性：用连续短文说明“不做第二大脑”；
4. 三段产品导览：分栏编辑、外观个性化、导出设置，各自使用真实截图与简短注释；
5. 数据自主：本地 Markdown、WebDAV/S3、设备之间的清楚数据路径；
6. 移动端状态：纯文字说明“即将推出”，不绘制手机外壳；
7. 下载清单：平台行而非等宽卡片，仍按当前平台优先排列；
8. 宣言与开源：保留原五行宣言和真实项目链接；
9. 单行页脚：普通链接在一条可换行但不分栏的收尾带中。

## 真实产品截图

截图由当前仓库的 `@markra/web` 在 1440×900 浅色界面中直接采集，不包含官网手工重画的浏览器、手机或 IDE 外壳：

- `apps/site/public/product-editor-light.jpg`：所见即所得写作界面；
- `apps/site/public/product-editor-split.jpg`：预览与 Markdown 源码分栏；
- `apps/site/public/product-appearance.jpg`：真实主题与配色设置；
- `apps/site/public/product-export.jpg`：真实 PDF 导出设置。

截图使用固定 `width`、`height` 和准确的双语 `alt`；首屏截图设置高优先级，折叠线以下截图懒加载。截图只允许一层细边框，不叠加伪设备框。

## 官网设计系统变体

根 `design.md` 增加 `Marketing Site Variant`，同时保持桌面应用的现有约束。

- Paper：带极少暖色的非纯白表面；
- Ink：沿用 `#1A1C1E` 对应的墨黑语义；
- Accent：交互仍使用墨黑；黄色只来自官方 Logo 资产，不建立第二个页面级主色；
- Display：`QingYu WenKai Subset`，建立与写作产品直接相关的非模板化标题气质；
- Body：系统 UI 字体，保留产品原生感；
- Radius：普通控件 6px，截图 8px；不使用大面积圆角卡片；
- Spacing：4pt 命名 scale；页面 CSS 不直接写颜色或字体族；
- Motion：120/220/420ms 三档和命名 easing；focus ring 立即出现。

官网独立 `tokens.css` 保存完整 token；`styles.css` 只引用变量。根 `design.md` 的 Exports 记录 CSS、Tailwind v4、DTCG 和 shadcn 映射。

## 交互与可访问性

- 导航没有居中链接行；桌面使用品牌、语言、Web/下载行动，手机折叠为菜单；
- 所有链接和按钮有 default、hover、focus-visible、active 状态；按钮只在按下时有 1px 位移；
- 手机主要目标最小 44×44px；所有 CTA、导航和页脚链接保持单行；
- `html`、`body` 都使用 `overflow-x: clip`，不设置页面级最小宽度；
- `h1`–`h3` 使用 `overflow-wrap: anywhere` 和 `min-width: 0`；
- `prefers-reduced-motion` 下取消空间运动；
- 移动菜单继续支持 Escape 关闭和焦点回到触发按钮。

## 测试与验收

- 先更新组件测试，让旧的假编辑器、假手机和六链接导航断言失败；
- 新测试锁定真实截图路径、尺寸、加载策略和双语 `alt`；
- 样式测试锁定 Hallmark stamp、token import、根部 clip、无默认 `ease`、无 radial gradient、无 CSS 假手机和无原始颜色漂移；
- 保留语言持久化、下载排序、外部链接、五行宣言和移动端无下载链接测试；
- 运行 site test、test typecheck、site build 和根 build；
- 在真实浏览器验证 320、375、414、768、1280 宽度，检查无横向溢出、无双行 affordance、首屏截图可见、菜单开合和控制台无错误。

## 非目标

- 不修改桌面/Web 编辑器行为；
- 不添加后台、分析、表单、账号或新依赖；
- 不创建 Android/iOS 下载入口；
- 不删除现有生产组件文件；允许原位改写其职责以维持组件所有权；
- 不提交或覆盖用户当前 README 与图标工作区改动。
