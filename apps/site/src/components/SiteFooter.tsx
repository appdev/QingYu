import type { ReactElement } from "react";
import type { SiteCopy } from "../content";
import { siteLinks } from "../links";

export function SiteFooter({ copy }: { copy: SiteCopy }): ReactElement {
  return (
    <footer className="site-footer">
      <p>
        <span>{copy.brand.name}</span>
        {" · "}
        <span lang="en">Markdown, WebDAV, S3</span>
      </p>

      <nav aria-label={copy.accessibility.footerNavigation}>
        <a href={siteLinks.releases}>{copy.hero.download}</a>
        <a href={siteLinks.webEditor}>{copy.hero.web}</a>
        <a href={siteLinks.github}>{copy.openSource.github}</a>
        <a href={siteLinks.docs}>{copy.openSource.docs}</a>
        <a href={siteLinks.privacy}>{copy.footer.privacy}</a>
        <a href={siteLinks.changelog}>{copy.footer.changelog}</a>
        <a href={siteLinks.contributing}>{copy.footer.contribute}</a>
        <a href={siteLinks.license}>{copy.footer.license}</a>
      </nav>
    </footer>
  );
}
