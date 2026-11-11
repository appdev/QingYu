import { fitSidebarSizes, restoreSidebarSizes, sidebarLimits, type SidebarLayout, type SidebarSizes } from './sidebar-layout';

export function setupSidebarResize(shell: HTMLElement, getLayout: () => SidebarLayout, applyLayout: (layout: SidebarLayout) => void, error: (message: string) => void, external = false) {
  const key = external ? 'miaoyan-editor.sidebar-size.external.v1' : 'miaoyan-editor.sidebar-size.v1';
  let stored = '';
  let saved: ReturnType<typeof restoreSidebarSizes>;
  try { stored = localStorage.getItem(key) ?? ''; saved = restoreSidebarSizes(JSON.parse(stored || 'null')); }
  catch { saved = restoreSidebarSizes(null); }
  let sizes: SidebarSizes = { folders: saved.folders, notes: saved.notes };
  let previousColumns = saved.columns;
  // Window scaling changes the displayed width, not the remembered drag width.
  let folderReferenceWidth = innerWidth;
  const handles = { folders: shell.querySelector<HTMLElement>('#folder-resizer')!, notes: shell.querySelector<HTMLElement>('#notes-resizer')! };
  let drag: { id: number; kind: keyof SidebarSizes; x: number; width: number; sizes: SidebarSizes; layout: SidebarLayout; referenceWidth: number } | undefined;

  function update() {
    if (innerWidth <= 900) { if (drag) finish(true); return; }
    const { columns } = getLayout();
    if (columns !== 'all') folderReferenceWidth = innerWidth;
    if (!drag && columns === 'all' && previousColumns !== 'all') {
      sizes.folders = Math.max(sidebarLimits.folderMin, sizes.folders);
      folderReferenceWidth = innerWidth;
    }
    if (!drag && columns !== 'note' && previousColumns === 'note') sizes.notes = Math.max(sidebarLimits.noteMin, sizes.notes);
    previousColumns = columns;
    const visible = fitSidebarSizes(sizes, columns, innerWidth, folderReferenceWidth);
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
    const next = JSON.stringify({ ...sizes, columns });
    if (stored === next) return;
    try { localStorage.setItem(key, next); stored = next; }
    catch { error('分栏宽度未能保存'); }
  }

  function move(kind: keyof SidebarSizes, width: number, origin: SidebarLayout) {
    const collapse = kind === 'folders' ? width <= sidebarLimits.folderSnap : width < sidebarLimits.noteSnap;
    if (collapse) {
      if (drag) sizes[kind] = drag.sizes[kind];
      applyLayout({ columns: kind === 'folders' ? 'list' : 'note', unfolding: false });
    } else {
      if (kind === 'folders') folderReferenceWidth = innerWidth;
      else if (drag) folderReferenceWidth = drag.referenceWidth;
      sizes[kind] = Math.max(kind === 'folders' ? sidebarLimits.folderSnap + 1 : sidebarLimits.noteSnap, width);
      sizes[kind] = fitSidebarSizes(sizes, origin.columns, innerWidth, folderReferenceWidth)[kind];
      applyLayout({ ...origin });
    }
  }

  function finish(cancel = false) {
    if (!drag) return;
    const previous = drag;
    drag = undefined;
    delete document.body.dataset.libraryResizing;
    if (shell.hasPointerCapture(previous.id)) shell.releasePointerCapture(previous.id);
    if (cancel) {
      sizes = previous.sizes; folderReferenceWidth = previous.referenceWidth; previousColumns = previous.layout.columns;
      applyLayout(previous.layout);
    }
    else update();
  }

  for (const kind of ['folders', 'notes'] as const) {
    const handle = handles[kind];
    handle.onpointerdown = event => {
      if (event.button !== 0 || innerWidth <= 900 || drag) return;
      event.preventDefault();
      drag = { id: event.pointerId, kind, x: event.clientX, width: handle.parentElement!.getBoundingClientRect().width, sizes: { ...sizes }, layout: { ...getLayout() }, referenceWidth: folderReferenceWidth };
      document.body.dataset.libraryResizing = kind;
      // Capture on the shell: the dragged column and its handle may collapse.
      shell.setPointerCapture(event.pointerId);
    };
    handle.onkeydown = event => {
      if (innerWidth <= 900 || !['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
      event.preventDefault();
      const width = Number(handle.getAttribute('aria-valuenow'));
      move(kind, event.key === 'Home' ? 0 : event.key === 'End' ? Infinity : width + (event.key === 'ArrowLeft' ? -10 : 10), getLayout());
    };
  }
  shell.addEventListener('pointermove', event => {
    if (drag && event.pointerId === drag.id) move(drag.kind, drag.width + event.clientX - drag.x, drag.layout);
  });
  shell.addEventListener('pointerup', event => { if (drag?.id === event.pointerId) finish(); });
  shell.addEventListener('pointercancel', event => { if (drag?.id === event.pointerId) finish(true); });
  shell.addEventListener('lostpointercapture', event => { if (drag?.id === event.pointerId) finish(true); });
  window.addEventListener('blur', () => finish(true));
  document.addEventListener('keydown', event => { if (drag && event.key === 'Escape') { event.preventDefault(); finish(true); } });
  return { columns: external ? 'list' as const : saved.columns, update };
}
