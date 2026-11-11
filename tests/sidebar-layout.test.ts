import { expect, it } from 'vitest';
import { nextSidebarLayout, type SidebarLayout } from '../src/sidebar-layout';

// Contract: MiaoYan/Controllers/ViewController+Layout.swift, toggleLayoutCycle,
// toggleSidebarPanel and toggleNoteList; cycle verified against the 4.8.0 app.
it('matches every reference layout/direction transition and its independent shortcuts', () => {
  const cases: [SidebarLayout, SidebarLayout, SidebarLayout, SidebarLayout][] = [
    [{ columns: 'all', unfolding: false }, { columns: 'list', unfolding: false }, { columns: 'list', unfolding: false }, { columns: 'note', unfolding: false }],
    [{ columns: 'all', unfolding: true }, { columns: 'list', unfolding: false }, { columns: 'list', unfolding: true }, { columns: 'note', unfolding: true }],
    [{ columns: 'list', unfolding: false }, { columns: 'note', unfolding: false }, { columns: 'all', unfolding: false }, { columns: 'note', unfolding: false }],
    [{ columns: 'list', unfolding: true }, { columns: 'all', unfolding: false }, { columns: 'all', unfolding: true }, { columns: 'note', unfolding: true }],
    [{ columns: 'note', unfolding: false }, { columns: 'list', unfolding: true }, { columns: 'all', unfolding: false }, { columns: 'list', unfolding: false }],
    [{ columns: 'note', unfolding: true }, { columns: 'list', unfolding: true }, { columns: 'all', unfolding: true }, { columns: 'list', unfolding: true }],
  ];
  for (const [before, cycle, folders, notes] of cases) {
    const original = { ...before };
    expect(nextSidebarLayout(before, 'cycle')).toEqual(cycle);
    expect(nextSidebarLayout(before, 'folders')).toEqual(folders);
    expect(nextSidebarLayout(before, 'notes')).toEqual(notes);
    expect(before).toEqual(original);
  }
});
