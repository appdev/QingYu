import type { ReactElement } from "react";
import type { SiteCopy } from "../content";
import { siteLinks } from "../links";
import { NewsprintEditorPreview } from "./NewsprintEditorPreview";

const supportedPlatforms = ["macOS", "Windows", "Linux", "Web"] as const;

export function Hero({ copy }: { copy: SiteCopy }): ReactElement {
  return (
    <section id="product" className="site-section hero-section" aria-labelledby="hero-title">
      <div className="hero-copy">
        <div className="hero-brandline">
          <img src="/qingyu-logo.webp" alt="" />
          <span>{copy.brand.displayName}</span>
        </div>
        <p className="eyebrow">{copy.hero.eyebrow}</p>
        <h1 id="hero-title">{copy.hero.title}</h1>
        <p className="hero-description">{copy.hero.description}</p>
        <div className="hero-actions">
          <a className="button-link button-link--primary" href="#download">{copy.hero.download}</a>
          <a className="button-link" href={siteLinks.webEditor}>{copy.hero.web}</a>
        </div>
        <ul className="platform-list" aria-label={copy.accessibility.availablePlatforms}>
          {supportedPlatforms.map((platform) => <li key={platform} lang="en">{platform}</li>)}
        </ul>
      </div>
      <NewsprintEditorPreview
        accessibleName={copy.accessibility.editorPreview}
        caption={copy.hero.previewCaption}
      />
    </section>
  );
}
