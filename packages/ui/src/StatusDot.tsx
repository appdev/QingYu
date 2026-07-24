import type { ComponentPropsWithoutRef } from "react";

import { mergeClassNames } from "./classes";

export type StatusDotSize = "md" | "sm";
export type StatusDotTone = "active" | "inactive" | "neutral";

export type StatusDotProps = ComponentPropsWithoutRef<"span"> & {
  size?: StatusDotSize;
  tone?: StatusDotTone;
};

const statusDotSizeClassNames: Record<StatusDotSize, string> = {
  md: "size-2",
  sm: "size-1.5"
};

const statusDotToneClassNames: Record<StatusDotTone, string> = {
  active: "bg-(--accent)",
  inactive: "bg-(--border-strong)",
  neutral: "bg-(--text-tertiary)"
};

export function StatusDot({
  className,
  size = "md",
  tone = "neutral",
  ...props
}: StatusDotProps) {
  return (
    <span
      className={mergeClassNames(
        "inline-block shrink-0 rounded-full",
        statusDotSizeClassNames[size],
        statusDotToneClassNames[tone],
        className
      )}
      {...props}
    />
  );
}
