import {
  getMarkraDocumentLinksState,
  getMarkraSlashMenuState,
  listMarkraUi,
  type MarkraDocumentLinksState,
  type MarkraSlashMenuState,
  type MarkraUiAction,
  type MarkraUiPlacement,
} from "@markra/editor/codemirror";
import { useEffect, useMemo, useState } from "react";
import { useMarkraEditorContext } from "./MarkraEditorProvider.tsx";

const emptyActions: readonly MarkraUiAction[] = [];
const emptySlashMenu: MarkraSlashMenuState = {
  actions: emptyActions,
  from: null,
  open: false,
  query: "",
  selectedIndex: 0,
  source: null,
  to: null,
};
const emptyDocumentLinks: MarkraDocumentLinksState = {
  from: null,
  items: [],
  open: false,
  query: "",
  selectedIndex: 0,
  to: null,
};

export interface MarkraEditorSelectionToolbarState {
  readonly actions: readonly MarkraUiAction[];
  readonly from: number | null;
  readonly open: boolean;
  readonly to: number | null;
}

export interface MarkraEditorUiAnchor {
  readonly left: number;
  readonly top: number;
}

type AnchorMode = "caret" | "selection";

function useMeasuredAnchor(
  from: number | null,
  to: number | null,
  mode: AnchorMode,
) {
  const { revision, view } = useMarkraEditorContext();
  const [anchor, setAnchor] = useState<MarkraEditorUiAnchor | null>(null);

  useEffect(() => {
    let active = true;
    if (!view || from === null || to === null) {
      setAnchor(null);
      return () => {
        active = false;
      };
    }

    view.requestMeasure({
      read(editor) {
        return {
          end: editor.coordsAtPos(to),
          start: editor.coordsAtPos(from),
        };
      },
      write(measurement) {
        if (!active) return;
        if (!measurement.start || !measurement.end) {
          setAnchor(null);
          return;
        }
        setAnchor(
          mode === "caret"
            ? {
                left: measurement.end.left,
                top: measurement.end.bottom + 8,
              }
            : {
                left: (measurement.start.left + measurement.end.right) / 2,
                top: Math.min(measurement.start.top, measurement.end.top) - 8,
              },
        );
      },
    });

    return () => {
      active = false;
    };
  }, [from, mode, revision, to, view]);

  return anchor;
}

export function useMarkraEditorCaretAnchor(position: number | null) {
  return useMeasuredAnchor(position, position, "caret");
}

export function useMarkraEditorSelectionAnchor(
  from: number | null,
  to: number | null,
) {
  return useMeasuredAnchor(from, to, "selection");
}

export function useMarkraEditorUi(placement: MarkraUiPlacement) {
  const { revision, view } = useMarkraEditorContext();
  return useMemo(
    () => (view ? listMarkraUi(view, placement) : emptyActions),
    [placement, revision, view],
  );
}

export function useMarkraEditorSlashMenu() {
  const { revision, view } = useMarkraEditorContext();
  return useMemo(
    () => (view ? getMarkraSlashMenuState(view) : emptySlashMenu),
    [revision, view],
  );
}

export function useMarkraEditorDocumentLinks() {
  const { revision, view } = useMarkraEditorContext();
  return useMemo(
    () => (view ? getMarkraDocumentLinksState(view) : emptyDocumentLinks),
    [revision, view],
  );
}

export function useMarkraEditorSelectionToolbar(): MarkraEditorSelectionToolbarState {
  const { revision, view } = useMarkraEditorContext();
  return useMemo(() => {
    if (!view) {
      return { actions: emptyActions, from: null, open: false, to: null };
    }

    const actions = listMarkraUi(view, "selection-toolbar");
    const selection = view.state.selection.main;
    const open = !selection.empty && actions.some((action) => action.enabled);
    return {
      actions,
      from: open ? selection.from : null,
      open,
      to: open ? selection.to : null,
    };
  }, [revision, view]);
}
