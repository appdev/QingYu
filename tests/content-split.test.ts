import { expect, it } from 'vitest';
import { constrainContentSplit, restoreContentSplit } from '../src/content-split';

it('uses the native 200px drag limits instead of 25–75 percent', () => {
  expect(constrainContentSplit(620, 50)).toBe(200);
  expect(constrainContentSplit(620, 1000)).toBe(420);
  expect(constrainContentSplit(2000, 1800)).toBe(1800);
});

it('restores a saved ratio with a 1200px cap, or defaults to half capped at 600', () => {
  expect(restoreContentSplit(620, 0)).toBe(310);
  expect(restoreContentSplit(2000, 0)).toBe(600);
  expect(restoreContentSplit(620, 300 / 620)).toBe(300);
  expect(restoreContentSplit(620, .1)).toBe(200);
  expect(restoreContentSplit(2000, .9)).toBe(1200);
});
