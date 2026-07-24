import { fireEvent, render, screen, within } from "@testing-library/react";
import { SiteApp } from "../SiteApp";
import { siteContent } from "../content";
import { siteLinks } from "../links";

describe("product story", () => {
  it("renders every approved section in order with one hero heading", () => {
    render(<SiteApp initialLocale="zh-CN" />);

    const sectionIds = Array.from(document.querySelectorAll("main > section"))
      .map((section) => section.id)
      .filter(Boolean);

    expect(sectionIds).toEqual([
      "product",
      "features",
      "personalization",
      "sync",
      "mobile",
      "download",
      "manifesto"
    ]);
    expect(screen.getAllByRole("heading", { level: 1 })).toHaveLength(1);
    expect(screen.getByRole("heading", {
      level: 1,
      name: siteContent["zh-CN"].hero.title
    })).toBeInTheDocument();
    expect(document.querySelector('img[src="/qingyu-logo.png"]')).toBeNull();
  });

  it("renders the QingYu hero, real calls to action, and the real product capture", () => {
    render(<SiteApp initialLocale="zh-CN" />);
    const product = document.querySelector<HTMLElement>("#product");
    if (!product) throw new Error("Missing product section");

    expect(product.querySelector('img[src="/qingyu-logo.webp"]'))
      .toHaveAttribute("alt", "");
    expect(within(product).getByText("轻语 QingYu")).toBeInTheDocument();
    expect(within(product).getByRole("link", { name: siteContent["zh-CN"].hero.download }))
      .toHaveAttribute("href", "#download");
    expect(within(product).getByRole("link", { name: siteContent["zh-CN"].hero.web }))
      .toHaveAttribute("href", siteLinks.webEditor);

    const availablePlatforms = within(product).getByRole("list", { name: "可用平台" });
    expect(availablePlatforms).not.toHaveAttribute("lang");
    for (const platform of within(availablePlatforms).getAllByRole("listitem")) {
      expect(platform).toHaveAttribute("lang", "en");
    }
    const preview = within(product).getByRole("img", { name: "轻语编辑器真实界面" });
    expect(preview).toHaveAttribute("src", "/product-editor-light.jpg");
    expect(preview).toHaveAttribute("width", "1440");
    expect(preview).toHaveAttribute("height", "900");
    expect(preview).toHaveAttribute("fetchpriority", "high");
    expect(within(product).getByText("Web 编辑器 · 让 Markdown 像纸页一样展开")).toBeInTheDocument();
  });

  it("localizes product captures and workflow accessible names", () => {
    render(<SiteApp initialLocale="zh-CN" />);

    expect(screen.getByRole("img", { name: "轻语编辑器真实界面" })).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "轻语移动端预览" })).toBeNull();
    const chineseSyncFlow = screen.getByLabelText("当前笔记目录 ↔ WebDAV / S3 兼容存储");
    expect(chineseSyncFlow).not.toHaveAttribute("lang");
    expect(within(chineseSyncFlow).getByText("当前笔记目录")).toBeInTheDocument();
    expect(within(chineseSyncFlow).getByText("WebDAV / S3 兼容存储")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "English" }));
    expect(screen.getByRole("img", { name: "Real QingYu editor interface" })).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "QingYu mobile preview" })).toBeNull();
    expect(screen.getByLabelText("Current notebook ↔ WebDAV / S3-compatible storage"))
      .toBeInTheDocument();
  });

  it("uses real product captures for the three workbench stories", () => {
    render(<SiteApp initialLocale="zh-CN" />);
    const features = document.querySelector<HTMLElement>("#features");
    const personalization = document.querySelector<HTMLElement>("#personalization");
    if (!features || !personalization) throw new Error("Missing workbench sections");

    const splitPreview = within(features).getByRole("img", { name: "轻语预览与源码分栏界面" });
    expect(splitPreview).toHaveAttribute("src", "/product-editor-split.jpg");
    expect(splitPreview).toHaveAttribute("width", "1440");
    expect(splitPreview).toHaveAttribute("height", "900");
    expect(splitPreview).toHaveAttribute("loading", "lazy");

    const exportPreview = within(features).getByRole("img", { name: "轻语导出设置界面" });
    expect(exportPreview).toHaveAttribute("src", "/product-export.jpg");
    expect(exportPreview).toHaveAttribute("loading", "lazy");

    const appearancePreview = within(personalization).getByRole("img", { name: "轻语外观设置界面" });
    expect(appearancePreview).toHaveAttribute("src", "/product-appearance.jpg");
    expect(appearancePreview).toHaveAttribute("loading", "lazy");

    expect(features.querySelector("svg")).toBeNull();
    expect(within(features).getByText("文档视图、源码和分栏，都是同一份 Markdown 的不同光景。"))
      .toBeInTheDocument();
    expect(within(features).getByText("桌面版导出 · 让文字去往 HTML、PDF 与 Pandoc 格式"))
      .toBeInTheDocument();
    expect(within(personalization).getByText("外观设置 · 让配色、字体与书写宽度随你"))
      .toBeInTheDocument();
  });

  it("describes the implemented product workflows and their platform boundaries", () => {
    render(<SiteApp initialLocale="zh-CN" />);

    expect(screen.getByText("让记录回到纸页般自然。")).toBeInTheDocument();
    expect(screen.getByText("文档与源码，一体两面")).toBeInTheDocument();
    expect(screen.getByText("文件夹，就是工作区")).toBeInTheDocument();
    expect(screen.getByText("写下、保存，也能回望")).toBeInTheDocument();
    expect(screen.getByText("给 MCP 一扇有边界的门")).toBeInTheDocument();
    expect(screen.getByText(
      "桌面版以应用级方式配置 MCP，并把文档工具限定在当前笔记目录；设置与同步权限也由应用统一控制，这扇门默认关闭。"
    )).toBeInTheDocument();
    expect(screen.getByText("让界面随心境收放。")).toBeInTheDocument();

    const sync = document.querySelector<HTMLElement>("#sync");
    if (!sync) throw new Error("Missing sync section");
    expect(within(sync).getByText("当前笔记目录")).toBeInTheDocument();
    expect(within(sync).getByText("WebDAV / S3 兼容存储")).toBeInTheDocument();
    expect(sync).toHaveTextContent("当前笔记目录 ↔ WebDAV / S3 兼容存储");
    expect(within(sync).getByText("桌面与移动端 · 当前笔记目录双向同步")).toBeInTheDocument();
    expect(sync).toHaveTextContent("notes/<目录名>/");
    expect(sync).toHaveTextContent("独立打开的单个 Markdown 文件不参与同步");
    expect(sync).toHaveTextContent("同步不是备份");
    expect(sync.querySelector("svg")).toBeNull();

    expect(document.body).not.toHaveTextContent("掌中随手记");
    expect(document.body).not.toHaveTextContent("剥离复杂的块与双链");
    expect(document.body).not.toHaveTextContent("Your devices");
  });

  it("renders five factual product principles in order", () => {
    render(<SiteApp initialLocale="zh-CN" />);
    const manifesto = document.querySelector<HTMLElement>("#manifesto");
    if (!manifesto) throw new Error("Missing manifesto section");

    const lines = within(manifesto).getAllByRole("listitem").map((item) => item.textContent);
    expect(lines).toEqual(siteContent["zh-CN"].manifesto.lines);
    expect(within(manifesto).getByText("同步可以抵达远方，但只跟随你当前选择的笔记目录。"))
      .toBeInTheDocument();
  });

  it("does not expose unreleased mobile downloads", () => {
    render(<SiteApp initialLocale="zh-CN" />);
    const mobile = document.querySelector<HTMLElement>("#mobile");
    if (!mobile) throw new Error("Missing mobile section");

    expect(within(mobile).getByText("原生验证中 · 尚未发布")).toBeInTheDocument();
    expect(within(mobile).getByText("案头之外，轻语正在走向掌中。")).toBeInTheDocument();
    expect(within(mobile).queryByRole("link")).toBeNull();
    expect(within(mobile).queryByRole("button")).toBeNull();
    expect(within(mobile).queryByRole("img")).toBeNull();
    expect(mobile.querySelector("figure")).toBeNull();
    expect(document.querySelector('a[href*="play.google"]')).toBeNull();
    expect(document.querySelector('a[href*="apps.apple"]')).toBeNull();
  });

  it("uses centralized destinations for downloads and open-source links", () => {
    render(<SiteApp initialLocale="zh-CN" />);
    const download = document.querySelector<HTMLElement>("#download");
    if (!download) throw new Error("Missing download section");

    expect(within(download).getAllByRole("link", { name: siteContent["zh-CN"].downloads.release }))
      .toHaveLength(3);
    for (const link of within(download).getAllByRole("link", {
      name: siteContent["zh-CN"].downloads.release
    })) {
      expect(link).toHaveAttribute("href", siteLinks.releases);
    }
    expect(within(download).getByRole("link", { name: siteContent["zh-CN"].downloads.webAction }))
      .toHaveAttribute("href", siteLinks.webEditor);

    const openSourceTitle = screen.getByRole("heading", {
      name: siteContent["zh-CN"].openSource.title
    });
    const openSource = openSourceTitle.closest<HTMLElement>("section");
    if (!openSource) throw new Error("Missing open-source section");

    expect(within(openSource).getByRole("link", { name: siteContent["zh-CN"].openSource.github }))
      .toHaveAttribute("href", siteLinks.github);
    expect(within(openSource).getByRole("link", { name: siteContent["zh-CN"].openSource.docs }))
      .toHaveAttribute("href", siteLinks.docs);
  });
});
