import glyphText from "../scripts/font-glyphs.txt?raw";
import { siteContent, stringsInSiteCopy } from "./content";

describe("QingYu WenKai subset", () => {
  it("covers every non-ASCII character in static site content", () => {
    const required = new Set(
      stringsInSiteCopy(siteContent).join("").match(/[^\u0000-\u007f]/gu) ?? []
    );
    const available = new Set(glyphText);

    expect([...required].filter((character) => !available.has(character))).toEqual([]);
  });
});
