import { useEffect, useState, type ReactElement } from "react";
import type { SiteCopy } from "../content";
import { siteLinks } from "../links";
import {
  detectDownloadPlatform,
  orderDownloadPlatforms,
  type DownloadPlatform
} from "../lib/platform";

export function PlatformDownload({ copy }: { copy: SiteCopy["downloads"] }): ReactElement {
  const [preferredPlatform, setPreferredPlatform] = useState<DownloadPlatform | null>(null);

  useEffect(() => {
    setPreferredPlatform(detectDownloadPlatform(navigator));
  }, []);

  const platforms = orderDownloadPlatforms(preferredPlatform);

  return (
    <section id="download" className="site-section download-section" aria-labelledby="download-title">
      <p className="eyebrow">{copy.label}</p>
      <h2 id="download-title">{copy.title}</h2>
      <div className="download-list">
        {platforms.map((platform) => (
          <article
            className="download-row"
            key={platform}
            data-platform={platform}
            data-preferred={platform === preferredPlatform ? "true" : undefined}
          >
            <h3 lang="en">{copy.platformLabels[platform]}</h3>
            <a className="text-link" href={siteLinks.releases}>{copy.release}</a>
          </article>
        ))}
        <article className="download-row">
          <h3>{copy.webLabel}</h3>
          <a className="text-link" href={siteLinks.webEditor}>{copy.webAction}</a>
        </article>
      </div>
    </section>
  );
}
