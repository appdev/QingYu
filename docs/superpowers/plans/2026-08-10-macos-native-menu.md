# macOS 原生应用菜单实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为轻语提供跟随界面语言、面向当前焦点工作空间并保留安全退出语义的 macOS 原生应用菜单。

**Architecture:** 使用可独立测试的 CommonJS 模块定义主进程菜单状态校验和 Electron 菜单模板；主进程按 `webContents.id` 缓存状态并负责焦点路由，渲染进程只提交受约束的标签、快捷键和只读状态，并复用现有命令分发。非 macOS 继续使用当前最小菜单模板。

**Tech Stack:** Electron 42、Node.js CommonJS、TypeScript 4.9、Node Test Runner、现有 IPC 和 i18n JSON。

## Global Constraints

- 只完善 macOS Application Menu，不实现 Linux/KDE 全局菜单。
- 保留顶栏 HTML 主菜单以及全部富菜单、插件菜单和上下文菜单。
- 菜单语言跟随轻语界面语言，不跟随 macOS 系统语言。
- 不新增依赖，不修改 `app/stage/build/**` 等生成目录。
- 不运行 `pnpm build`、`pnpm dev` 或编译/重启内核。
- 前端最终验证使用 `cd app && pnpm run lint`。
- 保留用户在目标文件中的现有品牌和功能删减改动，不提交、不推送。

---

### Task 1: 可测试的主进程菜单模型

**Files:**
- Create: `app/electron/nativeMenu.js`
- Create: `app/src/util/nativeMenu.test.ts`

**Interfaces:**
- Produces: `sanitizeNativeMenuState(value): NativeMenuState | undefined`
- Produces: `createApplicationMenuTemplate({platform, productName, state, dispatch, hotKey2Electron}): MenuItemConstructorOptions[]`
- Produces: `NATIVE_MENU_COMMANDS: ReadonlySet<string>`，只包含 `config`、`newFile`、`recentDocs`、`dataHistory`、`goBack`、`goForward`、`globalSearch`、`commandPanel`、`userGuide`、`feedback` 和 `debug`
- `NativeMenuState` 只含 `ready: boolean`、`readonly: boolean`、固定键集合的 `labels: Record<string, string>` 与固定命令集合的 `accelerators: Record<string, string>`

- [ ] **Step 1: 编写失败测试，固定状态校验边界**

测试构造包含额外属性、非字符串标签、超长文本和未知快捷键的状态，断言只接受完整、固定键、有限长度的快照；无效快照返回 `undefined`，不会把任意菜单结构带入主进程。

```ts
import assert from "node:assert/strict";
import {describe, it} from "node:test";

const {sanitizeNativeMenuState} = require("../../electron/nativeMenu.js");

it("rejects malformed state and strips unknown fields", () => {
    assert.equal(sanitizeNativeMenuState({ready: "yes"}), undefined);
    const state = sanitizeNativeMenuState(validState({unknown: "ignored"}));
    assert.equal("unknown" in state, false);
});
```

- [ ] **Step 2: 编写失败测试，固定 macOS 菜单结构**

断言 `darwin` 模板依次包含应用、文件、编辑、显示、窗口、帮助六组；写操作受 `readonly` 控制；业务项点击只调用 `dispatch(command)`；退出使用 `role: "quit"`；编辑、缩放、全屏和窗口系统操作使用 Electron role。

```ts
it("builds the localized macOS menu and dispatches allowlisted commands", () => {
    const commands: string[] = [];
    const template = createApplicationMenuTemplate({
        platform: "darwin",
        productName: "QingYu",
        state: validState(),
        dispatch: (command: string) => commands.push(command),
        hotKey2Electron: (key: string) => key,
    });
    assert.deepEqual(template.map((item: {label?: string}) => item.label), ["QingYu", "文件", "编辑", "显示", "窗口", "帮助"]);
    findById(template, "newFile").click();
    assert.deepEqual(commands, ["newFile"]);
    assert.equal(findByRole(template, "quit").role, "quit");
});
```

