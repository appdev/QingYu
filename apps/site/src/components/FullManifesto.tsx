import type { ReactElement } from "react";
import type { SiteCopy } from "../content";

export function FullManifesto({ copy }: { copy: SiteCopy["manifesto"] }): ReactElement {
  return (
    <section
      id="manifesto"
      className="site-section manifesto-section"
      aria-labelledby="manifesto-title"
    >
      <h2 id="manifesto-title">{copy.label}</h2>
      <ol>
        {copy.lines.map((line) => <li key={line}>{line}</li>)}
      </ol>
    </section>
  );
}
