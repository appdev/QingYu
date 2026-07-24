import { cloneElement, isValidElement, useId, type ReactElement, type ReactNode } from "react";

import { mergeClassNames } from "./classes";

type FieldControlProps = {
  "aria-describedby"?: string;
  "aria-invalid"?: boolean;
  "aria-label"?: string;
  id?: string;
};

export type FieldProps = {
  children: ReactElement<FieldControlProps>;
  className?: string;
  description?: ReactNode;
  error?: ReactNode;
  label: ReactNode;
};

export function Field({ children, className, description, error, label }: FieldProps) {
  const generatedId = useId();
  const controlId = children.props.id ?? generatedId;
  const descriptionId = description ? `${controlId}-description` : undefined;
  const errorId = error ? `${controlId}-error` : undefined;
  const describedBy = [descriptionId, errorId].filter(Boolean).join(" ") || undefined;

  const control = isValidElement<FieldControlProps>(children)
    ? cloneElement(children, {
        "aria-describedby": describedBy,
        "aria-invalid": error ? true : children.props["aria-invalid"],
        id: controlId
      })
    : children;

  return (
    <div className={mergeClassNames("grid gap-1.5", className)}>
      <label className="text-[12px] leading-5 font-[650] text-(--text-secondary)" htmlFor={controlId}>
        {label}
      </label>
      {control}
      {description ? (
        <span className="text-[11px] leading-4 text-(--text-secondary)" id={descriptionId}>
          {description}
        </span>
      ) : null}
      {error ? (
        <span className="text-[11px] leading-4 text-(--danger)" id={errorId} role="alert">
          {error}
        </span>
      ) : null}
    </div>
  );
}
