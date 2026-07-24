import type { ComponentPropsWithoutRef, ReactNode } from "react";

import { mergeClassNames } from "./classes";

export type RoundIconButtonSize = "lg" | "md";

export type RoundIconButtonProps = Omit<ComponentPropsWithoutRef<"button">, "aria-label" | "children"> & {
  children: ReactNode;
  label: string;
  size?: RoundIconButtonSize;
};

const roundIconButtonSizeClassNames: Record<RoundIconButtonSize, string> = {
  lg: "size-10",
  md: "size-8"
};

export function RoundIconButton({
  children,
  className,
  label,
  size = "md",
  type = "button",
  ...props
}: RoundIconButtonProps) {
  return (
    <button
      className={mergeClassNames(
        "inline-flex shrink-0 cursor-pointer items-center justify-center rounded-full border-0 bg-(--bg-active) p-0 text-(--text-secondary) transition-colors duration-150 ease-out hover:bg-(--accent) hover:text-(--bg-primary) focus-visible:bg-(--accent) focus-visible:text-(--bg-primary) focus-visible:outline-none disabled:cursor-default disabled:opacity-45 disabled:hover:bg-(--bg-active) disabled:hover:text-(--text-secondary)",
        roundIconButtonSizeClassNames[size],
        className
      )}
      type={type}
      aria-label={label}
      {...props}
    >
      {children}
    </button>
  );
}
