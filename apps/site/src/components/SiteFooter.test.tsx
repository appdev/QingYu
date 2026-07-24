import { render, screen } from "@testing-library/react";
import { siteContent } from "../content";
import { siteLinks } from "../links";
import { SiteFooter } from "./SiteFooter";

describe("SiteFooter", () => {
  it("uses the centralized destinations for every footer link", () => {
    const copy = siteContent["zh-CN"];
    render(<SiteFooter copy={copy} />);

    const links = [
      [copy.hero.download, siteLinks.releases],
      [copy.hero.web, siteLinks.webEditor],
      [copy.openSource.github, siteLinks.github],
      [copy.openSource.docs, siteLinks.docs],
      [copy.footer.privacy, siteLinks.privacy],
      [copy.footer.changelog, siteLinks.changelog],
      [copy.footer.contribute, siteLinks.contributing],
      [copy.footer.license, siteLinks.license]
    ] as const;

    for (const [name, href] of links) {
      expect(screen.getByRole("link", { name })).toHaveAttribute("href", href);
    }

    expect(screen.getAllByRole("navigation")).toHaveLength(1);
    expect(screen.getByRole("navigation", { name: "页脚导航" })).toBeInTheDocument();
  });

  it("uses English navigation names with English copy", () => {
    render(<SiteFooter copy={siteContent.en} />);

    expect(screen.getAllByRole("navigation")).toHaveLength(1);
    expect(screen.getByRole("navigation", { name: "Footer navigation" })).toBeInTheDocument();
  });
});
