import { useId, type ReactNode } from "react";

import { mergeClassNames } from "./classes";
import { IconButton } from "./IconButton";

export type ModalProps = {
  bodyClassName?: string;
  children: ReactNode;
  className?: string;
  closeIcon?: ReactNode;
  closeLabel?: string;
  footer?: ReactNode;
  footerClassName?: string;
  headerClassName?: string;
  onClose?: () => unknown;
  title: ReactNode;
  titleId?: string;
};

export function Modal({
  bodyClassName,
  children,
  className,
  closeIcon,
  closeLabel,
  footer,
  footerClassName,
  headerClassName,
  onClose,
  title,
  titleId
}: ModalProps) {
  const generatedTitleId = useId();
  const dialogTitleId = titleId ?? generatedTitleId;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-[rgba(31,35,44,0.42)] px-4 py-6">
      <div
        aria-labelledby={dialogTitleId}
        aria-modal="true"
        className={mergeClassNames(
          "grid max-h-[min(640px,calc(100vh-48px))] w-full max-w-2xl grid-rows-[auto_minmax(0,1fr)_auto] overflow-hidden rounded-lg border border-(--border-default) bg-(--bg-primary) shadow-xl",
          className
        )}
        role="dialog"
      >
        <div
          className={mergeClassNames(
            "flex items-center justify-between gap-3 border-b border-(--border-default) px-4 py-3",
            headerClassName
          )}
        >
          <h4 className="m-0 text-[13px] leading-5 font-bold text-(--text-heading)" id={dialogTitleId}>
            {title}
          </h4>
          {onClose && closeLabel ? (
            <IconButton label={closeLabel} size="icon-md" onClick={onClose}>
              {closeIcon ?? <span aria-hidden="true">×</span>}
            </IconButton>
          ) : null}
        </div>
        <div className={mergeClassNames("grid min-h-0 gap-2 overflow-auto px-4 py-4", bodyClassName)}>
          {children}
        </div>
        {footer ? (
          <div className={mergeClassNames("flex justify-end gap-2 border-t border-(--border-default) px-4 py-3", footerClassName)}>
            {footer}
          </div>
        ) : null}
      </div>
    </div>
  );
}
