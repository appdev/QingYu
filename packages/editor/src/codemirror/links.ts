import { syntaxTree } from "@codemirror/language";
import type { EditorState } from "@codemirror/state";
import { EditorView, type EditorView as CodeMirrorView } from "@codemirror/view";
import { defineMarkraPlugin } from "./plugin.ts";
import type { MarkraSyntaxNode } from "./renderers.ts";
import { unescapeMarkdown } from "./syntax.ts";

export type MarkraLinkActivation = "click" | "modifier" | "none";

export interface MarkraLinkSourceContext {
  readonly source: string;
  readonly state: EditorState;
  readonly view: CodeMirrorView;
}

export interface MarkraLinkOpenContext extends MarkraLinkSourceContext {
  readonly target: string;
}

export interface LinksPluginOptions {
  readonly activation?: MarkraLinkActivation;
  readonly label?: string;
  readonly open: (context: MarkraLinkOpenContext) => unknown;
  readonly resolveTarget?: (
    context: MarkraLinkSourceContext,
  ) => string | null;
}

interface ResolvedLink {
  source: string;
  target: string;
}

const unsafeCodePoint = /[\u0000-\u001f\u007f]/u;
const scheme = /^([a-z][a-z\d+.-]*):/iu;
const safeSchemes = new Set(["http", "https", "mailto", "tel"]);

export function resolveSafeLinkTarget(source: string) {
  const candidate = unescapeMarkdown(source.trim());
  if (!candidate || unsafeCodePoint.test(candidate)) return null;

  const matchedScheme = scheme.exec(candidate)?.[1]?.toLocaleLowerCase();
  if (!matchedScheme) return candidate;
  return safeSchemes.has(matchedScheme) ? candidate : null;
}

function linkNodeAt(state: EditorState, position: number) {
  const boundedPosition = Math.max(0, Math.min(position, state.doc.length));
  const tree = syntaxTree(state);
  const candidates = [
    tree.resolveInner(boundedPosition, 1),
    tree.resolveInner(boundedPosition, -1),
  ];

  for (const candidate of candidates) {
    let node: MarkraSyntaxNode | null = candidate;
    while (node) {
      if (node.name === "Link") return node;
      node = node.parent;
    }
  }
  return null;
}

function linkSource(state: EditorState, node: MarkraSyntaxNode) {
  const url = node.getChild("URL");
  if (!url) return null;
  const source = unescapeMarkdown(state.sliceDoc(url.from, url.to).trim());
  return source || null;
}

function resolvedLinkAt(
  view: CodeMirrorView,
  position: number,
  resolver: LinksPluginOptions["resolveTarget"],
): ResolvedLink | null {
  const node = linkNodeAt(view.state, position);
  if (!node) return null;
  const source = linkSource(view.state, node);
  if (!source) return null;

  const sourceContext: MarkraLinkSourceContext = {
    source,
    state: view.state,
    view,
  };
  let target: string | null;
  try {
    target = resolver
      ? resolver(sourceContext)
      : resolveSafeLinkTarget(source);
  } catch {
    return null;
  }

  const normalizedTarget = target?.trim();
  return normalizedTarget ? { source, target: normalizedTarget } : null;
}

function openLinkAt(
  view: CodeMirrorView,
  position: number,
  options: LinksPluginOptions,
) {
  const link = resolvedLinkAt(view, position, options.resolveTarget);
  if (!link) return false;

  try {
    const result = options.open({
      ...link,
      state: view.state,
      view,
    });
    if (
      result &&
      typeof (result as { then?: unknown }).then === "function"
    ) {
      Promise.resolve(result).catch(() => undefined);
    }
    return true;
  } catch {
    return false;
  }
}

function linkPositionFromEvent(event: MouseEvent, view: CodeMirrorView) {
  const target = event.target;
  if (!(target instanceof Element)) return null;
  const link = target.closest(".cm-markra-link");
  if (!link || !view.contentDOM.contains(link)) return null;

  try {
    return view.posAtDOM(link, 0);
  } catch {
    return null;
  }
}

export function linksPlugin(options: LinksPluginOptions) {
  const activation = options.activation ?? "modifier";
  const isAvailable = (view: CodeMirrorView) =>
    resolvedLinkAt(
      view,
      view.state.selection.main.head,
      options.resolveTarget,
    ) !== null;

  return defineMarkraPlugin({
    id: "markra.links",
    commands: [
      {
        id: "link.open",
        isEnabled: isAvailable,
        keybindings: [
          { key: "Mod-Enter", preventDefault: true },
        ],
        label: options.label ?? "Open link",
        run(view) {
          return openLinkAt(
            view,
            view.state.selection.main.head,
            options,
          );
        },
      },
    ],
    extension:
      activation === "none"
        ? []
        : EditorView.domEventHandlers({
            click(event, view) {
              const target = event.target;
              if (!(target instanceof Element)) return false;
              const link = target.closest(".cm-markra-link");
              if (!link || !view.contentDOM.contains(link)) return false;

              // Navigation is handled on mousedown so CodeMirror cannot move
              // the caret and reveal the source before the link is resolved.
              event.preventDefault();
              return true;
            },
            mousedown(event, view) {
              if (event.button !== 0) return false;
              if (
                activation === "modifier" &&
                !event.metaKey &&
                !event.ctrlKey
              ) {
                return false;
              }

              const position = linkPositionFromEvent(event, view);
              if (position === null) return false;
              // CodeMirror moves the selection on mousedown. Intercepting here
              // keeps the rendered link DOM alive long enough to resolve it.
              if (!openLinkAt(view, position, options)) return false;
              event.preventDefault();
              return true;
            },
          }),
    ui: [
      {
        command: "link.open",
        group: "link",
        icon: "external-link",
        placement: "context-menu",
        when: isAvailable,
      },
    ],
  });
}
