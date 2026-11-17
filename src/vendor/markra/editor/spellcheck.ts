import type { ITrie } from "cspell-trie-lib";

export type Spellchecker = {
  check: (word: string) => boolean;
  isReady?: () => boolean;
  load?: () => Promise<unknown>;
  suggest?: (word: string) => string[];
};

export type SpellcheckToken = {
  from: number;
  text: string;
  to: number;
};

export type SpellcheckMatch = {
  from: number;
  suggestions: string[];
  to: number;
  word: string;
};

export type SpellcheckOptions = {
  enabled?: boolean;
  ignoredWords?: readonly string[];
  minWordLength?: number;
  spellchecker?: Spellchecker;
};

const urlPattern = /\b(?:https?:\/\/|www\.)[^\s<>()]+/giu;
const wordPattern = /[\p{Script=Latin}\p{Script=Cyrillic}]+(?:['’][\p{Script=Latin}\p{Script=Cyrillic}]+)?(?:-[\p{Script=Latin}\p{Script=Cyrillic}]+(?:['’][\p{Script=Latin}\p{Script=Cyrillic}]+)?)*/gu;
const snakeCaseIdentifierPattern = /[\p{Script=Latin}\p{Script=Cyrillic}][\p{Script=Latin}\p{Script=Cyrillic}\d]*(?:_[\p{Script=Latin}\p{Script=Cyrillic}\d]+)+/giu;
const defaultMinWordLength = 2;
const defaultKnownMisspellings = [
  ["adress", ["address"]],
  ["definately", ["definitely"]],
  ["enviroment", ["environment"]],
  ["langauge", ["language"]],
  ["occured", ["occurred"]],
  ["recieve", ["receive"]],
  ["seperate", ["separate"]],
  ["teh", ["the"]],
  ["untill", ["until"]],
  ["wich", ["which"]]
] as const;

export const defaultEnglishSpellchecker = createKnownMisspellingSpellchecker(defaultKnownMisspellings);

export function createLazyCspellTrieSpellchecker(
  loadTrie: () => ITrie,
  options: {
    preferredSuggestions?: Iterable<readonly [word: string, suggestions: readonly string[]]>;
  } = {}
): Spellchecker {
  let spellchecker: Spellchecker | null = null;

  const getSpellchecker = () => {
    if (!spellchecker) {
      spellchecker = createCspellTrieSpellchecker(loadTrie(), options);
    }

    return spellchecker;
  };

  return {
    check(word) {
      return getSpellchecker().check(word);
    },
    isReady() {
      return true;
    },
    async load() {
      getSpellchecker();
    },
    suggest(word) {
      return getSpellchecker().suggest?.(word) ?? [];
    }
  };
}

export function createAsyncCspellTrieSpellchecker(
  loadTrie: () => Promise<ITrie>,
  options: {
    preferredSuggestions?: Iterable<readonly [word: string, suggestions: readonly string[]]>;
  } = {}
): Spellchecker {
  let loading: Promise<unknown> | null = null;
  let spellchecker: Spellchecker | null = null;
  let failed = false;

  const load = () => {
    if (spellchecker || failed) return Promise.resolve();
    if (!loading) {
      loading = loadTrie()
        .then((trie) => {
          spellchecker = createCspellTrieSpellchecker(trie, options);
        })
        .catch(() => {
          failed = true;
        });
    }

    return loading;
  };

  return {
    check(word) {
      if (!spellchecker) {
        load().catch(() => {});
        return true;
      }

      return spellchecker.check(word);
    },
    isReady() {
      return Boolean(spellchecker);
    },
    load,
    suggest(word) {
      return spellchecker?.suggest?.(word) ?? [];
    }
  };
}

export function createCspellTrieSpellchecker(
  trie: ITrie,
  options: {
    preferredSuggestions?: Iterable<readonly [word: string, suggestions: readonly string[]]>;
  } = {}
): Spellchecker {
  const preferredSuggestions = createSuggestionMap(options.preferredSuggestions ?? []);

  return {
    check(word) {
      const normalizedWord = normalizeSpellcheckWord(word);
      if (!normalizedWord) return true;
      if (hasTrieWord(trie, normalizedWord)) return true;
      if (normalizedWord.includes("-")) {
        return normalizedWord
          .split("-")
          .filter(Boolean)
          .every((part) => hasTrieWord(trie, part));
      }

      return false;
    },
    suggest(word) {
      const normalizedWord = normalizeSpellcheckWord(word);
      if (!normalizedWord) return [];

      return preferredSuggestions.get(normalizedWord)
        ?? trie
          .suggest(normalizedWord, {
            ignoreCase: true,
            numSuggestions: 5,
            timeout: 25
          });
    }
  };
}

export function createKnownMisspellingSpellchecker(
  entries: Iterable<readonly [word: string, suggestions: readonly string[]]>
): Spellchecker {
  const misspellings = createSuggestionMap(entries);

  return {
    check(word) {
      const normalizedWord = normalizeSpellcheckWord(word);
      if (!normalizedWord) return true;

      return !misspellings.has(normalizedWord);
    },
    suggest(word) {
      return misspellings.get(normalizeSpellcheckWord(word)) ?? [];
    }
  };
}