- [ ] **Step 3: 编写失败测试，固定非 macOS 兼容行为**

断言 `linux` 与 `win32` 模板仍只包含当前应用、编辑、窗口三组，不包含轻语业务命令。

- [ ] **Step 4: 运行聚焦测试确认失败**

Run: `cd app && pnpm exec tsx --test src/util/nativeMenu.test.ts`

Expected: FAIL，因为 `electron/nativeMenu.js` 尚不存在。

- [ ] **Step 5: 实现最小菜单模型**

`sanitizeNativeMenuState` 必须重新构造对象而不是返回输入引用；标签限制为固定键且单项不超过 200 个字符；快捷键只保留命令白名单中的字符串。`createApplicationMenuTemplate` 不接收渲染进程模板或 role，所有结构和 click 回调都在该模块内定义。

- [ ] **Step 6: 运行聚焦测试确认通过**

Run: `cd app && pnpm exec tsx --test src/util/nativeMenu.test.ts`

Expected: PASS。

### Task 2: 菜单专用 i18n 资源

**Files:**
- Modify: `app/appearance/langs/*.json`

**Interfaces:**
- Produces: 每种语言对象顶部的 `_nativeMenu`，固定包含 `file`、`view`、`window`、`services`、`hideOthers`、`showAll`、`minimize`、`bringAllToFront`
- Consumes: Task 3 的渲染进程快照直接读取 `window.siyuan.languages._nativeMenu`

- [ ] **Step 1: 在所有语言文件顶部增加 `_nativeMenu`**

为 21 个语言文件使用下表的确定文本，不复制英文占位；所有字符串使用 ASCII 三点号规则，且本组不需要省略号。

| 语言 | file | view | window | services | hideOthers | showAll | minimize | bringAllToFront |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| ar | ملف | عرض | نافذة | الخدمات | إخفاء الآخرين | إظهار الكل | تصغير | إحضار الكل إلى المقدمة |
| de | Ablage | Darstellung | Fenster | Dienste | Andere ausblenden | Alle einblenden | Im Dock ablegen | Alle nach vorne |
| en | File | View | Window | Services | Hide Others | Show All | Minimize | Bring All to Front |
| es | Archivo | Visualización | Ventana | Servicios | Ocultar otros | Mostrar todo | Minimizar | Traer todo al frente |
| fr | Fichier | Présentation | Fenêtre | Services | Masquer les autres | Tout afficher | Réduire | Tout ramener au premier plan |
| he | קובץ | תצוגה | חלון | שירותים | הסתר אחרים | הצג הכל | מזער | הבא הכל לחזית |
| hi | फ़ाइल | दृश्य | विंडो | सेवाएँ | अन्य छिपाएँ | सभी दिखाएँ | छोटा करें | सभी को सामने लाएँ |
| id | File | Tampilan | Jendela | Layanan | Sembunyikan Lainnya | Tampilkan Semua | Minimalkan | Bawa Semua ke Depan |
| it | File | Vista | Finestra | Servizi | Nascondi altre | Mostra tutte | Contrai | Porta tutto in primo piano |
| ja | ファイル | 表示 | ウインドウ | サービス | ほかを隠す | すべてを表示 | しまう | すべてを手前に移動 |
| ko | 파일 | 보기 | 윈도우 | 서비스 | 기타 가리기 | 모두 보기 | 최소화 | 모두 앞으로 가져오기 |
| nl | Archief | Weergave | Venster | Voorzieningen | Verberg andere | Toon alles | Minimaliseer | Breng alles naar voren |
| pl | Plik | Widok | Okno | Usługi | Ukryj pozostałe | Pokaż wszystkie | Minimalizuj | Umieść wszystkie na wierzchu |
| pt-BR | Arquivo | Visualizar | Janela | Serviços | Ocultar Outros | Mostrar Tudo | Minimizar | Trazer Tudo para a Frente |
| ru | Файл | Вид | Окно | Службы | Скрыть остальные | Показать все | Свернуть | Все окна — вперед |
| sk | Súbor | Zobraziť | Okno | Služby | Skryť ostatné | Zobraziť všetky | Minimalizovať | Preniesť všetko dopredu |
| th | ไฟล์ | มุมมอง | หน้าต่าง | บริการ | ซ่อนแอปอื่น | แสดงทั้งหมด | ย่อหน้าต่าง | นำทั้งหมดมาไว้ด้านหน้า |
| tr | Dosya | Görüntü | Pencere | Servisler | Diğerlerini Gizle | Tümünü Göster | Simge Durumuna Küçült | Tümünü Öne Getir |
| uk | Файл | Перегляд | Вікно | Служби | Сховати інші | Показати всі | Згорнути | Перемістити всі вікна на передній план |
| zh-CN | 文件 | 显示 | 窗口 | 服务 | 隐藏其他 | 全部显示 | 最小化 | 前置全部窗口 |
| zh-TW | 檔案 | 顯示 | 視窗 | 服務 | 隱藏其他 | 全部顯示 | 最小化 | 將所有視窗移到最前面 |

