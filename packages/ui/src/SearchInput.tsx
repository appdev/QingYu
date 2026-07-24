import type { ComponentPropsWithoutRef, ReactNode } from "react";

import { mergeClassNames } from "./classes";

export type SearchInputSize = "md" | "sm";

export type SearchInputProps = Omit<ComponentPropsWithoutRef<"input">, "size" | "type"> & {
  icon?: ReactNode;
  iconClassName?: string;
  size?: SearchInputSize;
  wrapperClassName?: string;
};

const searchInputSizeClassNames: Record<SearchInputSize, string> = {
  md: "h-9 text-[13px]",
  sm: "h-8 text-[12px]"
};

const searchInputIconClassNames: Record<SearchInputSize, string> = {
  md: "left-3",
  sm: "left-2.5"
};

const searchInputPaddingClassNames: Record<SearchInputSize, string> = {
  md: "pr-3 pl-8",
  sm: "pr-2 pl-7"
};

export function SearchInput({
  className,
  icon,
  iconClassName,
  size = "md",
  wrapperClassName,
  ...props
}: SearchInputProps) {
  return (
    <span className={mergeClassNames("relative block", wrapperClassName)}>
      {icon ? (
        <span
          aria-hidden="true"
          className={mergeClassNames(
            "pointer-events-none absolute top-1/2 -translate-y-1/2 text-(--text-secondary)",
            searchInputIconClassNames[size],
            iconClassName
          )}
        >
          {icon}
        </span>
      ) : null}
      <input
        className={mergeClassNames(
          "w-full rounded-md border border-(--border-default) bg-(--bg-primary) leading-5 font-[520] text-(--text-heading) outline-none transition-[border-color,box-shadow] duration-150 ease-out placeholder:text-(--text-secondary) focus:border-(--accent) focus:ring-2 focus:ring-(--accent)/20 focus-visible:outline-none",
          searchInputSizeClassNames[size],
          icon ? searchInputPaddingClassNames[size] : "px-3",
          className
        )}
        type="search"
        {...props}
      />
    </span>
  );
}
