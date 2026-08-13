export const APPEARANCE_FIXTURE_MARKDOWN = `# Heading 1
## Heading 2
### Heading 3
#### Heading 4
##### Heading 5
###### Heading 6

Paragraph with **strong**, *emphasis*, ~~strike~~, ==mark==, \`inline code\`, [link](https://example.com), and $x^2$.

- list item
- [x] completed task

> quote

> [!NOTE]
> Callout

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
    return root;
};