export function createWordSetSpellchecker(words: Iterable<string>): Spellchecker {
  const dictionary = new Set<string>();

  for (const word of words) {
    const normalizedWord = normalizeSpellcheckWord(word);
    if (normalizedWord) {
      dictionary.add(normalizedWord);
    }
  }

  return {
    check(word) {
      const normalizedWord = normalizeSpellcheckWord(word);
      if (!normalizedWord) return true;
      if (dictionary.has(normalizedWord)) return true;
      if (normalizedWord.includes("-")) {
        return normalizedWord
          .split("-")
          .filter(Boolean)
          .every((part) => dictionary.has(part));
      }

      return false;
    },
    suggest(word) {
      return suggestClosestWords(normalizeSpellcheckWord(word), dictionary);
    }
  };
}

export function tokenizeSpellcheckText(text: string, options: Pick<SpellcheckOptions, "minWordLength"> = {}) {
  const minWordLength = options.minWordLength ?? defaultMinWordLength;
  const skippedRanges = [
    ...findUrlRanges(text),
    ...findSnakeCaseIdentifierRanges(text)
  ];
  const tokens: SpellcheckToken[] = [];

  for (const match of text.matchAll(wordPattern)) {
    const token = match[0];
    const from = match.index ?? 0;
    const to = from + token.length;
    if (token.length < minWordLength) continue;
    if (rangeOverlaps(from, to, skippedRanges)) continue;
    if (shouldSkipToken(token)) continue;

    tokens.push({ from, text: token, to });
  }

  return tokens;
}

function findUrlRanges(text: string) {
  return Array.from(text.matchAll(urlPattern), (match) => {
    const from = match.index ?? 0;

    return {
      from,
      to: from + match[0].length
    };
  });
}

function findSnakeCaseIdentifierRanges(text: string) {
  return Array.from(text.matchAll(snakeCaseIdentifierPattern), (match) => {
    const from = match.index ?? 0;

    return {
      from,
      to: from + match[0].length
    };
  });
}

function rangeOverlaps(from: number, to: number, ranges: Array<{ from: number; to: number }>) {
  return ranges.some((range) => from < range.to && range.from < to);
}

function shouldSkipToken(token: string) {
  if (/^[A-Z]{2,}$/u.test(token)) return true;
  if (/[0-9]/u.test(token)) return true;
  if (isMixedCaseIdentifier(token)) return true;

  return false;
}

function isMixedCaseIdentifier(token: string) {
  return /[\p{Ll}][\p{Lu}]/u.test(token) || /^[\p{Lu}]{2,}[\p{Ll}]/u.test(token);
}

function createSuggestionMap(entries: Iterable<readonly [word: string, suggestions: readonly string[]]>) {
  const suggestionsByWord = new Map<string, string[]>();

  for (const [word, suggestions] of entries) {
    const normalizedWord = normalizeSpellcheckWord(word);
    if (normalizedWord) {
      suggestionsByWord.set(normalizedWord, suggestions.map((suggestion) => normalizeSpellcheckWord(suggestion)));
    }
  }

  return suggestionsByWord;
}

function hasTrieWord(trie: ITrie, word: string) {
  const result = trie.findWord(word, {
    caseSensitive: false,
    checkForbidden: true
  });

  return Boolean(result.found) && !result.forbidden;
}

function normalizeSpellcheckWord(word: string) {
  return word
    .trim()
    .replaceAll("’", "'")
    .toLocaleLowerCase();
}

function suggestClosestWords(word: string, dictionary: Set<string>) {
  if (!word || word.length < 3) return [];

  return Array.from(dictionary)
    .filter((candidate) => Math.abs(candidate.length - word.length) <= 2)
    .map((candidate) => ({
      distance: levenshteinDistance(word, candidate),
      word: candidate
    }))
    .filter((candidate) => candidate.distance <= 2)
    .sort((first, second) => first.distance - second.distance || first.word.localeCompare(second.word))
    .slice(0, 5)
    .map((candidate) => candidate.word);
}

function levenshteinDistance(first: string, second: string) {
  const previous = Array.from({ length: second.length + 1 }, (_, index) => index);
  const current = Array.from({ length: second.length + 1 }, () => 0);

  for (let firstIndex = 1; firstIndex <= first.length; firstIndex += 1) {
    current[0] = firstIndex;
    for (let secondIndex = 1; secondIndex <= second.length; secondIndex += 1) {
      const substitutionCost = first[firstIndex - 1] === second[secondIndex - 1] ? 0 : 1;
      current[secondIndex] = Math.min(
        current[secondIndex - 1] + 1,
        previous[secondIndex] + 1,
        previous[secondIndex - 1] + substitutionCost
      );
    }
    previous.splice(0, previous.length, ...current);
  }

  return previous[second.length] ?? 0;
}
