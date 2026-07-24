import type { ComponentPropsWithoutRef, MouseEvent } from "react";

import { mergeClassNames } from "./classes";

export type SwitchProps = Omit<ComponentPropsWithoutRef<"button">, "aria-checked" | "aria-label" | "role" | "type"> & {
  checked: boolean;
  label: string;
  onCheckedChange?: (checked: boolean) => unknown;
};

export function Switch({
  checked,
  className,
  disabled,
  label,
  onCheckedChange,
  onClick,
  ...props
}: SwitchProps) {
  const handleClick = (event: MouseEvent<HTMLButtonElement>) => {
    onClick?.(event);
    if (event.defaultPrevented || disabled) return;
    onCheckedChange?.(!checked);
  };

  return (
    <button
      className={mergeClassNames(
        "relative inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full border border-(--border-strong) bg-(--bg-secondary) transition-colors duration-150 ease-out aria-checked:border-(--accent) aria-checked:bg-(--accent) disabled:cursor-default disabled:opacity-60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent)",
        className
      )}
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={handleClick}
      {...props}
    >
      <span
        className="inline-block size-3.5 translate-x-0.75 rounded-full bg-(--text-secondary) transition-transform duration-150 ease-out aria-checked:translate-x-4.5 aria-checked:bg-(--bg-primary)"
        aria-checked={checked}
      />
    </button>
  );
}
