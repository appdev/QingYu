import type { ComponentPropsWithoutRef } from "react";

import { mergeClassNames } from "./classes";

export type TextareaProps = ComponentPropsWithoutRef<"textarea">;

export function Textarea({ className, ...props }: TextareaProps) {
  return (
    <textarea
      className={mergeClassNames(
        "min-h-23 w-full resize-y rounded-md border border-(--border-default) bg-(--bg-primary) px-3 py-2 text-[12px] leading-5 font-[520] text-(--text-heading) outline-none transition-[border-color,box-shadow] duration-150 ease-out placeholder:text-(--text-secondary) focus:border-(--accent) focus:ring-2 focus:ring-(--accent)/20 focus-visible:outline-none",
        className
      )}
      {...props}
    />
  );
}
