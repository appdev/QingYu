import type { DownloadPlatform } from "./lib/platform";

export type SiteLocale = "zh-CN" | "en";

export type SiteCopy = {
  brand: {
    name: string;
    displayName: string;
  };
  languageLabel: string;
  accessibility: {
    brandHome: string;
    navigationMenu: string;
    compactNavigation: string;
    availablePlatforms: string;
    editorPreview: string;
    editorSplitPreview: string;
    exportPreview: string;
    appearancePreview: string;
    syncFlow: string;
    footerNavigation: string;
  };
  nav: {
    product: string;
    features: string;
    sync: string;
    mobile: string;
    manifesto: string;
    download: string;
  };
  hero: {
    eyebrow: string;
    title: string;
    description: string;
    download: string;
    web: string;
    previewCaption: string;
  };
  personality: { label: string; title: string; body: string[] };
  features: {
    label: string;
    title: string;
    items: Array<{ title: string; body: string }>;
    editorCaption: string;
    exportCaption: string;
  };
  personalization: {
    label: string;
    title: string;
    body: string;
    items: string[];
    previewCaption: string;
  };
  sync: {
    label: string;
    title: string;
    body: string;
    flow: { local: string; remote: string; note: string };
    points: string[];
  };
  mobile: { label: string; title: string; body: string; status: string };
  downloads: {
    label: string;
    title: string;
    platformLabels: Record<DownloadPlatform, string>;
    webLabel: string;
    webAction: string;
    release: string;
  };
  manifesto: { label: string; lines: string[] };
  openSource: { label: string; title: string; body: string; github: string; docs: string };
  footer: { privacy: string; changelog: string; contribute: string; license: string };
};

export function stringsInSiteCopy(value: unknown): string[] {
  if (typeof value === "string") return [value];
  if (Array.isArray(value)) return value.flatMap(stringsInSiteCopy);
  if (value !== null && typeof value === "object") {
    return Object.values(value).flatMap(stringsInSiteCopy);
  }
  return [];
}

