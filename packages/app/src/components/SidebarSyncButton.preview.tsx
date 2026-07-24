/* Hallmark · pre-emit critique: P5 H5 E5 S5 R5 V4 */
import { SidebarSyncButton, type SidebarSyncButtonState } from "./SidebarSyncButton";

type PreviewRow = {
  className?: string;
  disabled?: boolean;
  label: string;
  state: SidebarSyncButtonState;
};

const previewRows: PreviewRow[] = [
  { label: "default", state: "idle" },
  {
    className: "bg-(--bg-hover) text-(--text-heading) opacity-100",
    label: "forced hover",
    state: "idle"
  },
  {
    className: "opacity-100 ring-2 ring-(--accent)",
    label: "forced focus",
    state: "idle"
  },
  {
    className: "translate-y-px bg-(--bg-hover) text-(--text-heading) opacity-100 motion-reduce:transform-none",
    label: "forced active",
    state: "idle"
  },
  { disabled: true, label: "disabled", state: "idle" },
  { label: "loading", state: "running" },
  { label: "error", state: "failed" },
  { label: "success", state: "succeeded" }
];

export function SidebarSyncButtonPreview() {
  return (
    <section
      aria-label="Sidebar sync button states"
      className="grid w-fit grid-cols-[7rem_auto] items-center gap-x-4 gap-y-3 rounded-lg border border-(--border-default) bg-(--bg-primary) p-4 text-(--text-heading)"
    >
      {previewRows.map((row) => (
        <div className="contents" key={row.label}>
          <span className="text-[11px] leading-4 font-[560] text-(--text-secondary)">
            {row.label}
          </span>
          <SidebarSyncButton
            className={row.className}
            disabled={row.disabled}
            language="en"
            onSync={() => undefined}
            state={row.state}
          />
        </div>
      ))}
    </section>
  );
}
