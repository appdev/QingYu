import { useEffect, useState, type MouseEvent, type ReactNode } from "react";
import { CircleAlert, LoaderCircle } from "lucide-react";
import { toast } from "sonner";

export const syncErrorToastHostClassName =
  "app-toast-sync-error min-h-12! w-[min(20rem,calc(100vw-1.5rem))]! max-w-[min(20rem,calc(100vw-1.5rem))]! items-stretch! gap-0! border-0! bg-transparent! p-0! text-[13px]! shadow-none!";
export const syncErrorToastClassNames = {
  content: "contents!",
  title: "contents!"
};

export type SyncErrorToastAction = {
  label: ReactNode;
  onClick: (event: MouseEvent<HTMLButtonElement>) => unknown;
};

export function SyncErrorToast({
  action,
  duration,
  lifecycleKey,
  message,
  status,
  toastId
}: {
  action?: SyncErrorToastAction;
  duration: number;
  lifecycleKey: number;
  message: ReactNode;
  status: "error" | "loading";
  toastId: number | string;
}) {
  const [focusWithin, setFocusWithin] = useState(false);
  const [pointerWithin, setPointerWithin] = useState(false);
  const loading = status === "loading";

  useEffect(() => {
    if (loading || focusWithin || pointerWithin) return;
    const timeout = window.setTimeout(() => toast.dismiss(toastId), duration);
    return () => window.clearTimeout(timeout);
  }, [duration, focusWithin, lifecycleKey, loading, pointerWithin, toastId]);

  const loadingLabel = typeof message === "string" ? message : undefined;

  return (
    <div
      aria-atomic="true"
      aria-busy={loading}
      aria-live="polite"
      className="flex min-h-12 w-full items-center gap-2 rounded-md border border-(--border-default) bg-(--bg-primary) py-0.5 pr-2 pl-3 text-(--text-heading) shadow-[0_10px_28px_rgba(15,23,42,0.12)] transition-opacity duration-150 motion-reduce:transition-opacity"
      onBlurCapture={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setFocusWithin(false);
      }}
      onFocusCapture={() => setFocusWithin(true)}
      onPointerEnter={() => setPointerWithin(true)}
      onPointerLeave={() => setPointerWithin(false)}
      role="status"
    >
      <CircleAlert aria-hidden="true" className="size-4 shrink-0 text-(--danger)" strokeWidth={2} />
      <span className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap font-[620]">
        {message}
      </span>
      {action ? (
        <button
          aria-disabled={loading}
          aria-label={loading ? loadingLabel : undefined}
          className="inline-flex min-h-11 shrink-0 cursor-pointer items-center justify-center rounded-sm border-0 bg-transparent px-2 py-0 text-[12px] leading-5 font-[650] text-(--text-heading) underline decoration-(--border-strong) underline-offset-4 transition-colors duration-150 hover:bg-(--bg-hover) focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent) aria-disabled:cursor-default aria-disabled:no-underline motion-reduce:transition-none"
          data-action="true"
          onClick={(event) => {
            if (loading) {
              event.preventDefault();
              return;
            }
            action.onClick(event);
          }}
          type="button"
        >
          {loading ? (
            <>
              <LoaderCircle aria-hidden="true" className="size-3.5 animate-spin motion-reduce:animate-none" />
              <span className="sr-only">{message}</span>
            </>
          ) : action.label}
        </button>
      ) : null}
    </div>
  );
}
