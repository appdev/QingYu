import {
  createDefaultCustomMarkdownTemplate,
  createMarkdownTemplateFromEntry,
  createCustomMarkdownTemplateFromFile,
  defaultMarkdownTemplates,
  loadMarkdownTemplatesFromEntries,
  markdownTemplateEntryFromTemplate,
  markdownTemplateInitialDocumentName,
  markdownTemplateToSource,
  mergeMarkdownTemplates,
  normalizeMarkdownTemplateEntries,
  renderMarkdownTemplate,
  updateMarkdownTemplateFromSource
} from "./templates";

describe("markdown templates", () => {
  const now = new Date(2026, 4, 21, 9, 30);

  it("renders built-in templates without generated document-title headings", () => {
    expect(defaultMarkdownTemplates.map((template) => [
      template.id,
      renderMarkdownTemplate(template, { now })
    ])).toEqual([
      ["daily-note", `Date: 2026-05-21

# Notes

# Tasks

- [ ]
`],
      ["meeting-note", `Date: 2026-05-21

# Attendees

# Notes

# Decisions

# Follow up

- [ ]
`],
      ["reading-note", `Date: 2026-05-21

# Source

# Summary

# Highlights

# Questions
`],
      ["project-note", `Date: 2026-05-21

# Goal

# Context

# Plan

- [ ]

# Notes
`]
    ]);
  });

  it("uses a local calendar name only for the stable daily-note id", () => {
    expect(markdownTemplateInitialDocumentName("daily-note", "en", now)).toBe("2026-05-21.md");
    expect(markdownTemplateInitialDocumentName("meeting-note", "en", now)).toBe("Untitled.md");
    expect(markdownTemplateInitialDocumentName("reading-note", "zh-CN", now)).toBe("未命名.md");
    expect(markdownTemplateInitialDocumentName("project-note", "en", now)).toBe("Untitled.md");
    expect(markdownTemplateInitialDocumentName("custom-template", "zh-CN", now)).toBe("未命名.md");
  });

  it("substitutes supported date variables while leaving title literal", () => {
    const template = {
      id: "custom",
      name: "Custom",
      content: "# User heading\n\n{{title}}\n\n{{date}} | {{datetime}} | {{weekday}}"
    };

    expect(renderMarkdownTemplate(template, { now })).toBe(
      "# User heading\n\n{{title}}\n\n2026-05-21 | 2026-05-21 09:30 | Thursday"
    );
  });

  it("normalizes the persisted template list without markdown contents", () => {
    expect(normalizeMarkdownTemplateEntries([
      {
        content: "# Content belongs in the template file",
        fileName: " weekly-review.md ",
        id: " custom-template ",
        name: " Weekly review "
      },
      {
        fileName: "../unsafe.md",
        id: "",
        name: ""
      },
      "invalid"
    ])).toEqual([
      {
        fileName: "weekly-review.md",
        id: "custom-template",
        name: "Weekly review"
      }
    ]);
  });

  it("loads readable template files including empty content", async () => {
    const readTemplateFile = vi.fn(async (fileName: string) => {
      if (fileName === "standup.md") return "# Standup\n\n## Yesterday";
      if (fileName === "empty.md") return "";
      throw new Error("missing template file");
    });

    await expect(loadMarkdownTemplatesFromEntries([
      {
        fileName: "standup.md",
        id: "standup",
        name: "Standup"
      },
      {
        fileName: "empty.md",
        id: "empty",
        name: "Empty"
      },
      {
        fileName: "missing.md",
        id: "missing",
        name: "Missing"
      }
    ], readTemplateFile)).resolves.toEqual([
      {
        content: "# Standup\n\n## Yesterday",
        fileName: "standup.md",
        id: "standup",
        name: "Standup"
      },
      {
        content: "",
        fileName: "empty.md",
        id: "empty",
        name: "Empty"
      }
    ]);
  });

  it("creates a runtime template from settings metadata and the unchanged markdown body", () => {
    expect(createMarkdownTemplateFromEntry({
      fileName: "standup.md",
      id: "standup",
      name: "Standup"
    }, "# User title\n\n###### Detail")).toEqual({
      content: "# User title\n\n###### Detail",
      fileName: "standup.md",
      id: "standup",
      name: "Standup"
    });
  });

  it("serializes custom templates with name as their only source metadata", () => {
    expect(markdownTemplateToSource({
      id: "weekly-review",
      name: "Weekly review",
      content: "# User title\n\n## Wins"
    })).toBe(`---
name: Weekly review
---

# User title

## Wins`);
  });

  it("updates custom template names without treating body headings as document titles", () => {
    expect(updateMarkdownTemplateFromSource({
      id: "standup",
      name: "Standup",
      content: "# Existing user heading\n\n## Yesterday"
    }, `---
name: Daily review
---

# User heading

- [ ] Ship it`)).toEqual({
      id: "standup",
      name: "Daily review",
      content: "# User heading\n\n- [ ] Ship it"
    });
  });

  it("keeps the existing template name when markdown source has no name metadata", () => {
    expect(updateMarkdownTemplateFromSource({
      id: "standup",
      name: "Standup",
      content: "# Existing user heading"
    }, "# User heading\n\nStill a template")).toEqual({
      id: "standup",
      name: "Standup",
      content: "# User heading\n\nStill a template"
    });
  });

  it("creates a new custom template with an empty body", () => {
    expect(createDefaultCustomMarkdownTemplate([])).toEqual({
      content: "",
      fileName: "custom-template.md",
      id: "custom-template",
      name: "New template"
    });
  });

  it("creates a custom template from a markdown file without changing its body headings", () => {
    expect(createCustomMarkdownTemplateFromFile({
      content: "# Standup\n\n###### Yesterday",
      name: "standup.md"
    }, [
      {
        id: "custom-template",
        name: "Existing",
        content: "# Existing"
      }
    ])).toEqual({
      fileName: "custom-template-2.md",
      id: "custom-template-2",
      name: "standup",
      content: "# Standup\n\n###### Yesterday"
    });
  });

  it("creates a settings list entry from a runtime template without content", () => {
    expect(markdownTemplateEntryFromTemplate({
      content: "# Standup",
      fileName: "standup.md",
      id: "standup",
      name: "Standup"
    })).toEqual({
      fileName: "standup.md",
      id: "standup",
      name: "Standup"
    });
  });

  it("merges custom templates over built-in templates without duplicating overridden ids", () => {
    expect(mergeMarkdownTemplates([
      {
        id: "daily-note",
        name: "Daily note edited",
        content: "# Edited daily"
      },
      {
        id: "custom-template",
        name: "Standup",
        content: "# Standup"
      }
    ]).map((template) => template.id)).toEqual([
      "daily-note",
      "meeting-note",
      "reading-note",
      "project-note",
      "custom-template"
    ]);
    expect(mergeMarkdownTemplates([
      {
        id: "daily-note",
        name: "Daily note edited",
        content: "# Edited daily"
      }
    ])[0]).toEqual({
      id: "daily-note",
      name: "Daily note edited",
      content: "# Edited daily"
    });
  });
});