- [ ] **Step 2: 校验语言键完整性和 JSON 语法**

Run: `python scripts/check-lang-keys.py`

Expected: 所有语言键结构一致，命令退出码为 0。

### Task 3: 主进程状态缓存、焦点切换与命令路由

**Files:**
- Modify: `app/electron/main.js`
- Modify: `app/src/constants.ts`

**Interfaces:**
- Consumes: Task 1 的 `sanitizeNativeMenuState`、`createApplicationMenuTemplate`
- Produces: IPC `siyuan-native-menu-state`，渲染进程向主进程提交状态
- Produces: IPC `siyuan-native-menu-command`，主进程向当前焦点工作空间派发白名单命令
- Produces: `nativeMenuStates: Map<number, NativeMenuState>`

- [ ] **Step 1: 添加 IPC 常量**

在 `Constants` 的桌面 IPC 常量区域增加 `SIYUAN_NATIVE_MENU_STATE` 与 `SIYUAN_NATIVE_MENU_COMMAND`，不复用通用 `siyuan-cmd`。

```ts
public static readonly SIYUAN_NATIVE_MENU_STATE = "siyuan-native-menu-state";
public static readonly SIYUAN_NATIVE_MENU_COMMAND = "siyuan-native-menu-command";
```

- [ ] **Step 2: 用菜单模块替换内联模板**

在 `main.js` 引入 Task 1 模块。窗口创建时调用统一模板函数；`darwin` 使用当前窗口状态或未就绪默认状态，其他平台生成与当前等价的最小模板。

- [ ] **Step 3: 实现安全状态接收**

注册 `ipcMain.on("siyuan-native-menu-state", ...)`。只有 `event.sender.id` 对应 `workspaces` 中的主窗口且 `sanitizeNativeMenuState` 成功时才更新 Map；若该窗口当前获得焦点，则立即重建菜单。

- [ ] **Step 4: 实现焦点和销毁清理**

主工作空间窗口 `focus` 时根据自身快照重建菜单；窗口从 `workspaces` 移除或渲染内容销毁时删除相应 Map 项，避免旧工作空间状态泄漏到新窗口。

- [ ] **Step 5: 实现当前窗口命令派发**

菜单业务项点击时从 `BrowserWindow.getFocusedWindow()` 解析有效工作空间，只向其 `webContents` 发送 `siyuan-native-menu-command`。无有效焦点窗口、窗口已销毁或命令不在白名单时静默返回。

- [ ] **Step 6: 静态检查主进程语法**

Run: `node --check app/electron/nativeMenu.js && node --check app/electron/main.js`

Expected: 两个文件均无语法错误。

### Task 4: 渲染进程状态桥和业务命令执行

**Files:**
- Create: `app/src/boot/nativeMenu.ts`
- Modify: `app/src/boot/onGetConfig.ts`
- Modify: `app/src/boot/globalEvent/command/global.ts`

