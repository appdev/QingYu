import { useLayoutEffect, useRef, type CompositionEvent, type FormEvent, type KeyboardEvent } from "react";
import { t, type AppLanguage } from "@markra/shared";

export type DocumentTitleEditorProps = {
  disabled?: boolean;
  language: AppLanguage;
  onCommit: (reason: "blur" | "enter") => unknown;
  onInput: (title: string) => unknown;
  resetToken: number;
  title: string;
};

function normalizedTitle(element: HTMLElement) {
  return (element.innerText ?? element.textContent ?? "").replace(/[\r\n]+/g, " ");
}

function normalizeEditableTitle(element: HTMLElement) {
  const title = normalizedTitle(element);

  if (element.textContent === title) return title;

  element.textContent = title;
  if (document.activeElement !== element) return title;

  const selection = window.getSelection();
  if (!selection) return title;

  const range = document.createRange();
  const textNode = element.firstChild;
  if (textNode) {
    range.setStart(textNode, title.length);
    range.collapse(true);
  } else {
    range.selectNodeContents(element);
    range.collapse(false);
  }
  selection.removeAllRanges();
  selection.addRange(range);

  return title;
}

export function DocumentTitleEditor({
  disabled = false,
  language,
  onCommit,
  onInput,
  resetToken,
  title
}: DocumentTitleEditorProps) {
  const editorRef = useRef<HTMLDivElement>(null);
  const composingRef = useRef(false);
  const compositionEndTitleRef = useRef<string | null>(null);
  const previousResetTokenRef = useRef(resetToken);
  const skipEnterBlurCommitRef = useRef(false);

  useLayoutEffect(() => {
    const editor = editorRef.current;
    const resetRequested = previousResetTokenRef.current !== resetToken;
    previousResetTokenRef.current = resetToken;

    if (!editor || (!resetRequested && document.activeElement === editor) || editor.textContent === title) return;

    editor.textContent = title;
  }, [resetToken, title]);

  const publishInput = (element: HTMLElement, fromCompositionEnd = false) => {
    const nextTitle = normalizeEditableTitle(element);

    if (!fromCompositionEnd && compositionEndTitleRef.current !== null) {
      const isDuplicate = compositionEndTitleRef.current === nextTitle;
      compositionEndTitleRef.current = null;
      if (isDuplicate) return;
    }

    onInput(nextTitle);
    if (fromCompositionEnd) compositionEndTitleRef.current = nextTitle;
  };

  const handleInput = (event: FormEvent<HTMLDivElement>) => {
    if (disabled || composingRef.current) return;

    publishInput(event.currentTarget);
  };

  const handleCompositionStart = () => {
    composingRef.current = true;
    compositionEndTitleRef.current = null;
  };

  const handleCompositionEnd = (event: CompositionEvent<HTMLDivElement>) => {
    composingRef.current = false;
    if (!disabled) publishInput(event.currentTarget, true);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Enter" || composingRef.current || event.nativeEvent.isComposing) return;

    event.preventDefault();
    skipEnterBlurCommitRef.current = true;
    onCommit("enter");
    queueMicrotask(() => {
      skipEnterBlurCommitRef.current = false;
    });
  };

  return (
    <div
      ref={editorRef}
      role="textbox"
      aria-label={t(language, "app.documentTitle")}
      aria-multiline="false"
      contentEditable={!disabled}
      suppressContentEditableWarning
      className="mb-6 w-full whitespace-pre-wrap break-words text-4xl leading-tight font-semibold text-(--text-primary)"
      onBlur={() => {
        compositionEndTitleRef.current = null;
        if (skipEnterBlurCommitRef.current) {
          skipEnterBlurCommitRef.current = false;
          return;
        }

        onCommit("blur");
      }}
      onCompositionEnd={handleCompositionEnd}
      onCompositionStart={handleCompositionStart}
      onInput={handleInput}
      onKeyDown={handleKeyDown}
    />
  );
}
