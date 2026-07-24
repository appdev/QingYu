import type { ComponentPropsWithoutRef } from "react";

import { mergeClassNames } from "./classes";

export type ButtonVariant = "ghost" | "primary" | "secondary";
export type ButtonSize = "icon-lg" | "icon-md" | "icon-sm" | "icon-xs" | "md" | "sm";

export type ButtonProps = ComponentPropsWithoutRef<"button"> & {
  size?: ButtonSize;
  variant?: ButtonVariant;
};

const variantClassNames: Record<ButtonVariant, string> = {
  ghost: "border-transparent bg-transparent text-(--text-secondary) hover:bg-(--bg-hover) hover:text-(--text-heading) disabled:hover:bg-transparent disabled:hover:text-(--text-secondary)",
  primary: "border-(--accent) bg-(--accent) text-(--bg-primary) hover:border-(--accent-hover) hover:bg-(--accent-hover) disabled:hover:border-(--accent) disabled:hover:bg-(--accent)",
  secondary: "border-(--border-default) bg-(--bg-primary) text-(--text-heading) hover:bg-(--bg-hover) disabled:hover:bg-(--bg-primary)"
};

const sizeClassNames: Record<ButtonSize, string> = {
  "icon-lg": "size-10 p-0",
  "icon-md": "size-8 p-0",
  "icon-sm": "size-7 p-0",
  "icon-xs": "size-6 p-0",
  md: "h-9 px-3 text-[12px] leading-5 font-[650]",
  sm: "h-8 px-3 text-[12px] leading-5 font-[560]"
};

export function Button({
  children,
  className,
  size = "md",
  type = "button",
  variant = "secondary",
  ...props
}: ButtonProps) {
  return (
    <button
      className={mergeClassNames(
        "inline-flex shrink-0 cursor-pointer items-center justify-center gap-2 rounded-md border transition-colors duration-150 ease-out disabled:cursor-default disabled:opacity-60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent)",
        variantClassNames[variant],
        sizeClassNames[size],
        className
      )}
      type={type}
      {...props}
    >
      {children}
    </button>
  );
}
