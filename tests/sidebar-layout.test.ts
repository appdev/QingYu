import { expect, it } from 'vitest';
import { fitSidebarSizes, nextSidebarLayout, restoreSidebarSizes, type SidebarLayout } from '../src/sidebar-layout';

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

it('restores bounded widths and collapsed columns without trusting stored data', () => {
  expect(restoreSidebarSizes(null)).toEqual({ folders: 268, notes: 330, columns: 'all' });
  expect(restoreSidebarSizes({ folders: 90, notes: 900, columns: 'list' })).toEqual({ folders: 248, notes: 440, columns: 'list' });
  expect(restoreSidebarSizes({ folders: Infinity, notes: '300', columns: 'unknown' })).toEqual({ folders: 268, notes: 330, columns: 'all' });
  expect(restoreSidebarSizes({ folders: 250, notes: 400, columns: 'note' })).toEqual({ folders: 250, notes: 400, columns: 'note' });
  expect(restoreSidebarSizes({ folders: 200, notes: 180 })).toEqual({ folders: 248, notes: 330, columns: 'all' });
  expect(restoreSidebarSizes({ folders: 248, notes: 330, columns: 'list' })).toEqual({ folders: 268, notes: 330, columns: 'list' });
  expect(restoreSidebarSizes({ version: 2, folders: 248, notes: 330, columns: 'list' })).toEqual({ folders: 248, notes: 330, columns: 'list' });
});

it('preserves editor space as the window shrinks without overwriting preferred widths', () => {
  const preferred = { folders: 368, notes: 440 };
  expect(fitSidebarSizes(preferred, 'all', 1258)).toEqual(preferred);
  expect(fitSidebarSizes(preferred, 'all', 1120)).toEqual({ folders: 368, notes: 392 });
  expect(fitSidebarSizes(preferred, 'all', 901)).toEqual({ folders: 211, notes: 330 });
  expect(fitSidebarSizes(preferred, 'list', 1120)).toEqual({ folders: 0, notes: 440 });
  expect(fitSidebarSizes(preferred, 'note', 1120)).toEqual({ folders: 0, notes: 0 });
  expect(preferred).toEqual({ folders: 368, notes: 440 });
});

it('keeps both default columns fixed down to a 960 px window', () => {
  const preferred = { folders: 268, notes: 330 };
  for (const width of [1258, 1200, 1000, 960]) expect(fitSidebarSizes(preferred, 'all', width)).toEqual(preferred);
});
