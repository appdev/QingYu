# QingYu Brand Voice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the legacy “Refactor your thinking” brand system with “轻语 · 明窗净几，字字轻语” and align application, metadata, and README introductions around a clear, quiet, user-controlled writing space.

**Architecture:** Keep `slogan` as the localized phrase without the product name, then compose product name, separator, and slogan at each application presentation surface. Treat the Chinese phrase as the semantic source and maintain explicit localized values in the existing language resources and Electron pre-boot dictionary.

**Tech Stack:** TypeScript, Electron HTML/CSS/JavaScript, JSON i18n resources, Markdown, Node.js built-in test runner, Python language-key validator, pnpm lint.

## Global Constraints

- Chinese master brand line is exactly `轻语 · 明窗净几，字字轻语`.
- Chinese `slogan` values are exactly `明窗净几，字字轻语`; non-Chinese values are natural localizations and retain `QingYu` only where the presentation surface needs the full line.
- Do not make privacy promises not proven by the existing product behavior.
- Do not edit generated paths listed in `AGENTS.md`; do not run `pnpm build` or `pnpm dev`.
- Preserve every unrelated dirty-worktree change.
- Do not commit, push, publish, or deploy.

---

### Task 1: Lock the brand-copy contract in tests

**Files:**
- Modify: `app/src/util/qingyuBranding.test.ts`

**Interfaces:**
- Consumes: repository files through the existing `readRepositoryFile(path)` helper.
- Produces: a regression test named `QingYu brand voice is consistent across application and README surfaces`.

- [ ] **Step 1: Write the failing brand-copy test**

Add a test that parses every `app/appearance/langs/*.json` file and compares `slogan` with an explicit locale map. Assert that `app/electron/window.js` contains every expected localized phrase, that the four README headers contain their full localized brand lines, that `app/src/config/tabs/aboutTab.ts` renders `config-about__separator` and no longer contains `会泽百家 至公天下`, and that the Web App description includes `privacy-first space for writing and thinking`.

The locale map is:

```ts
const slogans = {
    ar: "نوافذ مضيئة، مكتب هادئ، وكلمات تُقال برفق",
    de: "Helle Fenster, ein stiller Schreibtisch, sanft gesetzte Worte.",
    en: "Clear windows, a quiet desk, words softly spoken.",
    es: "Ventanas luminosas, un escritorio sereno, palabras dichas en voz baja.",
    fr: "Fenêtres claires, bureau paisible, mots murmurés.",
    he: "חלונות מוארים, שולחן שקט, מילים הנאמרות ברכות.",
    hi: "उजली खिड़कियाँ, शांत मेज़, हर शब्द धीमे से कहा गया।",
    id: "Jendela terang, meja tenang, kata-kata yang terucap lembut.",
    it: "Finestre luminose, una scrivania quieta, parole sussurrate.",
    ja: "明るい窓、静かな机、言葉はそっと。",
    ko: "밝은 창, 고요한 책상, 나직이 놓이는 말.",
    nl: "Heldere ramen, een rustige schrijftafel, zacht gesproken woorden.",
    pl: "Jasne okna, spokojne biurko, słowa wypowiadane cicho.",
    "pt-BR": "Janelas claras, uma mesa serena, palavras ditas suavemente.",
    ru: "Светлые окна, тихий стол, слова, сказанные вполголоса.",
    sk: "Svetlé okná, pokojný stôl, slová vyslovené potichu.",
    th: "หน้าต่างสว่าง โต๊ะสงบ ถ้อยคำแผ่วเบา",
    tr: "Aydınlık pencereler, sakin bir masa, usulca söylenen sözler.",
    uk: "Світлі вікна, тихий стіл, слова, сказані пошепки.",
    "zh-CN": "明窗净几，字字轻语",
    "zh-TW": "明窗淨几，字字輕語",
} as const;
```

- [ ] **Step 2: Run the focused test and confirm the contract fails**

Run: `cd app && node --import tsx --test src/util/qingyuBranding.test.ts`

Expected: FAIL because the current language resources and README headers still use the legacy slogan.

### Task 2: Update application presentation surfaces

**Files:**
- Modify: `app/appearance/langs/ar.json`
- Modify: `app/appearance/langs/de.json`
- Modify: `app/appearance/langs/en.json`
- Modify: `app/appearance/langs/es.json`
- Modify: `app/appearance/langs/fr.json`
- Modify: `app/appearance/langs/he.json`
- Modify: `app/appearance/langs/hi.json`
- Modify: `app/appearance/langs/id.json`
- Modify: `app/appearance/langs/it.json`
- Modify: `app/appearance/langs/ja.json`
- Modify: `app/appearance/langs/ko.json`
- Modify: `app/appearance/langs/nl.json`
- Modify: `app/appearance/langs/pl.json`
- Modify: `app/appearance/langs/pt-BR.json`
- Modify: `app/appearance/langs/ru.json`
- Modify: `app/appearance/langs/sk.json`
- Modify: `app/appearance/langs/th.json`
- Modify: `app/appearance/langs/tr.json`
- Modify: `app/appearance/langs/uk.json`
- Modify: `app/appearance/langs/zh-CN.json`
- Modify: `app/appearance/langs/zh-TW.json`
- Modify: `app/electron/window.js`
- Modify: `app/src/config/tabs/aboutTab.ts`

