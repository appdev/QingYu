import type { EditorState } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  WidgetType,
  type EditorView as CodeMirrorView,
} from "@codemirror/view";
import { defineMarkraPlugin } from "./plugin.ts";
import {
  markraRenderer,
  type MarkraRendererContext,
  type MarkraSyntaxNode,
} from "./renderers.ts";
import { unescapeMarkdown, unquoteMarkdownTitle } from "./syntax.ts";

export interface MarkraImageSourceContext {
  readonly alt: string;
  readonly source: string;
  readonly state: EditorState;
  readonly title: string;
  readonly view: CodeMirrorView;
}

export interface ImagePreviewPluginOptions {
  className?: string;
  resolveSource?: (context: MarkraImageSourceContext) => string | null;
}

interface ImageDetails {
  alt: string;
  source: string;
  title: string;
}

const safeDataImage = /^data:image\/(?:avif|gif|jpeg|png|webp);base64,/iu;
const scheme = /^([a-z][a-z\d+.-]*):/iu;

const imageTheme = EditorView.baseTheme({
  ".cm-markra-image": {
    borderRadius: "0.35em",
    display: "inline-block",
    maxHeight: "32rem",
    maxWidth: "100%",
    objectFit: "contain",
    verticalAlign: "middle",
  },
});

function imageDetails(
  state: EditorState,
  node: MarkraSyntaxNode,
): ImageDetails | null {
  const marks = node.getChildren("LinkMark");
  const url = node.getChild("URL");
  const openingLabel = marks[0];
  const closingLabel = marks[1];
  if (!url || !openingLabel || !closingLabel) return null;

  const title = node.getChild("LinkTitle");
  return {
    alt: unescapeMarkdown(
      state.sliceDoc(openingLabel.to, closingLabel.from),
    ),
    source: unescapeMarkdown(state.sliceDoc(url.from, url.to).trim()),
    title: title
      ? unquoteMarkdownTitle(state.sliceDoc(title.from, title.to).trim())
      : "",
  };
}

export function resolveSafeImageSource(source: string) {
  const candidate = source.trim();
  if (!candidate) return null;
  if (safeDataImage.test(candidate)) return candidate;

  const matchedScheme = scheme.exec(candidate)?.[1]?.toLocaleLowerCase();
  if (!matchedScheme) return candidate;
  return matchedScheme === "http" ||
    matchedScheme === "https" ||
    matchedScheme === "blob"
    ? candidate
    : null;
}

class ImageWidget extends WidgetType {
  constructor(
    readonly alt: string,
    readonly className: string,
    readonly source: string,
    readonly title: string,
  ) {
    super();
  }

  eq(other: ImageWidget) {
    return (
      this.alt === other.alt &&
      this.className === other.className &&
      this.source === other.source &&
      this.title === other.title
    );
  }

  ignoreEvent() {
    // Let CodeMirror translate pointer events on the replacement widget into a
    // document selection, which immediately reveals the editable image source.
    return false;
  }

  toDOM(view: CodeMirrorView) {
    const image = view.dom.ownerDocument.createElement("img");
    image.alt = this.alt;
    image.className = this.className;
    image.decoding = "async";
    image.draggable = false;
    image.loading = "lazy";
    image.src = this.source;
    if (this.title) image.title = this.title;
    return image;
  }
}

function resolvedImageSource(
  context: MarkraRendererContext,
  details: ImageDetails,
  resolver: ImagePreviewPluginOptions["resolveSource"],
) {
  const sourceContext: MarkraImageSourceContext = {
    ...details,
    state: context.state,
    view: context.view,
  };
  if (!resolver) return resolveSafeImageSource(details.source);

  try {
    return resolver(sourceContext);
  } catch {
    return null;
  }
}

export function imagePreviewPlugin(options: ImagePreviewPluginOptions = {}) {
  const customClassName = options.className?.trim();
  const className = customClassName
    ? `cm-markra-image ${customClassName}`
    : "cm-markra-image";

  return defineMarkraPlugin({
    id: "markra.image-preview",
    extension: [
      markraRenderer({
        id: "markra.image-preview",
        nodeNames: ["Image"],
        render(context) {
          if (context.revealed("node")) return true;
          const startLine = context.state.doc.lineAt(context.node.from).number;
          const endLine = context.state.doc.lineAt(context.node.to).number;
          if (startLine !== endLine) return true;

          const details = imageDetails(context.state, context.node);
          if (!details) return true;
          const source = resolvedImageSource(
            context,
            details,
            options.resolveSource,
          );
          if (!source) return true;

          context.add(
            Decoration.replace({
              widget: new ImageWidget(
                details.alt,
                className,
                source,
                details.title,
              ),
            }).range(context.node.from, context.node.to),
          );
          return false;
        },
      }),
      imageTheme,
    ],
  });
}
