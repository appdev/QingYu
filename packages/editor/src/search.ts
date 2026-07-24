import type { Node as ProseNode } from "@milkdown/kit/prose/model";
import { Plugin, PluginKey, type EditorState } from "@milkdown/kit/prose/state";
import { Decoration, DecorationSet, type EditorView } from "@milkdown/kit/prose/view";
import { $prose } from "@milkdown/kit/utils";
import { findSearchRanges, type SearchRange } from "@markra/shared";
import { findHiddenDisplayMathSourceRanges, setMathCaretAnchorSuppressedMeta } from "./math.ts";

type TextSegment = {
  end: number;
  from: number;
  start: number;
};

type SearchState = {
  activeIndex: number;
  decorations: DecorationSet;
  matches: SearchRange[];
};

type SearchMeta = {
  activeIndex: number;
  matches: SearchRange[];
};

type SearchDecorationOptions = {
  suppressEditorChrome?: boolean;
};

const emptySearchState: SearchState = {
  activeIndex: -1,
  decorations: DecorationSet.empty,
  matches: []
};

const searchKey = new PluginKey<SearchState>("markra-search");
const currentSearchMatchSelector = "[data-markra-search-current=\"true\"]";

export function findSearchMatchesInDoc(
  doc: ProseNode,
  query: string,
  options: { caseSensitive?: boolean } = {}
): SearchRange[] {
  if (!query) return [];

  const matches: SearchRange[] = [];

  doc.descendants((node, position) => {
    if (!node.isTextblock) return true;

    const segments: TextSegment[] = [];
    let text = "";

    node.descendants((child, offset) => {
      if (!child.isText || !child.text) return true;

      const start = text.length;
      text += child.text;
      segments.push({
        end: text.length,
        from: position + 1 + offset,
        start
      });

      return true;
    });

    for (const range of findSearchRanges(text, query, options)) {
      const from = positionForTextOffset(segments, range.from);
      const to = positionForTextOffset(segments, range.to);
      if (from !== null && to !== null && from < to) {
        matches.push({ from, to });
      }
    }

    return false;
  });

  return matches;
}

export function findVisibleSearchMatchesInState(
  state: EditorState,
  query: string,
  options: { caseSensitive?: boolean } = {}
): SearchRange[] {
  const matches = findSearchMatchesInDoc(state.doc, query, options);
  if (matches.length === 0) return matches;

  const hiddenDisplayMathSourceRanges = findHiddenDisplayMathSourceRanges(state);
  if (hiddenDisplayMathSourceRanges.length === 0) return matches;

  return matches.filter((match) => !rangeOverlapsHiddenDisplayMathSource(match, hiddenDisplayMathSourceRanges));
}

export function markraSearchPlugin() {
  return $prose(() => new Plugin<SearchState>({
    key: searchKey,
    props: {
      decorations(state) {
        return searchKey.getState(state)?.decorations ?? DecorationSet.empty;
      }
    },
    state: {
      init() {
        return emptySearchState;
      },
      apply(transaction, previous, _oldState, newState) {
        const meta = transaction.getMeta(searchKey) as SearchMeta | undefined;
        if (!meta) {
          const matches = mapSearchRanges(previous.matches, transaction.mapping);
          return {
            activeIndex: previous.activeIndex,
            decorations: buildSearchDecorations(newState, matches, previous.activeIndex),
            matches
          };
        }

        return {
          activeIndex: meta.activeIndex,
          decorations: buildSearchDecorations(newState, meta.matches, meta.activeIndex),
          matches: meta.matches
        };
      }
    }
  }));
}

export function updateSearchDecorations(
  view: EditorView,
  matches: SearchRange[],
  activeIndex: number,
  options: SearchDecorationOptions = {}
) {
  const transaction = setMathCaretAnchorSuppressedMeta(
    view.state.tr.setMeta(searchKey, {
      activeIndex,
      matches
    } satisfies SearchMeta),
    Boolean(options.suppressEditorChrome)
  );

  view.dispatch(transaction);
}

export function scrollSearchMatchIntoView(view: EditorView) {
  const currentMatch = view.dom.querySelector(currentSearchMatchSelector);
  if (!(currentMatch instanceof HTMLElement) || typeof currentMatch.scrollIntoView !== "function") return false;

  currentMatch.scrollIntoView({
    block: "center",
    inline: "nearest"
  });
  return true;
}

function buildSearchDecorations(state: EditorState, matches: SearchRange[], activeIndex: number) {
  if (matches.length === 0) return DecorationSet.empty;

  const hiddenDisplayMathSourceRanges = findHiddenDisplayMathSourceRanges(state);

  return DecorationSet.create(
    state.doc,
    matches.flatMap((match, index) => {
      if (rangeOverlapsHiddenDisplayMathSource(match, hiddenDisplayMathSourceRanges)) return [];

      const current = index === activeIndex;
      return [Decoration.inline(match.from, match.to, current
        ? {
            class: "markra-search-match markra-search-match-current",
            "data-markra-search-current": "true"
          }
        : {
            class: "markra-search-match"
          })];
    })
  );
}

function mapSearchRanges(matches: SearchRange[], mapping: Parameters<DecorationSet["map"]>[0]) {
  return matches.flatMap((match) => {
    const from = mapping.map(match.from, 1);
    const to = mapping.map(match.to, -1);

    return from < to ? [{ from, to }] : [];
  });
}

function rangeOverlapsHiddenDisplayMathSource(
  match: SearchRange,
  hiddenDisplayMathSourceRanges: SearchRange[]
) {
  return hiddenDisplayMathSourceRanges.some((range) => match.from < range.to && range.from < match.to);
}

function positionForTextOffset(segments: TextSegment[], offset: number) {
  for (const segment of segments) {
    if (offset >= segment.start && offset <= segment.end) {
      return segment.from + offset - segment.start;
    }
  }

  return null;
}
