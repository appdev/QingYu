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
  expect(restoreSidebarSizes(null)).toEqual({ folders: 138, notes: 280, columns: 'all' });
  expect(restoreSidebarSizes({ folders: 90, notes: 900, columns: 'list' })).toEqual({ folders: 138, notes: 600, columns: 'list' });
  expect(restoreSidebarSizes({ folders: Infinity, notes: '300', columns: 'unknown' })).toEqual({ folders: 138, notes: 280, columns: 'all' });
  expect(restoreSidebarSizes({ folders: 250, notes: 400, columns: 'note' })).toEqual({ folders: 250, notes: 400, columns: 'note' });
  expect(restoreSidebarSizes({ folders: 200, notes: 180 })).toEqual({ folders: 200, notes: 220, columns: 'all' });
});

it('preserves editor space as the window shrinks without overwriting preferred widths', () => {
  const preferred = { folders: 280, notes: 600 };
  expect(fitSidebarSizes(preferred, 'all', 1600, 1600)).toEqual(preferred);
  expect(fitSidebarSizes(preferred, 'all', 1120, 1120)).toEqual({ folders: 280, notes: 360 });
  expect(fitSidebarSizes(preferred, 'all', 901, 901)).toEqual({ folders: 241, notes: 180 });
  expect(fitSidebarSizes(preferred, 'list', 1120, 1120)).toEqual({ folders: 0, notes: 600 });
  expect(fitSidebarSizes(preferred, 'note', 1120, 1120)).toEqual({ folders: 0, notes: 0 });
  expect(preferred).toEqual({ folders: 280, notes: 600 });
});

it('uses the native proportional sidebar rule and keeps 180–219 valid during a drag', () => {
  const preferred = { folders: 200, notes: 300 };
  const at1000 = fitSidebarSizes(preferred, 'all', 1000, 1120);
  expect(at1000.folders).toBeCloseTo(178.5714, 3);
  expect(at1000.notes).toBe(300);
  expect(fitSidebarSizes(preferred, 'all', 960, 1120).folders).toBeCloseTo(171.4286, 3);
  expect(fitSidebarSizes(preferred, 'all', 1120, 1120)).toEqual(preferred);
  for (const notes of [180, 219]) expect(fitSidebarSizes({ folders: 200, notes }, 'all', 1120, 1120).notes).toBe(notes);
  expect(preferred).toEqual({ folders: 200, notes: 300 });
});
