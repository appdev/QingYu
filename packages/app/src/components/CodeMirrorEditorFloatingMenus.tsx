import {
  useMarkraEditorCaretAnchor,
  useMarkraEditorDocumentLinks,
  useMarkraEditorSlashMenu,
} from "@markra/editor-react";
import type { CSSProperties, MouseEvent } from "react";

export interface CodeMirrorEditorFloatingMenusProps {
  documentLinksLabel?: string;
  slashMenuEmptyLabel?: string;
  slashMenuLabel?: string;
}

function keepEditorSelection(event: MouseEvent<HTMLElement>) {
  event.preventDefault();
}

export function CodeMirrorEditorFloatingMenus({
  documentLinksLabel = "Document links",
  slashMenuEmptyLabel = "No matching commands",
  slashMenuLabel = "Insert block",
}: CodeMirrorEditorFloatingMenusProps) {
  const slashMenu = useMarkraEditorSlashMenu();
  const documentLinks = useMarkraEditorDocumentLinks();
  const slashAnchor = useMarkraEditorCaretAnchor(slashMenu.to);
  const documentLinkAnchor = useMarkraEditorCaretAnchor(documentLinks.to);
  const slashStyle = slashAnchor
    ? ({ left: slashAnchor.left, top: slashAnchor.top } satisfies CSSProperties)
    : undefined;
  const documentLinkStyle = documentLinkAnchor
    ? ({
        left: documentLinkAnchor.left,
        top: documentLinkAnchor.top,
      } satisfies CSSProperties)
    : undefined;

  return (
    <>
      {slashMenu.open && slashStyle ? (
        <div
          aria-label={slashMenuLabel}
          className="markra-slash-menu"
          role="menu"
          style={slashStyle}
        >
          {slashMenu.actions.length > 0 ? (
            slashMenu.actions.map((action, index) => (
              <button
                aria-selected={index === slashMenu.selectedIndex}
                className="markra-slash-menu-option"
                key={action.command}
                onClick={() => {
                  action.run();
                }}
                onMouseDown={keepEditorSelection}
                role="menuitem"
                type="button"
              >
                {action.label}
              </button>
            ))
          ) : (
            <div className="markra-slash-menu-empty">{slashMenuEmptyLabel}</div>
          )}
        </div>
      ) : null}

      {documentLinks.open && documentLinkStyle ? (
        <div
          aria-label={documentLinksLabel}
          className="markra-document-link-menu"
          role="listbox"
          style={documentLinkStyle}
        >
          {documentLinks.items.map((item, index) => (
            <button
              aria-selected={index === documentLinks.selectedIndex}
              className="markra-document-link-option w-full border-0 bg-transparent"
              key={item.id}
              onClick={() => {
                item.run();
              }}
              onMouseDown={keepEditorSelection}
              role="option"
              type="button"
            >
              <span className="markra-document-link-title">{item.label}</span>
              {item.detail ? (
                <span className="markra-document-link-path">{item.detail}</span>
              ) : null}
            </button>
          ))}
        </div>
      ) : null}
    </>
  );
}
