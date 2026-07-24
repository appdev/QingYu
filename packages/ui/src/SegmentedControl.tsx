import type { ComponentPropsWithoutRef } from "react";

import { mergeClassNames } from "./classes";

export type SegmentedControlProps = Omit<ComponentPropsWithoutRef<"div">, "aria-label" | "role"> & {
  label: string;
};

export type SegmentedControlItemProps = ComponentPropsWithoutRef<"button"> & {
  label: string;
  selected: boolean;
};

export function SegmentedControl({ children, className, label, ...props }: SegmentedControlProps) {
  return (
    <div
      className={mergeClassNames(
        "grid shrink-0 rounded-md border border-(--border-default) bg-(--bg-secondary) p-0.5",
        className
      )}
      role="group"
      aria-label={label}
      {...props}
    >
      {children}
    </div>
  );
}

export function SegmentedControlItem({
  children,
  className,
  label,
  selected,
  type = "button",
  ...props
}: SegmentedControlItemProps) {
  return (
    <button
      className={mergeClassNames(
        "inline-flex h-7 min-w-16 items-center justify-center gap-1.5 rounded-sm border-0 bg-transparent px-2.5 text-[12px] leading-5 font-[560] text-(--text-secondary) transition-colors duration-150 ease-out hover:text-(--text-heading) aria-pressed:bg-(--bg-active) aria-pressed:text-(--text-heading) focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent)",
        className
      )}
      type={type}
      aria-label={label}
      aria-pressed={selected}
      {...props}
    >
      {children}
    </button>
  );
}
