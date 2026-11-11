export type SidebarLayout = { columns: 'all' | 'list' | 'note'; unfolding: boolean };
export type SidebarCommand = 'cycle' | 'folders' | 'notes';

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
