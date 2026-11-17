import { syntaxTree } from "@codemirror/language";
import { EditorSelection, Prec, type EditorState, type Text } from "@codemirror/state";
import { EditorView, keymap, type EditorView as CodeMirrorView } from "@codemirror/view";
import { defineMarkraPlugin } from "./plugin.ts";
import type { MarkraSyntaxNode } from "./renderers.ts";
import { unescapeMarkdown } from "./syntax.ts";
import { readWikiLink } from "./wiki-link.ts";

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

type LinkPress = { doc: Text; position: number; x: number; y: number };

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

export function resolveAutolinkTarget(source: string) {
  const candidate = unescapeMarkdown(source.trim());
  if (scheme.test(candidate)) return resolveSafeLinkTarget(candidate);
  if (/^[^\s@<>]+@[^\s@<>]+\.[^\s@<>]+$/u.test(candidate)) {
    return resolveSafeLinkTarget(`mailto:${candidate}`);
  }
  if (/^www\./iu.test(candidate)) {
    return resolveSafeLinkTarget(`http://${candidate}`);
  }
  return resolveSafeLinkTarget(candidate);
}

function normalizeMarkdownLinkLabel(source: string) {
  return unescapeMarkdown(source.replace(/^\[|\]$/gu, ""))
    .trim()
    .replace(/\s+/gu, " ")
    .toLocaleLowerCase();
}

export function readMarkdownLinkReferences(state: EditorState) {
  const references = new Map<string, string>();

  syntaxTree(state).iterate({
    enter(node) {
      if (node.name !== "LinkReference") return;
      const label = node.node.getChild("LinkLabel");
      const url = node.node.getChild("URL");
      if (!label || !url) return;
      const source = unescapeMarkdown(state.sliceDoc(url.from, url.to).trim());
      if (!source) return;
      references.set(
        normalizeMarkdownLinkLabel(state.sliceDoc(label.from, label.to)),
        source,
      );
    },
  });

  return references;
}

export function readMarkdownLinkDestination(
  state: EditorState,
  node: MarkraSyntaxNode,
  references = readMarkdownLinkReferences(state),
) {
  const url = node.name === "URL" ? node : node.getChild("URL");
  if (url) {
    const source = unescapeMarkdown(state.sliceDoc(url.from, url.to).trim());
    return source || null;
  }
  if (node.name !== "Link" || node.getChildren("LinkMark").length !== 2) {
    return null;
  }

  const label = node.getChild("LinkLabel");
  // Shortcut and collapsed references use the visible label; empty inline URLs must not fall back to a reference.
  const source = label && label.to > label.from + 2
    ? state.sliceDoc(label.from, label.to)
    : state.sliceDoc(node.from, label?.from ?? node.to);
  return references.get(
    normalizeMarkdownLinkLabel(source),
  ) ?? null;
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
      if (node.name === "Link" || node.name === "Autolink" || node.name === "WikiLink") return node;
      if (
        node.name === "URL" &&
        node.parent?.name !== "Image" &&
        node.parent?.name !== "Link" &&
        node.parent?.name !== "Autolink"
      ) {
        return node;
      }
      node = node.parent;
    }
  }
  return null;
}

function linkSource(state: EditorState, node: MarkraSyntaxNode) {
  if (node.name === "WikiLink") return readWikiLink(state.sliceDoc(node.from, node.to))?.href ?? null;
  const source = readMarkdownLinkDestination(state, node);
  if (!source) return null;
  return node.name === "Link" ? source : resolveAutolinkTarget(source);
}

