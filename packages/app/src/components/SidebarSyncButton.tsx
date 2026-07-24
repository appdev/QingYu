import { t, type AppLanguage } from "@markra/shared";
import { IconButton } from "@markra/ui";
import { Check, Cloud, CloudOff, LoaderCircle, X } from "lucide-react";

export type SidebarSyncButtonState = "idle" | "unavailable" | "running" | "failed" | "succeeded";

type SidebarSyncButtonProps = {
  className?: string;
  disabled?: boolean;
  language?: AppLanguage;
  muted?: boolean;
  onSync: () => unknown | Promise<unknown>;
  state: SidebarSyncButtonState;
};

export function SidebarSyncButton({
  className,
  disabled = false,
  language = "en",
  muted = false,
  onSync,
  state
}: SidebarSyncButtonProps) {
  const actionLabel = t(language, "settings.sync.run");
  const stateLabel = state === "running"
    ? t(language, "settings.sync.running")
    : state === "unavailable"
      ? `${actionLabel} · ${t(language, "settings.sync.readiness.disabled")}`
      : state === "failed"
        ? `${actionLabel} · ${t(language, "settings.sync.status.failed")}`
        : state === "succeeded"
          ? `${actionLabel} · ${t(language, "settings.sync.status.succeeded")}`
          : actionLabel;
  const interactionDisabled = disabled || state === "running";
  const opacityClassName = muted ? "opacity-40" : "opacity-70";

  return (
    <IconButton
      className={`relative rounded-md ${opacityClassName} hover:opacity-100 focus-visible:opacity-100 active:translate-y-px motion-reduce:transform-none ${className ?? ""}`}
      data-sync-state={state}
      disabled={interactionDisabled}
      aria-busy={state === "running" ? true : undefined}
      label={stateLabel}
      tooltip={stateLabel}
      onClick={onSync}
    >
      {state === "running" ? (
        <LoaderCircle aria-hidden="true" className="animate-spin motion-reduce:animate-none" size={15} />
      ) : state === "unavailable" ? (
        <CloudOff aria-hidden="true" size={15} />
      ) : (
        <Cloud aria-hidden="true" size={15} />
      )}
      {state === "failed" ? (
        <X
          aria-hidden="true"
          className="absolute right-0.5 bottom-0.5 text-(--danger)"
          size={10}
          strokeWidth={3}
        />
      ) : null}
      {state === "succeeded" ? (
        <Check
          aria-hidden="true"
          className="absolute right-0.5 bottom-0.5 text-(--accent)"
          size={10}
          strokeWidth={3}
        />
      ) : null}
    </IconButton>
  );
}
