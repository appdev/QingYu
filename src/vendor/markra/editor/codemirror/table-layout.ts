import type { EditorView } from "@codemirror/view";
import { activeTableCell, revealTableCellHorizontally } from "./table-selection.ts";

export function tableColumnWidths(text: readonly number[], available: number, fontSize: number, fill = false) {
  const insets = 2 * Math.max(8, Math.round(fontSize * 0.6));
  const maximum = Math.max(280, 280 * fontSize / 17);
  const natural = text.map(width => Math.min(Math.max(width + insets + 8, 64), maximum));
  const sum = (widths: readonly number[]) => widths.reduce((total, width) => total + width, 0);
  const total = sum(natural);
  if (total < available && (fill || total >= available * 0.7)) return natural.map(width => width * available / total);
  if (total <= available) return natural;

  const tight = text.map((width, index) => Math.min(Math.max(width + insets, 44), natural[index]!));
  const least = sum(tight);
  if (least >= available) return tight;
  return natural.map((width, index) => width - (width - tight[index]!) * (total - available) / (total - least));
}

const measuringCanvases = new WeakMap<Document, CanvasRenderingContext2D>();
export const tableDisplayChanged = "markra-table-display-changed";

export function layoutTableColumns(view: EditorView, wrapper: HTMLElement, scroller: HTMLElement, table: HTMLTableElement, fill = false) {
  const document = wrapper.ownerDocument;
  const window = document.defaultView;
  if (!window?.ResizeObserver) return;
  const columns = document.createElement("colgroup");
  for (const _cell of table.rows[0]?.cells ?? []) columns.append(document.createElement("col"));
  table.prepend(columns);
  let active = true, font = "", longest: number[] = [], previousWidth = 0;

  function overflow() {
    scroller.toggleAttribute("data-overflow-left", scroller.scrollLeft > 1);
    scroller.toggleAttribute("data-overflow-right", scroller.scrollWidth - scroller.clientWidth - scroller.scrollLeft > 1);
  }

  function measure() {
    if (!active || !wrapper.isConnected || !window) return;
    view.requestMeasure({
      key: table,
      read() {
        const line = wrapper.closest<HTMLElement>(".cm-line");
        if (!line) return;
        const lineStyle = window.getComputedStyle(line);
        const padding = window.getComputedStyle(wrapper);
        const available = line.clientWidth - parseFloat(lineStyle.paddingLeft) - parseFloat(lineStyle.paddingRight)
          - parseFloat(padding.paddingLeft) - parseFloat(padding.paddingRight) - 2;
        if (available <= 0) return;
        const style = window.getComputedStyle(table);
        const nextFont = `${style.fontSize} ${style.fontFamily}`;
        if (nextFont !== font) {
          const canvas = measuringCanvases.get(document) ?? document.createElement("canvas").getContext("2d");
          if (!canvas) return;
          measuringCanvases.set(document, canvas);
          canvas.font = nextFont;
          const digits = Array.from({ length: 10 }, (_, digit) => canvas.measureText(String(digit)).width);
          const tabular = Math.max(...digits);
          longest = Array.from(columns.children, () => 0);
          for (const row of table.rows) for (const cell of row.cells) {
            const label = cell.textContent ?? "";
            const digitRoom = [...label.matchAll(/[0-9]/gu)].reduce((extra, [digit]) => extra + tabular - digits[Number(digit)]!, 0);
            const rule = cell.cellIndex < columns.childElementCount - 1 ? 1 : 0;
            const width = Math.ceil(canvas.measureText(label).width + digitRoom) + rule;
            longest[cell.cellIndex] = Math.max(longest[cell.cellIndex] ?? 0, width);
          }
          font = nextFont;
          previousWidth = 0;
        }
        if (available !== previousWidth) {
          previousWidth = available;
          return tableColumnWidths(longest, available, parseFloat(style.fontSize), fill);
        }
      },
      write(widths) {
        if (!active || !widths?.length) return;
        widths.forEach((width, index) => { (columns.children[index] as HTMLElement).style.width = `${width}px`; });
        table.style.width = `${widths.reduce((total, width) => total + width, 0)}px`;
        table.dataset.measuredColumns = "true";
      },
    });
  }

  const size = new window.ResizeObserver(() => {
    const cell = table.contains(document.activeElement) ? activeTableCell(table) : null;
    // A new table widget gets its measured widths after the editing cell has regained focus.
    if (cell) revealTableCellHorizontally(cell);
    overflow();
    measure();
  });
  const fonts = new window.MutationObserver(measure);
  const refreshText = () => { font = ""; measure(); };
  queueMicrotask(() => {
    if (!active || !wrapper.isConnected) return;
    size.observe(wrapper.closest(".cm-line") ?? view.contentDOM);
    size.observe(table);
    fonts.observe(document.documentElement, { attributes: true, attributeFilter: ["style"] });
    measure();
  });
  scroller.addEventListener("scroll", overflow, { passive: true });
  table.addEventListener(tableDisplayChanged, refreshText);
  return () => {
    active = false;
    size.disconnect();
    fonts.disconnect();
    scroller.removeEventListener("scroll", overflow);
    table.removeEventListener(tableDisplayChanged, refreshText);
  };
}
