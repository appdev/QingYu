import { fireEvent, render, screen, within } from "@testing-library/react";
import { siteContent } from "../content";
import { siteLinks } from "../links";
import { SiteHeader } from "./SiteHeader";

describe("SiteHeader", () => {
  const copy = siteContent["zh-CN"];

  it("renders the edge-aligned brand and product actions without a desktop link row", () => {
    const onLocaleChange = vi.fn();
    const { container } = render(
      <SiteHeader copy={copy} locale="zh-CN" onLocaleChange={onLocaleChange} />
    );

    expect(screen.getByRole("link", { name: "轻语主页" })).toHaveAttribute("href", "#product");
    expect(screen.queryByRole("img")).toBeNull();
    expect(container.querySelector('img[alt=""]')).toHaveAttribute("src", "/qingyu-logo.webp");
    expect(container.querySelector('img[src="/qingyu-logo.png"]')).toBeNull();
    expect(screen.queryByRole("navigation", { name: "主导航" })).toBeNull();
    expect(screen.getByRole("link", { name: copy.hero.web })).toHaveAttribute("href", siteLinks.webEditor);
    expect(screen.getByRole("link", { name: copy.hero.download })).toHaveAttribute("href", siteLinks.releases);
    fireEvent.click(screen.getByRole("button", { name: copy.languageLabel }));
    expect(onLocaleChange).toHaveBeenCalledWith("en");
  });

  it("opens and closes the compact menu with its expanded state exposed", () => {
    render(<SiteHeader copy={copy} locale="zh-CN" onLocaleChange={vi.fn()} />);
    const menuButton = screen.getByRole("button", { name: "导航菜单" });

    expect(menuButton).toHaveAttribute("aria-controls", "site-compact-navigation");
    expect(menuButton).toHaveAttribute("aria-expanded", "false");

    fireEvent.click(menuButton);

    const compactNavigation = screen.getByRole("navigation", { name: "折叠导航" });
    expect(compactNavigation).toBeInTheDocument();
    expect(within(compactNavigation).getByRole("link", { name: copy.nav.features }))
      .toHaveAttribute("href", "#features");
    expect(menuButton).toHaveAttribute("aria-expanded", "true");
    expect(document.getElementById("site-compact-navigation")).not.toHaveAttribute("hidden");
  });

  it("closes the compact menu on Escape and returns focus to its button", () => {
    render(<SiteHeader copy={copy} locale="zh-CN" onLocaleChange={vi.fn()} />);
    const menuButton = screen.getByRole("button", { name: "导航菜单" });

    fireEvent.click(menuButton);
    const compactProductLink = screen.getAllByRole("link", { name: copy.nav.product }).at(-1);
    expect(compactProductLink).toBeDefined();
    compactProductLink?.focus();

    fireEvent.keyDown(document, { key: "Escape" });

    expect(menuButton).toHaveAttribute("aria-expanded", "false");
    expect(document.getElementById("site-compact-navigation")).toHaveAttribute("hidden");
    expect(menuButton).toHaveFocus();
  });

  it("exposes English accessible names when English is selected", () => {
    render(<SiteHeader copy={siteContent.en} locale="en" onLocaleChange={vi.fn()} />);

    expect(screen.getByRole("link", { name: "QingYu home" })).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Primary navigation" })).toBeNull();
    const menuButton = screen.getByRole("button", { name: "Navigation menu" });
    fireEvent.click(menuButton);
    expect(screen.getByRole("navigation", { name: "Compact navigation" })).toBeInTheDocument();
  });
});
