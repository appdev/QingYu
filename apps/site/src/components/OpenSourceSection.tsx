import type { ReactElement } from "react";
import type { SiteCopy } from "../content";
import { siteLinks } from "../links";

export function OpenSourceSection({ copy }: { copy: SiteCopy["openSource"] }): ReactElement {
  return (
    <section className="site-section open-source-section" aria-labelledby="open-source-title">
      <h2 id="open-source-title">{copy.title}</h2>
      <p>{copy.body}</p>
      <div className="open-source__actions">
        <a className="button-link button-link--primary" href={siteLinks.github}>{copy.github}</a>
        <a className="button-link" href={siteLinks.docs}>{copy.docs}</a>
      </div>
    </section>
  );
}
