import type { ReactElement } from "react";

type NewsprintEditorPreviewProps = {
  accessibleName: string;
  caption: string;
};

export function NewsprintEditorPreview({
  accessibleName,
  caption
}: NewsprintEditorPreviewProps): ReactElement {
  return (
    <figure className="product-figure">
      <img
        className="product-image"
        src="/product-editor-light.jpg"
        alt={accessibleName}
        width={1440}
        height={900}
        fetchPriority="high"
      />
      <figcaption className="product-caption">
        <span>{caption}</span>
        <span lang="en">QingYu · Preview</span>
      </figcaption>
    </figure>
  );
}
