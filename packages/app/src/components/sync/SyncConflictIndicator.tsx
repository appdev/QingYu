import { AlertTriangle } from "lucide-react";

export function SyncConflictIndicator({
  label,
  onOpen
}: {
  label: string;
  onOpen: () => unknown;
}) {
  return (
    <button
      className="absolute bottom-3 left-4.5 z-20 inline-flex min-h-7 items-center gap-1.5 rounded-md border border-(--danger)/35 bg-(--bg-primary)/95 px-2 text-[12px] font-[650] text-(--danger) shadow-sm backdrop-blur hover:bg-(--bg-hover) focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-(--accent)"
      type="button"
      onClick={onOpen}
    >
      <AlertTriangle aria-hidden="true" size={14} strokeWidth={1.8} />
      {label}
    </button>
  );
}
