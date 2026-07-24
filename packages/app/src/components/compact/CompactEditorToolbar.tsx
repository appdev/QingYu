import {
  Bold,
  Code,
  Heading1,
  Heading2,
  Heading3,
  Image,
  Italic,
  KeyboardOff,
  Link,
  List,
  ListChecks,
  ListOrdered,
  Pilcrow,
  Quote,
  Redo2,
  SquareCode,
  Strikethrough,
  Undo2,
  type LucideIcon
} from "lucide-react";
import { useCallback, useEffect, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from "react";
import { t, type AppLanguage } from "@markra/shared";
import type { CompactEditorController } from "./types";

type CompactEditorToolbarProps = {
  disabled?: boolean;
  editor: Omit<CompactEditorController, "host" | "readOnly">;
  imageImport?: boolean;
  language: AppLanguage;
  trueMobile?: boolean;
};

type ToolbarButtonProps = {
  active?: boolean;
  disabled: boolean;
  icon: LucideIcon;
  label: string;
  onAction: () => unknown;
};

const compactToolbarTargetClass = [
  "min-h-11",
  "min-w-11",
  "shrink-0",
  "inline-flex",
  "items-center",
  "justify-center",
  "rounded-lg",
  "border-0",
  "bg-transparent",
  "text-(--text-secondary)",
  "outline-none",
  "focus-visible:outline-2",
  "focus-visible:outline-offset-[-2px]",
  "focus-visible:outline-(--accent)",
  "active:bg-(--bg-active)",
  "active:text-(--text-heading)",
  "aria-pressed:bg-(--bg-active)",
  "aria-pressed:text-(--text-heading)",
  "disabled:opacity-40"
].join(" ");

function ToolbarButton({ active, disabled, icon: Icon, label, onAction }: ToolbarButtonProps) {
  const handlePointerDown = (event: ReactPointerEvent<HTMLButtonElement>) => {
    event.preventDefault();
  };

  return (
    <button
      aria-disabled={disabled}
      aria-label={label}
      aria-pressed={active}
      className={compactToolbarTargetClass}
      disabled={disabled}
      title={label}
      type="button"
      onClick={onAction}
      onPointerDown={handlePointerDown}
    >
      <Icon aria-hidden="true" size={19} />
    </button>
  );
}

function blurActiveEditable() {
  const activeElement = document.activeElement;
  if (!(activeElement instanceof HTMLElement)) return;
  if (!activeElement.matches("input, textarea, [contenteditable='true']")) return;

  activeElement.blur();
}

export function CompactEditorToolbar({
  disabled = false,
  editor,
  imageImport = false,
  language,
  trueMobile = false
}: CompactEditorToolbarProps) {
  const [, setFormattingRevision] = useState(0);
  const formattingState = editor.getSelectionFormattingState();
  const activeActions = new Set(formattingState.actions);
  const refreshFormattingState = useCallback(() => {
    setFormattingRevision((revision) => revision + 1);
  }, []);
  const runAction = useCallback((action: () => unknown) => {
    if (disabled) return;

    action();
    refreshFormattingState();
  }, [disabled, refreshFormattingState]);

  useEffect(() => {
    document.addEventListener("selectionchange", refreshFormattingState);
    return () => document.removeEventListener("selectionchange", refreshFormattingState);
  }, [refreshFormattingState]);

  const style: CSSProperties = {
    bottom: "var(--compact-bottom-inset)"
  };

  return (
    <div
      aria-label={t(language, "compact.toolbar.label")}
      className="absolute inset-x-0 z-30 flex overflow-x-auto overscroll-x-contain border-t border-(--border-subtle) bg-(--bg-primary)/95 px-1 py-1 shadow-lg backdrop-blur-sm"
      data-compact-scroll="horizontal"
      role="toolbar"
      style={style}
    >
      <ToolbarButton
        disabled={disabled}
        icon={Undo2}
        label={t(language, "compact.toolbar.undo")}
        onAction={() => runAction(() => editor.runEditorShortcut("z"))}
      />
      <ToolbarButton
        disabled={disabled}
        icon={Redo2}
        label={t(language, "compact.toolbar.redo")}
        onAction={() => runAction(() => editor.runEditorShortcut("z", { shiftKey: true }))}
      />
      <ToolbarButton
        active={formattingState.headingLevel === null}
        disabled={disabled}
        icon={Pilcrow}
        label={t(language, "compact.toolbar.paragraph")}
        onAction={() => runAction(() => editor.runFormattingAction("paragraph"))}
      />
      <ToolbarButton
        active={formattingState.headingLevel === 1}
        disabled={disabled}
        icon={Heading1}
        label={t(language, "compact.toolbar.heading1")}
        onAction={() => runAction(() => editor.setSelectionHeadingLevel(1))}
      />
      <ToolbarButton
        active={formattingState.headingLevel === 2}
        disabled={disabled}
        icon={Heading2}
        label={t(language, "compact.toolbar.heading2")}
        onAction={() => runAction(() => editor.setSelectionHeadingLevel(2))}
      />
      <ToolbarButton
        active={formattingState.headingLevel === 3}
        disabled={disabled}
        icon={Heading3}
        label={t(language, "compact.toolbar.heading3")}
        onAction={() => runAction(() => editor.setSelectionHeadingLevel(3))}
      />
      <ToolbarButton
        active={activeActions.has("bold")}
        disabled={disabled}
        icon={Bold}
        label={t(language, "compact.toolbar.bold")}
        onAction={() => runAction(() => editor.runFormattingAction("bold"))}
      />
      <ToolbarButton
        active={activeActions.has("italic")}
        disabled={disabled}
        icon={Italic}
        label={t(language, "compact.toolbar.italic")}
        onAction={() => runAction(() => editor.runFormattingAction("italic"))}
      />
      <ToolbarButton
        active={activeActions.has("strikethrough")}
        disabled={disabled}
        icon={Strikethrough}
        label={t(language, "compact.toolbar.strikethrough")}
        onAction={() => runAction(() => editor.runFormattingAction("strikethrough"))}
      />
      <ToolbarButton
        active={activeActions.has("inlineCode")}
        disabled={disabled}
        icon={Code}
        label={t(language, "compact.toolbar.inlineCode")}
        onAction={() => runAction(() => editor.runFormattingAction("inlineCode"))}
      />
      <ToolbarButton
        active={activeActions.has("link")}
        disabled={disabled}
        icon={Link}
        label={t(language, "compact.toolbar.link")}
        onAction={() => runAction(editor.insertMarkdownLink)}
      />
      <ToolbarButton
        active={activeActions.has("bulletList")}
        disabled={disabled}
        icon={List}
        label={t(language, "compact.toolbar.bulletList")}
        onAction={() => runAction(() => editor.runFormattingAction("bulletList"))}
      />
      <ToolbarButton
        active={activeActions.has("orderedList")}
        disabled={disabled}
        icon={ListOrdered}
        label={t(language, "compact.toolbar.orderedList")}
        onAction={() => runAction(() => editor.runFormattingAction("orderedList"))}
      />
      <ToolbarButton
        active={activeActions.has("taskList")}
        disabled={disabled}
        icon={ListChecks}
        label={t(language, "compact.toolbar.taskList")}
        onAction={() => runAction(editor.toggleTaskList)}
      />
      <ToolbarButton
        active={activeActions.has("quote")}
        disabled={disabled}
        icon={Quote}
        label={t(language, "compact.toolbar.quote")}
        onAction={() => runAction(() => editor.runFormattingAction("quote"))}
      />
      <ToolbarButton
        disabled={disabled}
        icon={SquareCode}
        label={t(language, "compact.toolbar.codeBlock")}
        onAction={() => runAction(() => editor.runFormattingAction("codeBlock"))}
      />
      <ToolbarButton
        disabled={disabled || (trueMobile && !imageImport)}
        icon={Image}
        label={t(language, "compact.toolbar.image")}
        onAction={() => runAction(trueMobile ? editor.importLocalImages : editor.insertMarkdownImage)}
      />
      <ToolbarButton
        disabled={disabled}
        icon={KeyboardOff}
        label={t(language, "compact.toolbar.dismissKeyboard")}
        onAction={() => runAction(blurActiveEditable)}
      />
    </div>
  );
}
