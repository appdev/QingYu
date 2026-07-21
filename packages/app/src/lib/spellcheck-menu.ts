import type { SpellcheckMatch } from "@markra/editor";

type SpellcheckCoordinateReader = {
  coordsAtPos: (position: number) => { bottom: number; left: number } | null;
};

export function spellcheckMenuPosition(
  view: SpellcheckCoordinateReader,
  match: SpellcheckMatch,
) {
  try {
    const coordinates = view.coordsAtPos(match.to);
    if (!coordinates) return { left: 8, top: 8 };

    return {
      left: Math.max(8, coordinates.left),
      top: Math.max(8, coordinates.bottom + 6),
    };
  } catch {
    return { left: 8, top: 8 };
  }
}

export function mergeSpellcheckIgnoredWords(
  currentWords: readonly string[],
  word: string,
) {
  const ignoredWords: string[] = [];
  const seenWords = new Set<string>();

  for (const item of [...currentWords, word]) {
    const normalizedWord = item.trim().toLocaleLowerCase();
    if (!normalizedWord || seenWords.has(normalizedWord)) continue;

    seenWords.add(normalizedWord);
    ignoredWords.push(normalizedWord);
  }

  return ignoredWords;
}