function resolvedLinkSource(
  view: CodeMirrorView,
  source: string,
  resolver: LinksPluginOptions["resolveTarget"],
): ResolvedLink | null {
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

function resolvedLinkAt(
  view: CodeMirrorView,
  position: number,
  resolver: LinksPluginOptions["resolveTarget"],
): ResolvedLink | null {
  const node = linkNodeAt(view.state, position);
  if (!node) return null;
  const source = linkSource(view.state, node);
  if (!source) return null;
  return resolvedLinkSource(view, source, resolver);
}

function openResolvedLink(
  view: CodeMirrorView,
  link: ResolvedLink,
  options: LinksPluginOptions,
) {
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

export function openMarkraLinkSource(
  view: CodeMirrorView,
  source: string,
  options: LinksPluginOptions,
) {
  const link = resolvedLinkSource(view, source, options.resolveTarget);
  return link ? openResolvedLink(view, link, options) : false;
}

function openLinkAt(
  view: CodeMirrorView,
  position: number,
  options: LinksPluginOptions,
) {
  const link = resolvedLinkAt(view, position, options.resolveTarget);
  return link ? openResolvedLink(view, link, options) : false;
}

function linkPositionFromEvent(event: Event, view: CodeMirrorView) {
  const target = event.target;
  if (!(target instanceof Element)) return null;
  const link = target.closest("a.cm-markra-link[href]");
  if (!link || !view.contentDOM.contains(link) || link.closest(".cm-markra-table")) return null;

  try {
    return view.posAtDOM(link, 0);
  } catch {
    return null;
  }
}

function revealLinkSourceAt(view: CodeMirrorView, position: number, address = false) {
  const node = linkNodeAt(view.state, position);
  if (!node) return false;
  const url = node.name === "URL" ? node : node.getChild("URL");
  const marks = node.name === "Link" ? node.getChildren("LinkMark") : [];
  const labelFrom = marks[0]?.to;
  const labelTo = marks[1]?.from;
  const anchor = address && url
    ? url.from
    : labelFrom !== undefined && labelTo !== undefined
    ? labelFrom < labelTo
      ? Math.min(labelFrom + 1, labelTo)
      : Math.min(node.from + 1, node.to)
    : Math.min((url?.from ?? node.from) + 1, url?.to ?? node.to);

  // Live preview only reveals an active node while the editor owns focus, so
  // focus before dispatching the selection that switches this link to source.
  view.focus();
  view.dispatch({
    selection: EditorSelection.cursor(anchor),
    scrollIntoView: true,
  });
  return true;
}

function syncLinkModifierCursor(
  view: CodeMirrorView,
  event: Pick<KeyboardEvent | MouseEvent, "ctrlKey" | "metaKey">,
) {
  if (event.metaKey || event.ctrlKey) {
    view.dom.dataset.markraLinkModifier = "true";
  } else {
    delete view.dom.dataset.markraLinkModifier;
  }
}

export function linksPlugin(options: LinksPluginOptions) {
  const activation = options.activation ?? "modifier";
  const pressed = new WeakMap<CodeMirrorView, LinkPress>();
  const tapped = new WeakMap<CodeMirrorView, LinkPress & { finger: number; start: number }>();
  const release = (view: CodeMirrorView, press: LinkPress, point: Pick<MouseEvent, "clientX" | "clientY">) =>
    press.doc === view.state.doc && !view.compositionStarted && view.state.selection.main.empty &&
    Math.hypot(point.clientX - press.x, point.clientY - press.y) <= 4 &&
    openLinkAt(view, press.position, options);
  const openAtCaret = (view: CodeMirrorView) =>
    !view.compositionStarted && openLinkAt(view, view.state.selection.main.head, options);
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
        label: options.label ?? "Open link",
        run: openAtCaret,
      },
    ],
    extension: [
      Prec.high(keymap.of([{ key: "Mod-Enter", run: openAtCaret }])),
      activation === "none"
        ? []
        : EditorView.domEventHandlers({
            blur(_event, view) {
              delete view.dom.dataset.markraLinkModifier;
              return false;
            },
            click(event, view) {
              const position = linkPositionFromEvent(event, view);
              if (position === null) return false;
              event.preventDefault();
              if (activation === "click" && event.detail === 0 && !view.compositionStarted) openLinkAt(view, position, options);
              return true;
            },
            mouseup(event, view) {
              const press = pressed.get(view);
              pressed.delete(view);
              // WebKit skips click if the caret has replaced the rendered anchor.
              return Boolean(press && event.button === 0 && !event.shiftKey && !event.altKey && release(view, press, event));
            },
            touchstart(event, view) {
              tapped.delete(view);
              if (activation !== "click" || event.touches.length !== 1 || view.compositionStarted) return false;
              const position = linkPositionFromEvent(event, view);
              if (position === null) return false;
              const touch = event.touches[0];
              tapped.set(view, { doc: view.state.doc, position, x: touch.clientX, y: touch.clientY,
                finger: touch.identifier, start: event.timeStamp });
              return false;
            },
            touchend(event, view) {
              const tap = tapped.get(view);
              tapped.delete(view);
              if (!tap || !event.cancelable || event.touches.length || event.timeStamp - tap.start >= 500) return false;
              const touch = [...event.changedTouches].find(touch => touch.identifier === tap.finger);
              // Consume a tap before iOS synthesizes mousedown and focuses the writing area.
              return Boolean(touch && release(view, tap, touch));
            },
            touchcancel(_event, view) {
              tapped.delete(view);
              return false;
            },
            keydown(event, view) {
              syncLinkModifierCursor(view, event);
              return false;
            },
            keyup(event, view) {
              syncLinkModifierCursor(view, event);
              return false;
            },
            mousedown(event, view) {
              pressed.delete(view);
              if (event.button !== 0) return false;
              const position = linkPositionFromEvent(event, view);
              if (position === null) return false;
              if (activation === "click") {
                if (view.compositionStarted || event.shiftKey) return false;
                if (event.altKey && !view.state.readOnly) return revealLinkSourceAt(view, position, true);
                pressed.set(view, { doc: view.state.doc, position, x: event.clientX, y: event.clientY });
                return false;
              }
              if (
                activation === "modifier" &&
                (linkNodeAt(view.state, position)?.name !== "WikiLink" || event.altKey) &&
                !event.metaKey &&
                !event.ctrlKey
              ) {
                if (!revealLinkSourceAt(view, position)) return false;
                event.preventDefault();
                return true;
              }

              // CodeMirror moves the selection on mousedown. Intercepting here
              // keeps the rendered link DOM alive long enough to resolve it.
              if (!openLinkAt(view, position, options)) return false;
              event.preventDefault();
              return true;
            },
            mousemove(event, view) {
              syncLinkModifierCursor(view, event);
              return false;
            },
          }),
    ],
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
