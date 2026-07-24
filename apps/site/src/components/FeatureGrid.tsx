import type { ReactElement } from "react";
import type { SiteCopy } from "../content";

export function FeatureGrid({ copy }: { copy: SiteCopy }): ReactElement {
  const exportFeature = copy.features.items[copy.features.items.length - 1];

  return (
    <section id="features" className="site-section workbench-tour" aria-labelledby="features-title">
      <article className="workbench-section">
        <div className="workbench-copy">
          <p className="eyebrow">{copy.features.label}</p>
          <h2 id="features-title">{copy.features.title}</h2>
          <ul className="workbench-notes">
            {copy.features.items.slice(0, -1).map((item) => (
              <li key={item.title}>
                <h3>{item.title}</h3>
                <p>{item.body}</p>
              </li>
            ))}
          </ul>
        </div>
        <figure className="workbench-figure">
          <img
            className="workbench-image"
            src="/product-editor-split.jpg"
            alt={copy.accessibility.editorSplitPreview}
            width={1440}
            height={900}
            loading="lazy"
          />
          <figcaption className="workbench-caption">{copy.features.editorCaption}</figcaption>
        </figure>
      </article>

      {exportFeature ? (
        <article className="workbench-section workbench-section--reverse">
          <div className="workbench-copy">
            <h3>{exportFeature.title}</h3>
            <p>{exportFeature.body}</p>
          </div>
          <figure className="workbench-figure">
            <img
              className="workbench-image"
              src="/product-export.jpg"
              alt={copy.accessibility.exportPreview}
              width={1440}
              height={900}
              loading="lazy"
            />
            <figcaption className="workbench-caption">{copy.features.exportCaption}</figcaption>
          </figure>
        </article>
      ) : null}
    </section>
  );
}
