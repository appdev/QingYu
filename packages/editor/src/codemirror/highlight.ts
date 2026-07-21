import type {
  DelimiterType,
  MarkdownExtension,
} from "@lezer/markdown";

const highlightDelimiter: DelimiterType = {
  mark: "HighlightMark",
  resolve: "Highlight",
};

export const markraHighlight: MarkdownExtension = {
  defineNodes: ["Highlight", "HighlightMark"],
  parseInline: [
    {
      after: "Emphasis",
      name: "MarkraHighlight",
      parse(context, next, position) {
        if (
          next !== 61 ||
          context.char(position + 1) !== 61 ||
          context.char(position - 1) === 61 ||
          context.char(position + 2) === 61
        ) {
          return -1;
        }

        const before = context.slice(position - 1, position);
        const after = context.slice(position + 2, position + 3);
        const open = Boolean(after && !/\s/u.test(after));
        const close = Boolean(before && !/\s/u.test(before));
        return context.addDelimiter(
          highlightDelimiter,
          position,
          position + 2,
          open,
          close,
        );
      },
    },
  ],
};
