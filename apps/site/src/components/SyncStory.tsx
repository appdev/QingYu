import type { ReactElement } from "react";
import type { SiteCopy } from "../content";

export function SyncStory({ copy }: { copy: SiteCopy }): ReactElement {
  return (
    <section id="sync" className="site-section sync-section" aria-labelledby="sync-title">
      <div className="section-copy">
        <p className="eyebrow">{copy.sync.label}</p>
        <h2 id="sync-title">{copy.sync.title}</h2>
        <p>{copy.sync.body}</p>
      </div>
      <div>
        <div className="sync-flow" aria-label={copy.accessibility.syncFlow}>
          <span>{copy.sync.flow.local}</span>
          <span aria-hidden="true">{" ↔ "}</span>
          <span>{copy.sync.flow.remote}</span>
        </div>
        <p className="sync-note">{copy.sync.flow.note}</p>
        <ul className="sync-points">
          {copy.sync.points.map((point) => <li key={point}>{point}</li>)}
        </ul>
      </div>
    </section>
  );
}
