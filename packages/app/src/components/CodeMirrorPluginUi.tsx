import {
  useMarkraEditorSelectionAnchor,
  useMarkraEditorSelectionToolbar,
  useMarkraEditorUi,
  useMarkraEditorView,
} from "@markra/editor-react";
import {
  useEffect,
  useMemo,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { fitCodeMirrorFloatingMenu } from "./CodeMirrorEditorFloatingMenus";

export interface CodeMirrorPluginUiProps {
  pluginIds: readonly string[];
}

function preserveEditorSelection(event: ReactMouseEvent<HTMLButtonElement>) {
  event.preventDefault();
}

export function CodeMirrorPluginUi({ pluginIds }: CodeMirrorPluginUiProps) {
  const view = useMarkraEditorView();
  const pluginIdSet = useMemo(() => new Set(pluginIds), [pluginIds]);
  const toolbar = useMarkraEditorUi("toolbar").filter((action) =>
    pluginIdSet.has(action.plugin));
  const contextActions = useMarkraEditorUi("context-menu").filter((action) =>
    pluginIdSet.has(action.plugin));
  const selectionToolbar = useMarkraEditorSelectionToolbar();
  const selectionActions = selectionToolbar.actions.filter((action) =>
    pluginIdSet.has(action.plugin));
  const selectionAnchor = useMarkraEditorSelectionAnchor(
    selectionActions.length > 0 ? selectionToolbar.from : null,
    selectionActions.length > 0 ? selectionToolbar.to : null,
  );
  const [contextPoint, setContextPoint] = useState<{
    left: number;
    top: number;
  } | null>(null);

  useEffect(() => {
    if (!view || contextActions.length === 0) return;
    const openContextMenu = (event: MouseEvent) => {
      event.preventDefault();
      setContextPoint({ left: event.clientX, top: event.clientY });
    };
    const closeContextMenu = (event: Event) => {
      const target = event.target instanceof Element
        ? event.target
        : event.target instanceof Node
          ? event.target.parentElement
          : null;
      if (target?.closest(".markra-plugin-context-menu")) return;
      setContextPoint(null);
    };
    const closeContextMenuOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setContextPoint(null);
    };
    view.dom.addEventListener("contextmenu", openContextMenu);
    view.dom.ownerDocument.addEventListener("pointerdown", closeContextMenu);
    view.dom.ownerDocument.addEventListener("scroll", closeContextMenu, true);
    view.dom.ownerDocument.addEventListener("keydown", closeContextMenuOnEscape);
    return () => {
      view.dom.removeEventListener("contextmenu", openContextMenu);
      view.dom.ownerDocument.removeEventListener("pointerdown", closeContextMenu);
      view.dom.ownerDocument.removeEventListener("scroll", closeContextMenu, true);
      view.dom.ownerDocument.removeEventListener("keydown", closeContextMenuOnEscape);
    };
  }, [contextActions.length, view]);

  const runAction = (run: () => boolean) => {
    run();
    setContextPoint(null);
    view?.focus();
  };
  const selectionStyle = selectionAnchor
    ? fitCodeMirrorFloatingMenu(
        { left: selectionAnchor.left - 120, top: selectionAnchor.top - 44 },
        { height: 36, width: 240 },
        { height: window.innerHeight, width: window.innerWidth },
      )
    : null;
  const contextStyle = contextPoint
    ? fitCodeMirrorFloatingMenu(
        contextPoint,
        { height: Math.max(36, contextActions.length * 32 + 8), width: 240 },
        { height: window.innerHeight, width: window.innerWidth },
      )
    : null;

  return (
    <>
      {toolbar.length > 0 ? (
        <div aria-label="Editor plugin toolbar" className="markra-plugin-toolbar" role="toolbar">
          {toolbar.map((action) => (
            <button
              aria-pressed={action.active}
              disabled={!action.enabled}
              key={`${action.plugin}:${action.command}`}
              onClick={() => runAction(action.run)}
              onMouseDown={preserveEditorSelection}
              type="button"
            >
              {action.label}
            </button>
          ))}
        </div>
      ) : null}

      {selectionStyle && selectionActions.length > 0 ? (
        <div
          aria-label="Editor plugin selection actions"
          className="markra-plugin-selection-toolbar"
          role="toolbar"
          style={selectionStyle}
        >
          {selectionActions.map((action) => (
            <button
              aria-pressed={action.active}
              disabled={!action.enabled}
              key={`${action.plugin}:${action.command}`}
              onClick={() => runAction(action.run)}
              onMouseDown={preserveEditorSelection}
              type="button"
            >
              {action.label}
            </button>
          ))}
        </div>
      ) : null}

      {contextStyle ? (
        <div
          aria-label="Editor plugin context menu"
          className="markra-plugin-context-menu"
          role="menu"
          style={contextStyle}
        >
          {contextActions.map((action) => (
            <button
              disabled={!action.enabled}
              key={`${action.plugin}:${action.command}`}
              onClick={() => runAction(action.run)}
              onMouseDown={preserveEditorSelection}
              role="menuitem"
              type="button"
            >
              {action.label}
            </button>
          ))}
        </div>
      ) : null}
    </>
  );
}
