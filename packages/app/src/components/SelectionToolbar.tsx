import { Fragment, useEffect, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import { Tooltip } from "@markra/ui";
import {
  Bold,
  Check,
  Code2,
  Copy,
  Eraser,
  Heading,
  Heading1,
  Heading2,
  Heading3,
  Heading4,
  Heading5,
  Heading6,
  Highlighter,
  Italic,
  Link,
  List,
  ListOrdered,
  Pilcrow,
  Quote,
  Strikethrough,
  type LucideIcon
} from "lucide-react";
import { t, type AppLanguage, type I18nKey } from "@markra/shared";
import { anchoredPopoverStyle } from "../lib/anchored-popover";
import type { SelectionAnchor } from "../lib/selection-anchor";
import {
  selectionHeadingLevels,
  type SelectionHeadingLevel,
  type SelectionFormattingAction,
  type SelectionFormattingToolbarAction
} from "../lib/selection-formatting";

type BasicSelectionAction = {
  action: SelectionFormattingToolbarAction;
  icon: LucideIcon;
  labelKey: I18nKey;
};

type HeadingLevelOption = {
  icon: LucideIcon;
  label: string;
  level: SelectionHeadingLevel;
  type: "heading";
};

type ParagraphLevelOption = {
  icon: LucideIcon;
  labelKey: I18nKey;
  type: "paragraph";
};

type HeadingMenuOption = HeadingLevelOption | ParagraphLevelOption;

type SelectionToolbarProps = {
  anchor: SelectionAnchor | null;
  copySucceeded?: boolean;
  language?: AppLanguage;
  open: boolean;
  activeFormattingActions?: readonly SelectionFormattingAction[];
  activeHeadingLevel?: SelectionHeadingLevel | null;
  onCopySelection: () => unknown;
  onDismiss?: () => unknown;
  onInsertLink: () => unknown;
  onRunFormattingAction: (action: SelectionFormattingToolbarAction) => unknown;
  onSetHeadingLevel?: (level: SelectionHeadingLevel) => unknown;
};

const toolbarOffsetPx = 12;
const toolbarViewportMarginPx = 12;
const toolbarHeightPx = 42;

function selectionToolbarPlacement(anchor: SelectionAnchor) {
  return anchor.top - toolbarOffsetPx - toolbarHeightPx >= toolbarViewportMarginPx ? "top" : "bottom";
}

function selectionToolbarTop(anchor: SelectionAnchor, placement: ReturnType<typeof selectionToolbarPlacement>) {
  if (placement === "top") {
    return Math.round(anchor.top - toolbarOffsetPx);
  }

  const viewportHeight = typeof window === "undefined" ? Number.POSITIVE_INFINITY : window.innerHeight;
  const maxTop = Math.max(toolbarViewportMarginPx, viewportHeight - toolbarViewportMarginPx - toolbarHeightPx);

  return Math.round(Math.max(toolbarViewportMarginPx, Math.min(anchor.bottom + toolbarOffsetPx, maxTop)));
}

const basicSelectionActions: BasicSelectionAction[] = [
  {
    action: "bold",
    icon: Bold,
    labelKey: "menu.bold"
  },
  {
    action: "italic",
    icon: Italic,
    labelKey: "menu.italic"
  },
  {
    action: "strikethrough",
    icon: Strikethrough,
    labelKey: "menu.strikethrough"
  },
  {
    action: "inlineCode",
    icon: Code2,
    labelKey: "menu.inlineCode"
  },
  {
    action: "highlight",
    icon: Highlighter,
    labelKey: "menu.highlight"
  },
  {
    action: "clearFormatting",
    icon: Eraser,
    labelKey: "menu.clearFormatting"
  },
  {
    action: "quote",
    icon: Quote,
    labelKey: "menu.quote"
  },
  {
    action: "bulletList",
    icon: List,
    labelKey: "menu.bulletList"
  },
  {
    action: "orderedList",
    icon: ListOrdered,
    labelKey: "menu.orderedList"
  }
];

const headingLevelIcons: Record<SelectionHeadingLevel, LucideIcon> = {
  1: Heading1,
  2: Heading2,
  3: Heading3,
  4: Heading4,
  5: Heading5,
  6: Heading6
};

const headingLevelOptions: HeadingLevelOption[] = selectionHeadingLevels.map((level) => ({
  icon: headingLevelIcons[level],
  label: `H${level}`,
  level,
  type: "heading"
}));
const headingMenuOptions: HeadingMenuOption[] = [
  {
    icon: Pilcrow,
    labelKey: "menu.paragraph",
    type: "paragraph"
  },
  ...headingLevelOptions
];

export function SelectionToolbar({
  anchor,
  copySucceeded = false,
  language = "en",
  open,
  activeFormattingActions = [],
  activeHeadingLevel = null,
  onCopySelection,
  onDismiss,
  onInsertLink,
  onRunFormattingAction,
  onSetHeadingLevel = () => {}
}: SelectionToolbarProps) {
  const [headingLevelMenuOpen, setHeadingLevelMenuOpen] = useState(false);
  const [headingLevelMenuStyle, setHeadingLevelMenuStyle] = useState<CSSProperties | null>(null);
  const toolbarRef = useRef<HTMLElement | null>(null);
  const headingLevelButtonRef = useRef<HTMLButtonElement | null>(null);
  const headingLevelMenuRef = useRef<HTMLDivElement | null>(null);

  const positionHeadingLevelMenu = (button: HTMLButtonElement) => {
    setHeadingLevelMenuStyle(
      anchoredPopoverStyle(button, headingLevelMenuRef.current, {
        fallbackSize: {
          height: 112,
          width: 112
        },
        gap: 4
      })
    );
  };

  useEffect(() => {
    if (!open) {
      setHeadingLevelMenuOpen(false);
      setHeadingLevelMenuStyle(null);
    }
  }, [open]);

  useEffect(() => {
    setHeadingLevelMenuOpen(false);
    setHeadingLevelMenuStyle(null);
  }, [anchor?.bottom, anchor?.left, anchor?.right, anchor?.top]);

  useEffect(() => {
    if (!headingLevelMenuOpen) return;

    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;

      if (headingLevelButtonRef.current?.contains(target) || headingLevelMenuRef.current?.contains(target)) {
        return;
      }

      setHeadingLevelMenuOpen(false);
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setHeadingLevelMenuOpen(false);
      }
    };

    const handleLayoutChange = () => {
      const button = headingLevelButtonRef.current;
      if (button) {
        positionHeadingLevelMenu(button);
      }
    };

    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    window.addEventListener("resize", handleLayoutChange);
    window.addEventListener("scroll", handleLayoutChange, true);

    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("resize", handleLayoutChange);
      window.removeEventListener("scroll", handleLayoutChange, true);
    };
  }, [headingLevelMenuOpen]);

  useEffect(() => {
    if (!open || !anchor || !onDismiss) return;

    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;

      if (toolbarRef.current?.contains(target) || headingLevelMenuRef.current?.contains(target)) return;

      onDismiss();
    };

    document.addEventListener("pointerdown", handlePointerDown, true);

    return () => {
      document.removeEventListener("pointerdown", handlePointerDown, true);
    };
  }, [anchor, onDismiss, open]);

  if (!open || !anchor) return null;

  const label = (key: I18nKey) => t(language, key);
  const activeFormattingActionSet = new Set(activeFormattingActions);
  const activeHeadingLevelOption =
    activeHeadingLevel === null
      ? null
      : headingLevelOptions.find((option) => option.level === activeHeadingLevel) ?? null;
  const ActiveHeadingLevelIcon = activeHeadingLevelOption?.icon ?? Heading;
  const headingLevelLabel = activeHeadingLevelOption
    ? `${label("menu.headingLevel")} ${activeHeadingLevelOption.label}`
    : label("menu.headingLevel");
  const copyLabel = label(copySucceeded ? "app.copySucceeded" : "menu.copy");
  const toolbarPlacement = selectionToolbarPlacement(anchor);
  const style: CSSProperties = {
    left: `${Math.round((anchor.left + anchor.right) / 2)}px`,
    top: `${selectionToolbarTop(anchor, toolbarPlacement)}px`
  };
  const toolbarPlacementClass = toolbarPlacement === "top" ? "-translate-y-full" : "translate-y-0";
  const headingLevelMenu =
    headingLevelMenuOpen && headingLevelMenuStyle && typeof document !== "undefined"
      ? createPortal(
          <div
            className="pointer-events-auto fixed z-60 grid grid-cols-3 gap-1 rounded-md border border-(--border-default) bg-(--bg-primary) p-1 shadow-(--floating-popover-shadow)"
            ref={headingLevelMenuRef}
            style={headingLevelMenuStyle}
            role="menu"
            aria-label={label("menu.headingLevel")}
          >
            {headingMenuOptions.map((option) => {
              const HeadingLevelIcon = option.icon;
              const optionLabel = option.type === "paragraph" ? label(option.labelKey) : option.label;
              const selected = option.type === "heading" && option.level === activeHeadingLevel;

              return (
                <Tooltip key={option.type === "paragraph" ? "paragraph" : option.level} content={optionLabel}>
                <button
                  className={`inline-flex size-8 cursor-pointer items-center justify-center rounded-md border-0 transition-colors duration-150 ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent) ${
                    selected
                      ? "bg-(--accent-soft) text-(--accent) hover:bg-(--accent-soft)"
                      : "bg-transparent text-(--text-primary) hover:bg-(--bg-hover) hover:text-(--text-heading) focus-visible:bg-(--bg-hover) focus-visible:text-(--text-heading)"
                  }`}
                  type="button"
                  role="menuitemradio"
                  aria-label={optionLabel}
                  aria-checked={selected}
                  onClick={() => {
                    setHeadingLevelMenuOpen(false);
                    if (option.type === "paragraph") {
                      onRunFormattingAction("paragraph");
                      return;
                    }

                    onSetHeadingLevel(option.level);
                  }}
                >
                  <HeadingLevelIcon aria-hidden="true" size={15} />
                </button>
                </Tooltip>
              );
            })}
          </div>,
          document.body
        )
      : null;

  return (
    <section
      className={`selection-toolbar pointer-events-none fixed z-50 -translate-x-1/2 ${toolbarPlacementClass} animate-[markra-float-in_160ms_ease-out_both] motion-reduce:animate-none`}
      ref={toolbarRef}
      style={style}
      role="toolbar"
      aria-label={label("menu.format")}
    >
      <div className="pointer-events-auto inline-flex max-w-[calc(100vw-24px)] items-center gap-1 overflow-x-auto rounded-lg border border-(--border-default) bg-(--bg-primary) p-1 shadow-(--floating-popover-shadow)">
        {basicSelectionActions.map((action) => {
          const Icon = action.icon;
          const actionLabel = label(action.labelKey);
          const active = activeFormattingActionSet.has(action.action);

          return (
            <Fragment key={action.action}>
              <Tooltip content={actionLabel}>
                <button
                  className={`inline-flex size-8 shrink-0 cursor-pointer items-center justify-center rounded-md border-0 transition-colors duration-150 ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent) disabled:cursor-default disabled:opacity-55 ${
                    active
                      ? "bg-(--accent-soft) text-(--accent) hover:bg-(--accent-soft)"
                      : "bg-transparent text-(--text-primary) hover:bg-(--bg-hover) hover:text-(--text-heading) focus-visible:bg-(--bg-hover) focus-visible:text-(--text-heading)"
                  }`}
                  type="button"
                  onClick={() => onRunFormattingAction(action.action)}
                  aria-label={actionLabel}
                  aria-pressed={active}
                >
                  <Icon aria-hidden="true" size={14} />
                </button>
              </Tooltip>
              {action.action === "highlight" ? (
                <div className="relative inline-flex shrink-0">
                  <Tooltip content={headingLevelLabel}>
                    <button
                      className={`inline-flex size-8 shrink-0 cursor-pointer items-center justify-center rounded-md border-0 transition-colors duration-150 ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent) disabled:cursor-default disabled:opacity-55 ${
                        activeHeadingLevelOption
                          ? "bg-(--accent-soft) text-(--accent) hover:bg-(--accent-soft)"
                          : "bg-transparent text-(--text-primary) hover:bg-(--bg-hover) hover:text-(--text-heading) focus-visible:bg-(--bg-hover) focus-visible:text-(--text-heading)"
                      }`}
                      ref={headingLevelButtonRef}
                      type="button"
                      onClick={(event) => {
                        if (!headingLevelMenuOpen) {
                          positionHeadingLevelMenu(event.currentTarget);
                        }
                        setHeadingLevelMenuOpen(!headingLevelMenuOpen);
                      }}
                      aria-label={headingLevelLabel}
                      aria-haspopup="menu"
                      aria-expanded={headingLevelMenuOpen}
                    >
                      <ActiveHeadingLevelIcon aria-hidden="true" size={15} />
                    </button>
                  </Tooltip>
                </div>
              ) : null}
            </Fragment>
          );
        })}
        <Tooltip content={label("menu.link")}>
          <button
            className={`inline-flex size-8 shrink-0 cursor-pointer items-center justify-center rounded-md border-0 transition-colors duration-150 ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent) disabled:cursor-default disabled:opacity-55 ${
              activeFormattingActionSet.has("link")
                ? "bg-(--accent-soft) text-(--accent) hover:bg-(--accent-soft)"
                : "bg-transparent text-(--text-primary) hover:bg-(--bg-hover) hover:text-(--text-heading) focus-visible:bg-(--bg-hover) focus-visible:text-(--text-heading)"
            }`}
            type="button"
            onClick={onInsertLink}
            aria-label={label("menu.link")}
            aria-pressed={activeFormattingActionSet.has("link")}
          >
            <Link aria-hidden="true" size={14} />
          </button>
        </Tooltip>
        <span className="mx-0.5 h-5 w-px bg-(--border-default)" aria-hidden="true" />
        <Tooltip content={copyLabel}>
          <button
            className={`inline-flex size-8 shrink-0 cursor-pointer items-center justify-center rounded-md border-0 transition-colors duration-150 ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent) disabled:cursor-default disabled:opacity-55 ${
              copySucceeded
                ? "bg-(--accent-soft) text-(--accent) hover:bg-(--accent-soft)"
                : "bg-transparent text-(--text-primary) hover:bg-(--bg-hover) hover:text-(--text-heading) focus-visible:bg-(--bg-hover) focus-visible:text-(--text-heading)"
            }`}
            type="button"
            onClick={onCopySelection}
            aria-label={copyLabel}
          >
            {copySucceeded ? <Check aria-hidden="true" size={14} /> : <Copy aria-hidden="true" size={14} />}
          </button>
        </Tooltip>
      </div>
      {headingLevelMenu}
    </section>
  );
}
