import { fitSidebarSizes, restoreSidebarSizes, sidebarLimits, type SidebarLayout, type SidebarSizes } from './sidebar-layout';

export function setupSidebarResize(shell: HTMLElement, getLayout: () => SidebarLayout, applyLayout: (layout: SidebarLayout) => void, error: (message: string) => void, external = false,
  getPreferredColumns = () => getLayout().columns) {
  const key = external ? 'miaoyan-editor.sidebar-size.external.v1' : 'miaoyan-editor.sidebar-size.v1';
  let stored = '';
  let saved: ReturnType<typeof restoreSidebarSizes>;
  try { stored = localStorage.getItem(key) ?? ''; saved = restoreSidebarSizes(JSON.parse(stored || 'null')); }
  catch { saved = restoreSidebarSizes(null); }
  let sizes: SidebarSizes = { folders: saved.folders, notes: saved.notes };
  const handles = { folders: shell.querySelector<HTMLElement>('#folder-resizer')!, notes: shell.querySelector<HTMLElement>('#notes-resizer')! };
  let drag: { id: number; kind: keyof SidebarSizes; x: number; width: number; sizes: SidebarSizes; visibleFolders: number; collapsed: boolean } | undefined;

  function update() {
    if (innerWidth <= 900) { if (drag) finish(true); return; }
    const { columns } = getLayout();
    const visible = fitSidebarSizes(sizes, columns, innerWidth);
    const pixel = (width: number) => Math.round(width * devicePixelRatio) / devicePixelRatio;
    visible.folders = pixel(visible.folders);
    visible.notes = Math.max(0, Math.min(pixel(visible.notes), innerWidth - sidebarLimits.editorMin - visible.folders));
    shell.style.setProperty('--folder-width', `${visible.folders}px`);
    shell.style.setProperty('--notes-width', `${visible.notes}px`);
    for (const kind of ['folders', 'notes'] as const) {
      handles[kind].setAttribute('aria-valuenow', String(visible[kind]));
      handles[kind].setAttribute('aria-valuemax', String(kind === 'folders' ? sidebarLimits.folderMax : sidebarLimits.noteMax));
    }
    if (drag) return;
    const next = JSON.stringify({ version: 2, ...sizes, columns: getPreferredColumns() });
    if (stored === next) return;
    try { localStorage.setItem(key, next); stored = next; }
    catch { error('分栏宽度未能保存'); }
  }

  function move(kind: keyof SidebarSizes, width: number) {
    if (kind === 'folders' && width <= sidebarLimits.folderSnap) {
      if (drag) { sizes = { ...drag.sizes }; drag.collapsed = true; }
      applyLayout({ columns: 'list', unfolding: false });
      return;
    }
    const { folderMin, folderMax, noteMin, noteMax } = sidebarLimits;
    if (kind === 'notes' && drag?.visibleFolders) {
      // Once the list reaches its minimum, the same divider can narrow the folder column.
      sizes.folders = width < noteMin ? Math.max(folderMin, drag.visibleFolders + width - noteMin) : drag.sizes.folders;
    }
    sizes[kind] = kind === 'folders' ? Math.max(folderMin, Math.min(width, folderMax)) : Math.max(noteMin, Math.min(width, noteMax));
    update();
  }

  function finish(cancel = false) {
    if (!drag) return;
    const previous = drag;
    drag = undefined;
    delete document.body.dataset.libraryResizing;
    if (shell.hasPointerCapture(previous.id)) shell.releasePointerCapture(previous.id);
    if (cancel) {
      sizes = previous.sizes;
      if (previous.collapsed) { applyLayout({ columns: 'all', unfolding: false }); return; }
    }
    update();
  }

  for (const kind of ['folders', 'notes'] as const) {
    const handle = handles[kind];
    handle.onpointerdown = event => {
      if (event.button !== 0 || innerWidth <= 900 || drag) return;
      event.preventDefault();
      drag = { id: event.pointerId, kind, x: event.clientX, width: handle.parentElement!.getBoundingClientRect().width, sizes: { ...sizes },
        visibleFolders: Number(handles.folders.getAttribute('aria-valuenow')), collapsed: false };
      document.body.dataset.libraryResizing = kind;
      // Capture on the shell: the dragged column and its handle may collapse.
      shell.setPointerCapture(event.pointerId);
    };
    handle.onkeydown = event => {
      if (innerWidth <= 900 || !['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
      event.preventDefault();
      const width = Number(handle.getAttribute('aria-valuenow'));
      move(kind, event.key === 'Home' ? 0 : event.key === 'End' ? Infinity : width + (event.key === 'ArrowLeft' ? -10 : 10));
    };
  }
  shell.addEventListener('pointermove', event => {
    // A hidden divider stays collapsed for this gesture; panel actions restore it.
    if (drag && !drag.collapsed && event.pointerId === drag.id) move(drag.kind, drag.width + event.clientX - drag.x);
  });
  shell.addEventListener('pointerup', event => { if (drag?.id === event.pointerId) finish(); });
  shell.addEventListener('pointercancel', event => { if (drag?.id === event.pointerId) finish(true); });
  shell.addEventListener('lostpointercapture', event => { if (drag?.id === event.pointerId) finish(true); });
  window.addEventListener('blur', () => finish(true));
  document.addEventListener('keydown', event => { if (drag && event.key === 'Escape') { event.preventDefault(); finish(true); } });
  return { columns: external ? 'note' as const : saved.columns, update };
}
