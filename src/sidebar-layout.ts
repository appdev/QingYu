export type SidebarLayout = { columns: 'all' | 'list' | 'note'; unfolding: boolean };
export type SidebarCommand = 'cycle' | 'folders' | 'notes';

// MiaoYan's default AppKit window, not its experimental SwiftUI shell.
export const sidebarLimits = { folderMin: 138, folderMax: 280, folderSnap: 86, noteMin: 220, noteMax: 600, noteSnap: 180, editorMin: 480 };
export type SidebarSizes = { folders: number; notes: number };

export function restoreSidebarSizes(value: unknown): SidebarSizes & { columns: SidebarLayout['columns'] } {
  const saved = value && typeof value === 'object' ? value as Record<string, unknown> : {};
  const width = (value: unknown, fallback: number, min: number, max: number) => typeof value === 'number' && Number.isFinite(value) ? Math.max(min, Math.min(max, value)) : fallback;
  return {
    folders: width(saved.folders, sidebarLimits.folderMin, sidebarLimits.folderMin, sidebarLimits.folderMax),
    notes: width(saved.notes, 280, sidebarLimits.noteMin, sidebarLimits.noteMax),
    columns: saved.columns === 'list' || saved.columns === 'note' ? saved.columns : 'all',
  };
}

export function fitSidebarSizes(sizes: SidebarSizes, columns: SidebarLayout['columns'], total: number, folderReferenceWidth: number): SidebarSizes {
  if (columns === 'note') return { folders: 0, notes: 0 };
  const available = Math.max(0, total - sidebarLimits.editorMin);
  const folders = columns === 'all'
    ? Math.max(sidebarLimits.folderSnap + 1, Math.min(sizes.folders * total / folderReferenceWidth, sidebarLimits.folderMax, available - sidebarLimits.noteSnap)) : 0;
  return { folders, notes: Math.max(sidebarLimits.noteSnap, Math.min(sizes.notes, sidebarLimits.noteMax, available - folders)) };
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
