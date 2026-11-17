import type { MarkdownExtension } from "@lezer/markdown";
import { Facet, StateEffect } from "@codemirror/state";

type WikiLinkExists = (target: string) => boolean;
export const wikiLinkAvailability = Facet.define<WikiLinkExists, WikiLinkExists | undefined>({
  combine: checks => checks[0],
});
export const wikiLinksChanged = StateEffect.define<null>();

const wikiSyntax = /^\[\[([^\[\]\r\n]+)\]\]$/u;
const noteScheme = "qingyu-note:";

export function readWikiLink(markdown: string) {
  const content = wikiSyntax.exec(markdown)?.[1];
  if (!content) return null;
  const bar = content.indexOf("|");
  const target = (bar < 0 ? content : content.slice(0, bar).replace(/\\$/u, "")).trim();
  const alias = bar < 0 ? "" : content.slice(bar + 1).trim();
  if (!target || /[\u0000-\u001f\u007f]/u.test(target)) return null;
  const text = alias || target.split("/").pop() || target;
  try { return { target, text, href: noteScheme + encodeURIComponent(target) }; }
  catch { return null; }
}

export function wikiLinkTarget(href: string) {
  if (!href.startsWith(noteScheme)) return null;
  try {
    const target = decodeURIComponent(href.slice(noteScheme.length));
    return readWikiLink(`[[${target}]]`)?.target ?? null;
  } catch {
    return null;
  }
}

export const markraWikiLink: MarkdownExtension = {
  defineNodes: ["WikiLink", "WikiCard"],
  parseInline: [{
    name: "WikiLink",
    before: "Link",
    parse(context, next, start) {
      const card = next === 33;
      if (!card && next !== 91) return -1;
      const brackets = start + Number(card);
      if (context.char(brackets) !== 91 || context.char(brackets + 1) !== 91) return -1;
      let end = brackets + 2;
      while (end < context.end && ![10, 13, 91, 93].includes(context.char(end))) end++;
      if (context.char(end) !== 93 || context.char(end + 1) !== 93) return -1;
      if (card && [40, 91].includes(context.char(end + 2))) return -1;
      if (!readWikiLink(context.slice(brackets, end + 2))) return -1;
      return context.addElement(context.elt(card ? "WikiCard" : "WikiLink", start, end + 2));
    },
  }],
};