**Interfaces:**
- Consumes: the locale-to-slogan contract from Task 1 and existing `window.siyuan.languages.slogan` rendering.
- Produces: consistent localized `slogan` strings and an about-page brand line composed as product name, `·`, localized slogan.

- [ ] **Step 1: Replace every application-language slogan**

Set each JSON `slogan` to the exact matching value in the Task 1 locale map. Change only the value of the existing key; do not reorder language resources.

- [ ] **Step 2: Replace every Electron pre-boot slogan**

Set each locale's `slogan` in `I18N_BASE` to the exact matching value from Task 1 so startup and runtime translations do not diverge.

- [ ] **Step 3: Compose the about-page brand line**

Delete the `motto` constant and all uses of it. Between `${window.siyuan.languages.siyuanNote}` and `${window.siyuan.languages.slogan}`, render:

```html
<span class="fn__space"></span>
<span class="config-about__separator">·</span>
<span class="fn__space"></span>
```

Remove `motto` from search keywords and remove the trailing `config-about__motto` span.

- [ ] **Step 4: Run the focused brand test**

Run: `cd app && node --import tsx --test src/util/qingyuBranding.test.ts`

Expected: README and manifest assertions still FAIL, while language-resource, Electron, and about-page assertions PASS.

### Task 3: Rewrite README introductions and Web App metadata

**Files:**
- Modify: `README.zh-CN.md`
- Modify: `README.md`
- Modify: `README.ja.md`
- Modify: `README.tr.md`
- Modify: `app/stage/manifest.webmanifest`

**Interfaces:**
- Consumes: the brand voice defined in the design specification.
- Produces: full localized header lines and two-layer localized product introductions.

- [ ] **Step 1: Replace README header slogans**

Use these exact lines:

```html
<em>轻语 · 明窗净几，字字轻语</em>
<em>QingYu · Clear windows, a quiet desk, words softly spoken.</em>
<em>QingYu · 明るい窓、静かな机、言葉はそっと。</em>
<em>QingYu · Aydınlık pencereler, sakin bir masa, usulca söylenen sözler.</em>
```

- [ ] **Step 2: Rewrite the four introductions**

Use one unwrapped paragraph per language:

```text
轻语为写作与思考留出一方明净、安静且由你掌控的空间。它是一款隐私优先的个人知识管理系统，支持细粒度块级引用和 Markdown 所见即所得，让记录、连接与沉淀自然发生。

QingYu gives writing and thinking a clear, quiet space that stays under your control. It is a privacy-first personal knowledge management system with fine-grained block references and Markdown WYSIWYG, designed to make capturing, connecting, and refining ideas feel natural.

QingYuは、書くことと思考することのために、明るく静かで、自分の手にある空間をつくります。きめ細かなブロック参照とMarkdown WYSIWYGを備えた、プライバシー優先の個人知識管理システムとして、記録し、つなぎ、考えを育てる流れを自然にします。

QingYu, yazmak ve düşünmek için aydınlık, sakin ve denetimi sende olan bir alan sunar. Ayrıntılı blok referansları ve Markdown WYSIWYG desteğine sahip, gizliliği ön planda tutan kişisel bilgi yönetim sistemi olarak fikirleri kaydetmeyi, ilişkilendirmeyi ve olgunlaştırmayı doğal hâle getirir.
```

- [ ] **Step 3: Update the Web App description**

Set `description` in `app/stage/manifest.webmanifest` to:

```json
"description": "QingYu is a privacy-first space for writing and thinking, with fine-grained block references and Markdown WYSIWYG."
```

- [ ] **Step 4: Run the focused brand test**

Run: `cd app && node --import tsx --test src/util/qingyuBranding.test.ts`

Expected: PASS.

### Task 4: Complete repository-level verification

Before verification, replace the exact source header `// SiYuan - Refactor your thinking` under `kernel/` and `app/electron/` with `// 轻语 · 明窗净几，字字轻语`. Set `app/package.json`, both `app/appx/AppxManifest*.xml` descriptions, and both Linux Electron Builder `Comment` fields to `QingYu · Clear windows, a quiet desk, words softly spoken.`. Extend the branding test to cover those package metadata files.

**Files:**
- Verify: all files modified in Tasks 1–3

**Interfaces:**
- Consumes: the frozen implementation tree from Tasks 1–3.
- Produces: static, localization, test, and lint evidence for handoff.

- [ ] **Step 1: Scan for legacy slogan variants**

Run an `rg` scan across maintained source and README files for all old `slogan` values that existed before this task, excluding generated build output, changelogs, and the new design/plan documents that name the old slogan for historical context.

Expected: no matches in runtime source, language resources, or README headers.

- [ ] **Step 2: Validate language resources**

Run: `python scripts/check-lang-keys.py`

Expected: exit code 0 with all language keys consistent.

- [ ] **Step 3: Run the full branding test file**

Run: `cd app && node --import tsx --test src/util/qingyuBranding.test.ts`

Expected: all tests PASS.

- [ ] **Step 4: Run frontend lint**

Run: `cd app && pnpm run lint`

Expected: exit code 0. If unrelated pre-existing failures occur, capture the exact output and distinguish them from files changed by this task.

- [ ] **Step 5: Review the final scoped diff**

Run `git diff --` for the design, plan, test, language resources, Electron dictionary, about page, READMEs, and manifest. Confirm that edits match the specification and that no unrelated dirty-worktree content was removed or overwritten.
