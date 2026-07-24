import type { ReactElement } from "react";
import type { SiteCopy } from "../content";

export function Personalization({ copy }: { copy: SiteCopy }): ReactElement {
  return (
    <section
      id="personalization"
      className="site-section workbench-section"
      aria-labelledby="personalization-title"
    >
      <div className="workbench-copy">
        <h2 id="personalization-title">{copy.personalization.title}</h2>
        <p>{copy.personalization.body}</p>
        <ul className="personalization-list">
          {copy.personalization.items.map((item) => <li key={item}>{item}</li>)}
        </ul>
      </div>
      <figure className="workbench-figure">
        <img
          className="workbench-image"
          src="/product-appearance.jpg"
          alt={copy.accessibility.appearancePreview}
          width={1440}
          height={900}
          loading="lazy"
        />
        <figcaption className="workbench-caption">
          {copy.personalization.previewCaption}
        </figcaption>
      </figure>
    </section>
  );
}
