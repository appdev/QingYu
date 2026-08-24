export const APPEARANCE_FIXTURE_MARKDOWN = `Appearance context paragraph.

# Heading 1
## Heading 2
### Heading 3
#### Heading 4
##### Heading 5
###### Heading 6

Setext heading
--------------

Paragraph with **strong**, *emphasis*, ~~strike~~, ==mark==, \`inline code\`, [link](https://example.com), and $x^2$.

- list item
  1. ordered child
- [x] completed task

> quote

> **Quoted list**
>
> - quoted item
>
> 1. quoted ordered item
>
> - [ ] quoted task

> [!NOTE]
> Note callout

> [!TIP]
> Tip callout

> [!IMPORTANT]
> Important callout

> [!WARNING]
> Warning callout

> [!CAUTION]
> Caution callout

---

| Head | Value |
| --- | --- |
| Cell | Text |

\`\`\`java
const message = "theme parity";
if (message) {
    console.log(message);
}
\`\`\`

\`\`\`mermaid
graph TD
    A --> B
\`\`\`

![image](data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIyNDAiIGhlaWdodD0iMTIwIiB2aWV3Qm94PSIwIDAgMjQwIDEyMCI+PHJlY3Qgd2lkdGg9IjI0MCIgaGVpZ2h0PSIxMjAiIHJ4PSIxMiIgZmlsbD0iIzhhYjRmOCIvPjxjaXJjbGUgY3g9IjcwIiBjeT0iNTIiIHI9IjIwIiBmaWxsPSIjZmZmIiBmaWxsLW9wYWNpdHk9Ii44Ii8+PHBhdGggZD0iTTMwIDEwNWw1NC00MiAzNSAyNiAzNi0zMyA1NSA0OXoiIGZpbGw9IiMyZjVmOTgiLz48L3N2Zz4=)

$$x^2 + y^2$$

$$
\\newcommand{\\appearanceMacro}{appearance}
$$

Footnote reference[^appearance].

[^appearance]: Footnote preview

<div>raw HTML</div>`;

export const createNativeAppearanceFixture = (document: Document, blockDOM: string) => {
    const root = document.createElement("div");
    root.className = "protyle-wysiwyg";
    root.dataset.appearanceFixture = "native";
    root.innerHTML = blockDOM;
    root.querySelectorAll<HTMLElement>(".bq").forEach((blockquote) => {
        const editable = blockquote.querySelector<HTMLElement>("[contenteditable=true]");
        const match = /^\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\]\s*\n?([\s\S]*)$/u.exec(editable?.textContent ?? "");
        if (!editable || !match) return;
        const callout = document.createElement("div");
        callout.className = "callout";
        callout.dataset.type = "NodeCallout";
        callout.dataset.subtype = match[1];
        const info = document.createElement("div");
        info.className = "callout-info";
        const icon = document.createElement("span");
        icon.className = "callout-icon";
        const title = document.createElement("span");
        title.className = "callout-title";
        title.textContent = match[1][0] + match[1].slice(1).toLowerCase();
        info.append(icon, title);
        const content = document.createElement("div");
        content.className = "callout-content";
        const paragraph = blockquote.querySelector<HTMLElement>(":scope > .p");
        if (paragraph) {
            editable.textContent = match[2];
            content.append(paragraph);
        }
        const attributes = blockquote.querySelector<HTMLElement>(":scope > .protyle-attr");
        callout.append(info, content);
        if (attributes) callout.append(attributes);
        blockquote.replaceWith(callout);
    });
    return root;
};

export const createNativeHeadingContextFixtures = (
    document: Document,
    lute: {Md2BlockDOM(markdown: string): string},
) => {
    const contexts = document.createElement("div");
    contexts.className = "markdown-appearance-heading-contexts";
    for (let level = 1; level <= 6; level++) {
        const root = createNativeAppearanceFixture(
            document,
            lute.Md2BlockDOM(`${"#".repeat(level)} First heading ${level}`),
        );
        root.dataset.markdownAppearanceContext = `heading-${level}-first`;
        contexts.append(root);
    }
    return contexts;
};
