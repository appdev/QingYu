import type { ReactElement } from "react";
import type { SiteCopy } from "../content";

export function MobilePreview({ copy }: { copy: SiteCopy }): ReactElement {
  return (
    <section id="mobile" className="site-section mobile-section" aria-labelledby="mobile-title">
      <div className="section-copy">
        <h2 id="mobile-title">{copy.mobile.title}</h2>
        <p>{copy.mobile.body}</p>
      </div>
      <div className="mobile-status">
        <strong>{copy.mobile.status}</strong>
        <span lang="en">QingYu · Mobile</span>
      </div>
    </section>
  );
}
