import { createElement, type ReactNode } from "react";
import { toast, type ExternalToast } from "sonner";
import {
  SyncErrorToast,
  syncErrorToastClassNames,
  syncErrorToastHostClassName,
  type SyncErrorToastAction
} from "../components/SyncErrorToast";

export type AppToastStatus = "error" | "loading" | "success";
export type AppToastSurface = "notice" | "toast";
export type AppToastAction = ExternalToast["action"];
export type AppToastPresentation = "default" | "sync-error";

export const defaultAppToastId = "app-toast";
export const appNoticeToasterId = "app-notice-toaster";
const maxAutoDismissAppToastDuration = 2000;
let syncErrorToastLifecycleKey = 0;

function resolveAppToastDuration(status: AppToastStatus, duration?: ExternalToast["duration"]) {
  if (status === "loading" || duration === Infinity) return Infinity;
  if (typeof duration === "number") return Math.min(duration, maxAutoDismissAppToastDuration);
  return maxAutoDismissAppToastDuration;
}

function isSyncErrorToastAction(action: AppToastAction): action is SyncErrorToastAction {
  return Boolean(
    action &&
    typeof action === "object" &&
    "label" in action &&
    "onClick" in action &&
    typeof action.onClick === "function"
  );
}

export function showAppToast({
  action,
  description,
  duration,
  id = defaultAppToastId,
  message,
  presentation = "default",
  status,
  surface = "toast"
}: {
  action?: AppToastAction;
  description?: ExternalToast["description"];
  duration?: ExternalToast["duration"];
  id?: string;
  message: ReactNode;
  presentation?: AppToastPresentation;
  status: AppToastStatus;
  surface?: AppToastSurface;
}) {
  const syncErrorPresentation = presentation === "sync-error";
  const resolvedDuration = resolveAppToastDuration(status, duration);
  const surfaceOptions = surface === "notice"
    ? {
        position: "bottom-right" as const,
        toasterId: appNoticeToasterId
      }
    : {};
  if (syncErrorPresentation) {
    const lifecycleKey = ++syncErrorToastLifecycleKey;
    const syncAction = isSyncErrorToastAction(action) ? action : undefined;
    toast.custom((toastId) => createElement(SyncErrorToast, {
      ...(syncAction ? { action: syncAction } : {}),
      duration: resolvedDuration,
      lifecycleKey,
      message,
      status: status === "loading" ? "loading" : "error",
      toastId
    }), {
      className: syncErrorToastHostClassName,
      classNames: syncErrorToastClassNames,
      closeButton: false,
      dismissible: false,
      duration: Infinity,
      id,
      ...surfaceOptions
    });
    return;
  }

  const options: ExternalToast = {
    ...(action ? { action } : {}),
    ...(description ? { description } : {}),
    duration: resolvedDuration,
    id,
    ...surfaceOptions
  };

  if (status === "error") {
    toast.error(message, options);
    return;
  }

  if (status === "loading") {
    toast.loading(message, options);
    return;
  }

  toast.success(message, options);
}

export function dismissAppToast(id = defaultAppToastId) {
  toast.dismiss(id);
}
