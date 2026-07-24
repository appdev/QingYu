import type { ReactElement } from "react";
import type { SiteCopy } from "../content";

export function ManifestoIntro({ copy }: { copy: SiteCopy["personality"] }): ReactElement {
  return (
    <section className="site-section manifesto-intro" aria-labelledby="personality-title">
      <h2 id="personality-title">{copy.title}</h2>
      {copy.body.map((paragraph) => <p key={paragraph}>{paragraph}</p>)}
    </section>
  );
}
