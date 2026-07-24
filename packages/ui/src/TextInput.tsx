import type { ComponentPropsWithoutRef } from "react";

import { mergeClassNames } from "./classes";

export type TextInputProps = ComponentPropsWithoutRef<"input">;

export function TextInput({ className, type = "text", ...props }: TextInputProps) {
  return (
    <input
      className={mergeClassNames(
        "h-9 w-full rounded-md border border-(--border-default) bg-(--bg-primary) px-3 text-[13px] leading-5 font-[520] text-(--text-heading) outline-none transition-[border-color,box-shadow] duration-150 ease-out placeholder:text-(--text-secondary) hover:bg-(--bg-hover) focus:border-(--accent) focus:ring-2 focus:ring-(--accent)/20 focus-visible:outline-none",
        className
      )}
      type={type}
      {...props}
    />
  );
}