**Interfaces:**
- Produces: `initNativeMenu(app: App): void`
- Consumes: `Constants.SIYUAN_NATIVE_MENU_STATE` 与 `Constants.SIYUAN_NATIVE_MENU_COMMAND`
- Consumes: `globalCommand(command, app)`；新增 `commandPanel`、`userGuide` 和 `feedback` 的明确执行路径

- [ ] **Step 1: 构造并发送状态快照**

`initNativeMenu` 仅在 `window.siyuan.config.system.os === "darwin"` 执行。快照标签来自现有语言键和 `_nativeMenu`，快捷键来自 `window.siyuan.config.keymap.general`，只发送 Task 1 白名单命令；`readonly` 直接来自配置。

```ts
export const initNativeMenu = (app: App) => {
    if (window.siyuan.config.system.os !== "darwin") {
        return;
    }
    ipcRenderer.send(Constants.SIYUAN_NATIVE_MENU_STATE, createNativeMenuState());
    bindNativeMenuCommandsOnce(app);
};
```

- [ ] **Step 2: 监听并分发原生菜单命令**

监听 `siyuan-native-menu-command`，先验证命令属于本地固定集合，再执行：常规命令调用 `globalCommand`；`commandPanel` 打开命令面板；`userGuide` 调用 `mountHelp`；`feedback` 根据当前语言打开 `ld246.com` 或 `liuyun.io`；未知命令不执行。

- [ ] **Step 3: 接入配置初始化**

在 `onGetConfig` 完成语言、配置和快捷键修正后调用一次 `initNativeMenu(app)`。监听器必须避免同一页面重复初始化时重复注册，可使用模块级布尔标记；每次调用仍重新发送最新状态。

- [ ] **Step 4: 保持命令模块无循环依赖**

`globalCommand` 只补充可安全复用且不会引入 `panel.ts -> global.ts -> panel.ts` 循环的命令。命令面板、帮助和反馈由 `nativeMenu.ts` 直接调用，不反向导入到 `global.ts`。

- [ ] **Step 5: 运行 TypeScript 类型检查**

Run: `cd app && pnpm run typecheck`

Expected: 退出码 0。

### Task 5: 集成验证与交付检查

**Files:**
- Verify: `app/electron/nativeMenu.js`
- Verify: `app/electron/main.js`
- Verify: `app/src/boot/nativeMenu.ts`
- Verify: `app/src/boot/onGetConfig.ts`
- Verify: `app/appearance/langs/*.json`

**Interfaces:**
- Consumes: Tasks 1–4 的最终文件状态
- Produces: 可复核的测试、lint 和差异证据

- [ ] **Step 1: 运行菜单聚焦测试**

Run: `cd app && pnpm exec tsx --test src/util/nativeMenu.test.ts`

Expected: PASS。

- [ ] **Step 2: 运行全部前端单元测试**

Run: `cd app && pnpm test`

Expected: 全部测试 PASS。

- [ ] **Step 3: 运行语言完整性检查**

Run: `python scripts/check-lang-keys.py`

Expected: 退出码 0。

- [ ] **Step 4: 运行项目规定的前端验证**

Run: `cd app && pnpm run lint`

Expected: 类型检查和 ESLint 均退出码 0。检查 lint 的自动修复差异，只接受本任务相关格式变化。

- [ ] **Step 5: 检查差异边界**

Run: `git diff --check -- app/electron/nativeMenu.js app/electron/main.js app/src/constants.ts app/src/boot/nativeMenu.ts app/src/boot/onGetConfig.ts app/src/boot/globalEvent/command/global.ts app/src/util/nativeMenu.test.ts app/appearance/langs docs/superpowers`

Expected: 无空白错误；差异不覆盖工作区已有品牌、功能删减或其他用户修改。

- [ ] **Step 6: 记录 macOS 手工验证项**

交付中明确列出尚需开发者在 macOS 运行态验证的内容：六组菜单展示、语言切换、两工作空间焦点切换、只读禁用、Protyle 撤销/重做、快捷键单次执行和安全退出。不得为了本验证启动或重启开发者正在运行的 Electron/内核实例。
