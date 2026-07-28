import { renderToString, type KatexOptions } from "katex";

export type MarkraMathKind = "display" | "inline";
export type MarkraMathMacros = NonNullable<KatexOptions["macros"]>;

export function createMarkraMathMacros(): MarkraMathMacros {
  return {};
}

export function renderMarkraMathToString(
  tex: string,
  kind: MarkraMathKind,
  macros = createMarkraMathMacros(),
) {
  return renderToString(tex, {
    displayMode: kind === "display",
    globalGroup: true,
    macros,
    output: "htmlAndMathml",
    strict: "ignore",
    throwOnError: false,
  });
}

function isEscaped(source: string, index: number) {
  let backslashes = 0;
  for (let cursor = index - 1; cursor >= 0 && source[cursor] === "\\"; cursor -= 1) {
    backslashes += 1;
  }
  return backslashes % 2 === 1;
}

function skipMathWhitespace(source: string, index: number) {
  let cursor = index;
  while (cursor < source.length && /\s/u.test(source[cursor] ?? "")) cursor += 1;
  return cursor;
}

function readMathCommand(source: string, index: number) {
  if (source[index] !== "\\") return null;

  let cursor = index + 1;
  while (cursor < source.length && /[a-zA-Z]/u.test(source[cursor] ?? "")) cursor += 1;
  if (cursor === index + 1 && cursor < source.length) cursor += 1;

  return {
    command: source.slice(index, cursor),
    to: cursor,
  };
}

function readBalancedMathGroup(
  source: string,
  index: number,
  open: "{" | "[",
  close: "}" | "]",
) {
  if (source[index] !== open) return null;

  let depth = 0;
  for (let cursor = index; cursor < source.length; cursor += 1) {
    const character = source[cursor];
    if (character === open && !isEscaped(source, cursor)) {
      depth += 1;
      continue;
    }

    if (character !== close || isEscaped(source, cursor)) continue;

    depth -= 1;
    if (depth === 0) return cursor + 1;
  }

  return null;
}

function readMathMacroNameArgument(source: string, index: number) {
  const cursor = skipMathWhitespace(source, index);
  if (source[cursor] === "{") {
    const groupEnd = readBalancedMathGroup(source, cursor, "{", "}");
    return groupEnd === null ? null : groupEnd;
  }

  return readMathCommand(source, cursor)?.to ?? null;
}

function readOptionalMathMacroArgument(source: string, index: number) {
  const cursor = skipMathWhitespace(source, index);
  if (source[cursor] !== "[") return index;

  return readBalancedMathGroup(source, cursor, "[", "]");
}

function readRequiredMathMacroReplacement(source: string, index: number) {
  const cursor = skipMathWhitespace(source, index);
  return readBalancedMathGroup(source, cursor, "{", "}");
}

function readNewcommandDefinition(source: string, index: number) {
  const command = readMathCommand(source, index);
  if (
    !command ||
    !["\\newcommand", "\\renewcommand", "\\providecommand"].includes(command.command)
  ) {
    return null;
  }

  let cursor = skipMathWhitespace(source, command.to);
  if (source[cursor] === "*") cursor += 1;

  const macroNameEnd = readMathMacroNameArgument(source, cursor);
  if (macroNameEnd === null) return null;
  cursor = macroNameEnd;

  const argumentCountEnd = readOptionalMathMacroArgument(source, cursor);
  if (argumentCountEnd === null) return null;
  cursor = argumentCountEnd;

  const optionalDefaultEnd = readOptionalMathMacroArgument(source, cursor);
  if (optionalDefaultEnd === null) return null;
  cursor = optionalDefaultEnd;

  return readRequiredMathMacroReplacement(source, cursor);
}

export function isMarkraMathMacroDefinitionSource(tex: string) {
  let cursor = skipMathWhitespace(tex, 0);
  let foundDefinition = false;

  while (cursor < tex.length) {
    const nextCursor = readNewcommandDefinition(tex, cursor);
    if (nextCursor === null) return false;

    foundDefinition = true;
    cursor = skipMathWhitespace(tex, nextCursor);
  }

  return foundDefinition;
}
