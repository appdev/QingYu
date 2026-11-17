type Change = 'added' | 'removed';
type VersionLine = { text: string; change?: Change };
const MAX_LINE_PAIRS = 1_000_000;

export function diffVersionLines(before: string, after: string): VersionLine[] {
  const old = before.split(/\r\n|\r|\n/), next = after.split(/\r\n|\r|\n/);
  const result: VersionLine[] = [];
  let first = 0, endOld = old.length, endNext = next.length;
  while (first < endOld && first < endNext && old[first] === next[first]) {
    result.push({ text: next[first++] });
  }
  while (endOld > first && endNext > first && old[endOld - 1] === next[endNext - 1]) {
    endOld--; endNext--;
  }
  const cols = endNext - first + 1;
  const pairs = (endOld - first + 1) * cols;
  // Keep previews responsive when a large section is rewritten.
  if (pairs > MAX_LINE_PAIRS) {
    for (let i = first; i < endOld; i++) result.push({ text: old[i], change: 'removed' });
    for (let j = first; j < endNext; j++) result.push({ text: next[j], change: 'added' });
  } else {
    const lengths = new Uint32Array(pairs);
    const cell = (i: number, j: number) => (i - first) * cols + j - first;
    for (let i = endOld - 1; i >= first; i--) {
      for (let j = endNext - 1; j >= first; j--) {
        lengths[cell(i, j)] = old[i] === next[j] ? lengths[cell(i + 1, j + 1)] + 1
          : Math.max(lengths[cell(i + 1, j)], lengths[cell(i, j + 1)]);
      }
    }
    let i = first, j = first;
    while (i < endOld || j < endNext) {
      if (i < endOld && j < endNext && old[i] === next[j]) { result.push({ text: next[j++] }); i++; }
      else if (i < endOld && (j === endNext || lengths[cell(i + 1, j)] >= lengths[cell(i, j + 1)])) result.push({ text: old[i++], change: 'removed' });
      else result.push({ text: next[j++], change: 'added' });
    }
  }
  for (let j = endNext; j < next.length; j++) result.push({ text: next[j] });
  return result;
}