export const siteContent = {
  "zh-CN": {
    brand: {
      name: "轻语",
      displayName: "轻语 QingYu"
    },
    languageLabel: "English",
    accessibility: {
      brandHome: "轻语主页",
      navigationMenu: "导航菜单",
      compactNavigation: "折叠导航",
      availablePlatforms: "可用平台",
      editorPreview: "轻语编辑器真实界面",
      editorSplitPreview: "轻语预览与源码分栏界面",
      exportPreview: "轻语导出设置界面",
      appearancePreview: "轻语外观设置界面",
      syncFlow: "当前笔记目录 ↔ WebDAV / S3 兼容存储",
      footerNavigation: "页脚导航"
    },
    nav: {
      product: "产品",
      features: "功能",
      sync: "同步",
      mobile: "移动进展",
      manifesto: "原则",
      download: "下载"
    },
    hero: {
      eyebrow: "一个能安心写字的地方",
      title: "明窗净几，字字轻语。",
      description: "打开一份 Markdown，文字便自然铺开。所见即所得与源码模式，写的是同一份文件；无需账号，也不把笔记困在云端。",
      download: "下载桌面版",
      web: "打开 Web 编辑器",
      previewCaption: "Web 编辑器 · 让 Markdown 像纸页一样展开"
    },
    personality: {
      label: "轻语的选择",
      title: "让记录回到纸页般自然。",
      body: [
        "不必先搭建一套系统，也不必把灵感交给算法。轻语只把文件、文件夹和熟悉的 Markdown 安静地放在手边。",
        "想写时便写，想迁移时便迁移；你的文字始终是可以打开、复制和带走的普通文件。"
      ]
    },
    features: {
      label: "写作工坊",
      title: "简单，不等于简陋。",
      items: [
        {
          title: "文档与源码，一体两面",
          body: "在所见即所得、源码或左右分栏之间切换，写下的始终是同一份 Markdown。"
        },
        {
          title: "文件夹，就是工作区",
          body: "在文件树中新建、重命名、移动与删除篇章；再用多标签、快速打开、工作区搜索与大纲找到它们。"
        },
        {
          title: "丰富呈现，不改底色",
          body: "支持链接、图片、HTML、GFM 表格、任务列表、KaTeX、Mermaid、提示块与代码高亮。"
        },
        {
          title: "写下、保存，也能回望",
          body: "桌面版自动保存已有文件，也会恢复标签页与工作区状态；文件历史和字数统计，让写作自然接续。"
        },
        {
          title: "给 MCP 一扇有边界的门",
          body: "桌面版以应用级方式配置 MCP，并把文档工具限定在当前笔记目录；设置与同步权限也由应用统一控制，这扇门默认关闭。"
        },
        {
          title: "图片照常存，文字自由去往",
          body: "本地图片仍在 assets 文件夹；桌面版可导出 HTML、PDF，以及配置 Pandoc 后的 DOCX、EPUB 和 LaTeX。"
        }
      ],
      editorCaption: "文档视图、源码和分栏，都是同一份 Markdown 的不同光景。",
      exportCaption: "桌面版导出 · 让文字去往 HTML、PDF 与 Pandoc 格式"
    },
    personalization: {
      label: "一室多景",
      title: "让界面随心境收放。",
      body: "完整、日常、专注、沉浸与自定义视图，随时收起或展开文件树、大纲、标签页和状态栏；主题、字体与快捷键也各自可调。",
      items: [
        "完整 / 日常 / 专注 / 沉浸视图",
        "浅色、深色与多套编辑器主题",
        "字体、字号、行高与书写宽度",
        "可录制的应用与编辑器快捷键"
      ],
      previewCaption: "外观设置 · 让配色、字体与书写宽度随你"
    },
    sync: {
      label: "数据自主",
      title: "默认留在本地，远行由你作主。",
      body: "WebDAV 或 S3 使用一份应用级配置，并只同步当前笔记目录；切换目录后改用对应的 notes/<目录名>/，独立打开的单个 Markdown 文件不参与同步。",
      flow: {
        local: "当前笔记目录",
        remote: "WebDAV / S3 兼容存储",
        note: "桌面与移动端 · 当前笔记目录双向同步"
      },
      points: ["当前只同步一个笔记目录", "应用级配置", "默认关闭", "同步不是备份"]
    },
    mobile: {
      label: "移动端",
      title: "案头之外，轻语正在走向掌中。",
      body: "Android 模拟器与 iOS Simulator 已走通核心编辑、自动保存与恢复；完整设备验收仍在继续，正式发布还需一些时日。",
      status: "原生验证中 · 尚未发布"
    },
    downloads: {
      label: "现在可用",
      title: "在桌面落笔，或从浏览器开始。",
      platformLabels: { macos: "macOS", windows: "Windows", linux: "Linux" },
      webLabel: "Web",
      webAction: "打开编辑器",
      release: "下载桌面版"
    },
    manifesto: {
      label: "产品原则",
      lines: [
        "写作，不必先搭一套系统。",
        "所见即所得与源码，只是同一份 Markdown 的两面。",
        "文件夹盛放篇章，图片与链接保持原来的模样。",
        "同步可以抵达远方，但只跟随你当前选择的笔记目录。",
        "工具退后一步，文字便向前一步。"
      ]
    },
    openSource: {
      label: "开放",
      title: "代码敞开，文字也不设围墙。",
      body: "轻语采用 AGPL-3.0。笔记、图片和项目文件仍是普通文件，随时可以查看、复制，也可以带往别处。",
      github: "查看源代码",
      docs: "阅读使用文档"
    },
    footer: {
      privacy: "隐私",
      changelog: "更新日志",
      contribute: "参与贡献",
      license: "AGPL-3.0"
    }
  },
  en: {
    brand: {
      name: "QingYu",
      displayName: "QingYu"
    },
    languageLabel: "简体中文",
    accessibility: {
      brandHome: "QingYu home",
      navigationMenu: "Navigation menu",
      compactNavigation: "Compact navigation",
      availablePlatforms: "Available platforms",
      editorPreview: "Real QingYu editor interface",
      editorSplitPreview: "QingYu document and source split view",
      exportPreview: "QingYu export settings",
      appearancePreview: "QingYu appearance settings",
      syncFlow: "Current notebook ↔ WebDAV / S3-compatible storage",
      footerNavigation: "Footer navigation"
    },
    nav: {
      product: "Product",
      features: "Features",
      sync: "Sync",
      mobile: "Mobile status",
      manifesto: "Principles",
      download: "Download"
    },
    hero: {
      eyebrow: "A quiet place to write",
      title: "A clear desk. An open file. Begin.",
      description: "Open a Markdown file and let the page unfold. WYSIWYG and source mode write to the same file—no account required, and no cloud lock-in.",
      download: "Download desktop",
      web: "Open Web editor",
      previewCaption: "Web editor · Let Markdown unfold like a page"
    },
    personality: {
      label: "A deliberate choice",
      title: "Let notes feel as natural as paper.",
      body: [
        "No system to build first, and no need to hand every thought to an algorithm. QingYu keeps files, folders, and familiar Markdown quietly within reach.",
        "Write when the thought arrives, move it when you choose. Your notes remain ordinary files you can open, copy, and carry away."
      ]
    },
    features: {
      label: "The writing room",
      title: "Simple does not mean bare.",
      items: [
        {
          title: "Document and source, two sides of one page",
          body: "Switch among WYSIWYG, source, and side-by-side views while writing to the same Markdown file."
        },
        {
          title: "A folder becomes the workspace",
          body: "Create, rename, move, and delete from the file tree; tabs, quick open, workspace search, and outline help you navigate."
        },
        {
          title: "Rich rendering, ordinary Markdown",
          body: "Use links, images, HTML, GFM tables, task lists, KaTeX, Mermaid, callouts, and highlighted code."
        },
        {
          title: "Write, save, and look back",
          body: "Desktop auto-saves existing files, restores tabs and workspace state, and keeps file history and word counts close at hand."
        },
        {
          title: "A bounded doorway for MCP",
          body: "Desktop uses one application-level MCP policy and binds document tools to the current notebook directory. The app also controls settings and sync permissions, and the door is closed by default."
        },
        {
          title: "Assets stay ordinary; words travel freely",
          body: "Local images stay in an assets folder. Desktop can export HTML and PDF, plus DOCX, EPUB, and LaTeX when Pandoc is configured."
        }
      ],
      editorCaption: "Document, source, and split views are different views of the same Markdown.",
      exportCaption: "Desktop export · Send words to HTML, PDF, and Pandoc formats"
    },
    personalization: {
      label: "A room for every mood",
      title: "Let the interface open and close with your focus.",
      body: "Full, daily, focus, immersive, and custom views reveal or tuck away the file tree, outline, tabs, and status bar. Themes, type, and shortcuts remain yours to tune.",
      items: [
        "Full / daily / focus / immersive views",
        "Light, dark, and multiple editor themes",
        "Font, size, line height, and writing width",
        "Recordable app and editor shortcuts"
      ],
      previewCaption: "Appearance settings · Color, type, and writing width, shaped by you"
    },
    sync: {
      label: "Data on your terms",
      title: "Local by default. Yours to send farther.",
      body: "One application-level WebDAV or S3 configuration synchronizes only the current notebook directory. Switching directories selects its notes/<directory-name>/ target; a standalone Markdown file never joins synchronization.",
      flow: {
        local: "Current notebook",
        remote: "WebDAV / S3-compatible storage",
        note: "Desktop and mobile · current notebook sync"
      },
      points: [
        "One current notebook",
        "Application-level configuration",
        "Off by default",
        "Sync is not backup"
      ]
    },
    mobile: {
      label: "Mobile",
      title: "Beyond the desk, QingYu is finding its way into your hand.",
      body: "Core editing, auto-save, and restoration now work on an Android emulator and iOS Simulator. Full device acceptance continues; public release still lies ahead.",
      status: "Native validation · Not released"
    },
    downloads: {
      label: "Available now",
      title: "Write on desktop, or begin in the browser.",
      platformLabels: { macos: "macOS", windows: "Windows", linux: "Linux" },
      webLabel: "Web",
      webAction: "Open editor",
      release: "Download desktop"
    },
    manifesto: {
      label: "Product principles",
      lines: [
        "Writing should not require a system first.",
        "WYSIWYG and source are two sides of the same Markdown file.",
        "Folders hold the chapters; images and links keep their familiar shape.",
        "Sync can travel far, but it follows only your currently selected notebook directory.",
        "When the tool steps back, the words move forward."
      ]
    },
    openSource: {
      label: "Open",
      title: "The code is open. Your words have no walls.",
      body: "QingYu is licensed under AGPL-3.0. Notes, images, and project files remain ordinary files you can inspect, copy, and carry elsewhere.",
      github: "View source code",
      docs: "Read the documentation"
    },
    footer: {
      privacy: "Privacy",
      changelog: "Changelog",
      contribute: "Contribute",
      license: "AGPL-3.0"
    }
  }
} satisfies Record<SiteLocale, SiteCopy>;
