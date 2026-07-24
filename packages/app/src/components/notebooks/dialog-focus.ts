import type { KeyboardEvent } from "react";

const focusableSelector = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])"
].join(", ");

export function containDialogTabFocus(
  event: KeyboardEvent<HTMLElement>,
  dialog: HTMLElement | null
) {
  if (event.key !== "Tab" || !dialog) return false;
  const focusableElements = Array.from(dialog.querySelectorAll<HTMLElement>(focusableSelector));
  const firstElement = focusableElements[0];
  const lastElement = focusableElements.at(-1);
  if (!firstElement || !lastElement) {
    event.preventDefault();
    dialog.focus();
    return true;
  }

  const activeElement = document.activeElement;
  const activeFocusable = activeElement instanceof HTMLElement && focusableElements.includes(activeElement);
  const target = event.shiftKey && activeElement === firstElement
    ? lastElement
    : !event.shiftKey && activeElement === lastElement
      ? firstElement
      : !activeFocusable
        ? event.shiftKey ? lastElement : firstElement
        : null;
  if (!target) return false;

  event.preventDefault();
  target.focus();
  return true;
}
