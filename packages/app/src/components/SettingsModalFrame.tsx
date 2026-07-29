import { X } from "lucide-react";
import {
  useLayoutEffect,
  useRef,
  type KeyboardEvent,
  type ReactNode
} from "react";
import type { DesktopPlatform } from "../lib/platform";

type SettingsModalFrameProps = {
  children: ReactNode;
  closeLabel?: string;
  label: string;
  onClose: () => unknown | Promise<unknown>;
  platform: DesktopPlatform;
};

const focusableSelector = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  '[tabindex]:not([tabindex="-1"])'
].join(",");

function runCloseAction(action: SettingsModalFrameProps["onClose"]) {
  try {
    Promise.resolve(action()).catch(() => {});
  } catch {
    // Closing can race with settings cleanup; keep the modal stable for a retry.
  }
}

function MacosCloseGlyph() {
  return (
    <svg
      aria-hidden="true"
      className="pointer-events-none absolute inset-0 m-auto size-[9px] text-[#2f2f2f] opacity-0 transition-opacity duration-100 group-hover/modal-close:opacity-70 group-focus-visible/modal-close:opacity-70"
      data-icon="macos-close"
      viewBox="0 0 9 9"
    >
      <path
        d="M2.25 2.25L6.75 6.75M6.75 2.25L2.25 6.75"
        fill="none"
        stroke="currentColor"
        strokeLinecap="round"
        strokeWidth="1.9"
      />
    </svg>
  );
}

function SettingsModalCloseControl({
  closeLabel,
  onClose,
  platform
}: Pick<SettingsModalFrameProps, "closeLabel" | "onClose" | "platform">) {
  const commonProps = {
    "aria-label": closeLabel ?? "Close window",
    "data-settings-modal-close": platform,
    onClick: () => runCloseAction(onClose),
    type: "button" as const
  };

  if (platform === "macos") {
    return (
      <div className="absolute top-0 left-0 z-[40] flex h-10 items-center pl-4 pr-2">
        <button
          {...commonProps}
          className="group/modal-close relative inline-flex size-[15px] shrink-0 cursor-default items-center justify-center rounded-full border border-[#e0443e] bg-[#ff5f57] p-0 transition-[box-shadow,filter] duration-150 ease-out hover:brightness-95 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-2 focus-visible:ring-offset-(--bg-primary)"
        >
          <MacosCloseGlyph />
        </button>
      </div>
    );
  }

  if (platform === "windows") {
    return (
      <button
        {...commonProps}
        className="absolute top-0 right-0 z-[40] inline-flex h-10 w-11 shrink-0 cursor-default items-center justify-center border-0 bg-transparent p-0 text-(--text-secondary) transition-[background-color,color] duration-100 ease-out hover:bg-[#c42b1c] hover:text-[#fdfdfd] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-inset"
      >
        <X aria-hidden="true" size={15} strokeWidth={1.8} />
      </button>
    );
  }

  return (
    <button
      {...commonProps}
      className="absolute top-3 right-4 z-[40] inline-flex size-8 shrink-0 cursor-pointer items-center justify-center rounded-md border border-transparent bg-transparent p-0 text-(--text-secondary) transition-colors duration-150 ease-out hover:bg-(--bg-hover) hover:text-(--text-heading) focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent)"
    >
      <X aria-hidden="true" size={16} />
    </button>
  );
}

export function SettingsModalFrame({
  children,
  closeLabel,
  label,
  onClose,
  platform
}: SettingsModalFrameProps) {
  const dialogRef = useRef<HTMLElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);

  useLayoutEffect(() => {
    previousFocusRef.current = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    const dialog = dialogRef.current;
    const close = dialog?.querySelector<HTMLElement>("[data-settings-modal-close]");
    (close ?? dialog)?.focus();

    return () => {
      const previousFocus = previousFocusRef.current;
      if (!previousFocus?.isConnected) return;
      previousFocus.focus();
      if (document.activeElement === previousFocus) return;
      window.requestAnimationFrame(() => {
        if (previousFocus.isConnected) previousFocus.focus();
      });
    };
  }, []);

  const handleKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.defaultPrevented) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      runCloseAction(onClose);
      return;
    }
    if (event.key !== "Tab") return;

    const dialog = dialogRef.current;
    if (!dialog) return;
    const focusable = Array.from(dialog.querySelectorAll<HTMLElement>(focusableSelector))
      .filter((element) => element.getAttribute("aria-hidden") !== "true");
    const first = focusable[0] ?? dialog;
    const last = focusable.at(-1) ?? dialog;
    const active = document.activeElement;

    if (event.shiftKey && (active === first || !dialog.contains(active))) {
      event.preventDefault();
      last.focus();
      return;
    }
    if (!event.shiftKey && (active === last || !dialog.contains(active))) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <div
      className="settings-modal-backdrop fixed inset-0 z-[90] flex items-center justify-center bg-[color-mix(in_srgb,var(--text-heading)_36%,transparent)] p-4"
      data-testid="settings-modal-backdrop"
    >
      <section
        ref={dialogRef}
        aria-label={label}
        aria-modal="true"
        className="settings-modal relative h-[720px] w-[1040px] max-h-[calc(100dvh-32px)] max-w-[calc(100vw-32px)] overflow-hidden rounded-xl border border-(--border-default) bg-(--bg-primary) shadow-2xl outline-none"
        onKeyDown={handleKeyDown}
        role="dialog"
        tabIndex={-1}
      >
        <SettingsModalCloseControl
          closeLabel={closeLabel}
          onClose={onClose}
          platform={platform}
        />
        {children}
      </section>
    </div>
  );
}
