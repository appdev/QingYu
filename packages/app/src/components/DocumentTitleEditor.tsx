import { useLayoutEffect, useRef, type CompositionEvent, type FormEvent, type KeyboardEvent } from "react";
import { t, type AppLanguage } from "@markra/shared";

export type DocumentTitleEditorProps = {
  disabled?: boolean;
  language: AppLanguage;
  onCommit: (reason: "blur" | "enter") => unknown;
  onInput: (title: string) => unknown;
  title: string;
};

function normalizedTitle(element: HTMLElement) {
  return (element.innerText ?? element.textContent ?? "").replace(/[\r\n]+/g, " ");
}

export function DocumentTitleEditor({
  disabled = false,
  language,
  onCommit,
  onInput,
  title
}: DocumentTitleEditorProps) {
  const editorRef = useRef<HTMLDivElement>(null);
  const composingRef = useRef(false);
  const skipEnterBlurCommitRef = useRef(false);

  useLayoutEffect(() => {
    const editor = editorRef.current;

    if (!editor || document.activeElement === editor || editor.textContent === title) return;

    editor.textContent = title;
  }, [title]);

  const publishInput = (element: HTMLElement) => {
    onInput(normalizedTitle(element));
  };

  const handleInput = (event: FormEvent<HTMLDivElement>) => {
    if (disabled || composingRef.current) return;

    publishInput(event.currentTarget);
  };

  const handleCompositionStart = () => {
    composingRef.current = true;
  };

  const handleCompositionEnd = (event: CompositionEvent<HTMLDivElement>) => {
    composingRef.current = false;
    if (!disabled) publishInput(event.currentTarget);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Enter") return;

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
