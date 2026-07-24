import { useEffect, useId, useRef, useState, type FormEvent, type KeyboardEvent } from "react";

type CompactNameDialogProps = {
  cancelLabel: string;
  errorMessage: string | ((error: unknown) => string);
  initialValue: string;
  onCancel: () => unknown;
  onSubmit: (value: string) => Promise<unknown>;
  submitLabel: string;
  title: string;
};

const targetClass = "min-h-11 min-w-11";

type CompactNameOperationErrorMessages = {
  duplicate: string;
  fallback: string;
  invalid: string;
};

export function compactNameOperationErrorMessage(
  error: unknown,
  messages: CompactNameOperationErrorMessages
) {
  const detail = error instanceof Error
    ? error.message
    : typeof error === "string"
      ? error
      : "";

  if (/\b(?:already exists|destination exists|file exists|folder exists)\b/iu.test(detail)) {
    return messages.duplicate;
  }
  if (/\b(?:invalid|required|cannot include|must use|not allowed|reserved)\b/iu.test(detail)) {
    return messages.invalid;
  }
  return messages.fallback;
}

export function CompactNameDialog({
  cancelLabel,
  errorMessage,
  initialValue,
  onCancel,
  onSubmit,
  submitLabel,
  title
}: CompactNameDialogProps) {
  const titleId = useId();
  const dialogRef = useRef<HTMLFormElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const [value, setValue] = useState(initialValue);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const normalizedValue = value.trim();

  useEffect(() => {
    previousFocusRef.current = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    inputRef.current?.focus();
    inputRef.current?.select();

    return () => {
      previousFocusRef.current?.focus();
    };
  }, []);

  const submit = async () => {
    if (!normalizedValue || pending) return;
    setError(null);
    setPending(true);
    try {
      await onSubmit(normalizedValue);
    } catch (operationError) {
      setError(typeof errorMessage === "function"
        ? errorMessage(operationError)
        : errorMessage);
      setPending(false);
    }
  };

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    submit().catch(() => {});
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLFormElement>) => {
    if (event.key === "Escape") {
      if (pending) return;
      event.preventDefault();
      onCancel();
      return;
    }

    if (event.key === "Tab") {
      const dialog = dialogRef.current;
      if (!dialog) return;
      const focusableElements = Array.from(dialog.querySelectorAll<HTMLElement>(
        "input:not([disabled]), button:not([disabled]), [href], [tabindex]:not([tabindex='-1'])"
      ));
      const firstElement = focusableElements[0];
      const lastElement = focusableElements.at(-1);
      if (!firstElement || !lastElement) return;

      if (event.shiftKey && document.activeElement === firstElement) {
        event.preventDefault();
        lastElement.focus();
      } else if (!event.shiftKey && document.activeElement === lastElement) {
        event.preventDefault();
        firstElement.focus();
      } else if (!dialog.contains(document.activeElement)) {
        event.preventDefault();
        firstElement.focus();
      }
      return;
    }

    if (event.key === "Enter" && event.target === inputRef.current) {
      event.preventDefault();
      submit().catch(() => {});
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 px-4 pt-[var(--compact-safe-area-top)] pb-[var(--compact-safe-area-bottom)]">
      <form
        ref={dialogRef}
        aria-labelledby={titleId}
        aria-modal="true"
        aria-busy={pending}
        className="grid w-full max-w-sm gap-4 rounded-xl border border-(--border-default) bg-(--bg-primary) p-4 shadow-xl"
        role="dialog"
        onKeyDown={handleKeyDown}
        onSubmit={handleSubmit}
      >
        <h2 className="m-0 text-base font-semibold text-(--text-heading)" id={titleId}>
          {title}
        </h2>
        <input
          ref={inputRef}
          aria-labelledby={titleId}
          className="min-h-11 w-full rounded-lg border border-(--border-default) bg-(--bg-secondary) px-3 text-base"
          readOnly={pending}
          type="text"
          value={value}
          onChange={(event) => setValue(event.currentTarget.value)}
        />
        {error ? <p className="m-0 text-sm text-(--status-error)" role="alert">{error}</p> : null}
        <div className="grid grid-cols-2 gap-2">
          <button
            className={`${targetClass} rounded-lg border border-(--border-default) px-4 text-sm font-medium`}
            disabled={pending}
            type="button"
            onClick={onCancel}
          >
            {cancelLabel}
          </button>
          <button
            className={`${targetClass} rounded-lg bg-(--accent) px-4 text-sm font-semibold text-white disabled:opacity-50`}
            disabled={!normalizedValue || pending}
            type="submit"
          >
            {submitLabel}
          </button>
        </div>
      </form>
    </div>
  );
}
