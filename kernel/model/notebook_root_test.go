// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package model

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/siyuan-note/siyuan/kernel/util"
)

func TestListNotebookRootDocuments(t *testing.T) {
	box := setupMarkdownTest(t)
	first, err := CreateMarkdown(box.ID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	second, err := CreateMarkdown(box.ID, "/", "b.md")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = CreateMarkdown(box.ID, "/nested", "child.md"); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(util.DataDir, box.ID, "b.md"), []byte(first.Content), 0644); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(util.DataDir, box.ID, "legacy.md"), []byte("---\ntitle: Legacy title\n---\n# Legacy title\nBody **preview**\n"), 0644); err != nil {
		t.Fatal(err)
	}

	listing, err := ListNotebookRootDocuments(box.ID)
	if err != nil {
		t.Fatal(err)
	}
	if listing.Notebook != box.ID || listing.SortMode != EffectiveFileTreeSortMode(Conf.Box(box.ID), util.SortModeUnassigned) {
		t.Fatalf("unexpected listing metadata: %+v", listing)
	}
	if len(listing.Documents) != 3 {
		t.Fatalf("expected only three direct-root documents, got %+v", listing.Documents)
	}
	byPath := map[string]*NotebookRootDocument{}
	for _, document := range listing.Documents {
		byPath[document.Path] = document
		if document.Path == "/nested/child.md" {
			t.Fatal("nested document leaked into root listing")
		}
	}
	if byPath[first.Path].IdentityConflict {
		t.Fatal("lexicographically first duplicate must keep the ID")
	}
	if !byPath[second.Path].IdentityConflict {
		t.Fatal("later duplicate was not nominated for a new ID")
	}
	legacy := byPath["/legacy.md"]
	if legacy == nil || legacy.IdentityState != "missing" || legacy.Title != "Legacy title" || legacy.DocumentID == "" || legacy.Revision == "" {
		t.Fatalf("unexpected legacy document: %+v", legacy)
	}
	if legacy.PreviewText != "Body preview" {
		t.Fatalf("unexpected legacy preview text: %q", legacy.PreviewText)
	}
}

func TestNotebookRootPreviewText(t *testing.T) {
	markdown := []byte("---\ntitle: 标题\nsecret: 不应展示\n---\n# 标题\n第一段 **正文**\n")
	if preview := markdownNotebookRootPreviewText(markdown, "标题"); preview != "第一段 正文" {
		t.Fatalf("unexpected Markdown preview: %q", preview)
	}
	if preview := markdownNotebookRootPreviewText([]byte("# 标题\n"), "标题"); preview != "" {
		t.Fatalf("title-only document has preview: %q", preview)
	}
	if preview := notebookRootPreviewText("正文\n\t 含有\x00 空白", "标题"); preview != "正文 含有 空白" {
		t.Fatalf("whitespace was not normalized: %q", preview)
	}
	preview := notebookRootPreviewText(strings.Repeat("轻", notebookRootPreviewTextLimit+8), "标题")
	if len([]rune(preview)) != notebookRootPreviewTextLimit {
		t.Fatalf("preview was not truncated by rune: %d", len([]rune(preview)))
	}
}
