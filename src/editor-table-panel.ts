import { keepPanelTargetVisible, showEditorInputPanel } from './editor-viewport';

type TableAction = { text: string; enabled: boolean; run?: () => void };

export function showTablePanel(cell: HTMLTableCellElement, groups: readonly TableAction[][], deleteTable: TableAction, restoreFocus: () => void, onClose: () => void, scroller: HTMLElement) {
  const document = cell.ownerDocument;
  const table = cell.closest('table')!;
  const row = cell.parentElement as HTMLTableRowElement;
  const tinted = new Set([...row.cells, ...[...table.rows].map(row => row.cells[cell.cellIndex]).filter(Boolean)]);
  for (const target of tinted) target.dataset.tablePanelTint = target === cell ? 'cross' : '';

  const dialog = document.createElement('dialog');
  dialog.className = 'table-panel';
  dialog.setAttribute('aria-labelledby', 'editor-table-panel-title');
  dialog.innerHTML = `<header><h2 id="editor-table-panel-title">表格</h2><button type="button" aria-label="关闭表格面板">×</button></header>
    <div class="table-panel-groups"></div>`;
  const events = new AbortController();
  function close(restore = false) {
    if (!dialog.isConnected) return;
    events.abort();
    for (const target of tinted) delete target.dataset.tablePanelTint;
    inputPanel.close();
    layout?.stop();
    onClose();
    if (restore) restoreFocus();
  }
  function button(action: TableAction, html: string) {
    const control = document.createElement('button');
    control.type = 'button';
    control.ariaLabel = action.text;
    control.innerHTML = html;
    control.disabled = !action.enabled;
    control.onclick = () => {
      close(true);
      action.run?.();
    };
    return control;
  }
  for (const [axis, actions] of groups.entries()) {
    const name = axis === 0 ? '行' : '列';
    const group = document.createElement('div');
    group.className = 'table-panel-group';
    const pill = document.createElement('div');
    pill.className = 'table-panel-pill';
    pill.setAttribute('role', 'group');
    pill.setAttribute('aria-label', name);
    for (const [index, action] of actions.entries()) {
      const band = [0, 2, 1][index];
      const rect = axis === 0 ? `x="1" y="${1 + 5 * band}" width="18" height="5"` : `x="${1 + 6 * band}" y="1" width="6" height="15"`;
      const control = button(action, `<svg viewBox="0 0 26 21" aria-hidden="true"><rect ${rect} fill="currentColor"/>
        <rect x="1" y="1" width="18" height="15" rx="2" fill="none" stroke="currentColor" stroke-width="1.5"/>
        <path d="M7 1v15m6-15v15M1 6h18M1 11h18" stroke="currentColor"/>
        <circle cx="20" cy="15" r="5" class="table-panel-glyph-badge"/>
        <path d="M17.5 15h5${index === 2 ? '' : 'M20 12.5v5'}" class="table-panel-glyph-mark" fill="none" stroke-width="1.5" stroke-linecap="round"/></svg>`);
      control.classList.toggle('destructive', index === 2);
      pill.append(control);
    }
    const caption = document.createElement('span');
    caption.textContent = name;
    caption.setAttribute('aria-hidden', 'true');
    group.append(pill, caption);
    dialog.querySelector('.table-panel-groups')!.append(group);
  }
  const remove = button(deleteTable, `<svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 11v5m4-5v5"/></svg>删除表格`);
  remove.className = 'table-panel-delete destructive';
  dialog.append(remove);
  dialog.querySelector('header button')!.addEventListener('click', () => close(true));
  const inputPanel = showEditorInputPanel(dialog, table);
  restoreFocus();
  const workspace = scroller.closest<HTMLElement>('#workspace');
  const layout = workspace && keepPanelTargetVisible(workspace, scroller, dialog, () => cell.getBoundingClientRect());
  document.addEventListener('pointerdown', event => {
    const target = event.target as Element;
    if (!dialog.contains(target) && !cell.contains(target)) close();
  }, { capture: true, signal: events.signal });
  document.addEventListener('keydown', event => {
    if (event.key !== 'Escape' || event.isComposing) return;
    event.preventDefault();
    event.stopPropagation();
    close(true);
  }, { capture: true, signal: events.signal });
  return close;
}
