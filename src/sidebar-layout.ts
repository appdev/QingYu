export type SidebarLayout = { columns: 'all' | 'list' | 'note'; unfolding: boolean };
export type SidebarCommand = 'cycle' | 'folders' | 'notes';

// The folder column includes the 8 px inset at the window edge.
// 笔记列表默认和最小都是 297（原来 330 收窄一成）。
export const sidebarLimits = { folderMin: 248, folderMax: 368, folderSnap: 120, noteMin: 297, noteMax: 440, editorMin: 360 };
export const folderFit = { hideBelow: 1100, restoreAt: 1220 };
export type SidebarSizes = { folders: number; notes: number };

export function restoreSidebarSizes(value: unknown): SidebarSizes & { columns: SidebarLayout['columns'] } {
  const saved = value && typeof value === 'object' ? value as Record<string, unknown> : {};
  const width = (value: unknown, fallback: number, min: number, max: number) => typeof value === 'number' && Number.isFinite(value) ? Math.max(min, Math.min(max, value)) : fallback;
  // The previous app saved its defaults on first launch, even before a drag.
  const oldDefault = saved.version === undefined && saved.folders === 248 && saved.notes === 330;
  // 版本 3 之前列表宽度存的 330 就是当时的默认和最小值，随新默认一起收窄。
  const oldNotesDefault = (typeof saved.version !== 'number' || saved.version < 3) && saved.notes === 330;
  return {
    folders: width(oldDefault ? undefined : saved.folders, 268, sidebarLimits.folderMin, sidebarLimits.folderMax),
    notes: width(oldNotesDefault ? undefined : saved.notes, sidebarLimits.noteMin, sidebarLimits.noteMin, sidebarLimits.noteMax),
    columns: saved.columns === 'list' || saved.columns === 'note' ? saved.columns : 'all',
  };
}

export function fitSidebarSizes(sizes: SidebarSizes, columns: SidebarLayout['columns'], total: number): SidebarSizes {
  if (columns === 'note') return { folders: 0, notes: 0 };
  const available = Math.max(0, total - sidebarLimits.editorMin);
  const folders = columns === 'all'
    ? Math.max(0, Math.min(sizes.folders, available - sidebarLimits.noteMin)) : 0;
  return { folders, notes: Math.min(sizes.notes, available - folders) };
}

// MiaoYan ViewController+Layout.toggleLayoutCycle: full → list → note → list → full.
export function nextSidebarLayout(state: SidebarLayout, command: SidebarCommand): SidebarLayout {
  if (command === 'folders') return { ...state, columns: state.columns === 'all' ? 'list' : 'all' };
  if (command === 'notes') return { ...state, columns: state.columns === 'note' ? 'list' : 'note' };
  switch (state.columns) {
    case 'all': return { columns: 'list', unfolding: false };
    case 'list': return { columns: state.unfolding ? 'all' : 'note', unfolding: false };
    case 'note': return { columns: 'list', unfolding: true };
  }
}
