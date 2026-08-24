// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

package tools

import (
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/siyuan-note/siyuan/kernel/conf"
	"github.com/siyuan-note/siyuan/kernel/model"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func setupMarkdownToolTest(t *testing.T) *model.Box {
	t.Helper()
	originalConf := model.Conf
	originalDataDir := util.DataDir
	util.DataDir = filepath.Join(t.TempDir(), "data")
	model.Conf = model.NewAppConf()
	model.Conf.Sync = conf.NewSync()
	model.Conf.FileTree = conf.NewFileTree()
	model.Conf.NotebookCrypto = conf.NewNotebookCrypto()

	box := &model.Box{ID: "20260815000000-abcdefg"}
	boxConf := conf.NewBoxConf()
	boxConf.Name = "MCP Markdown test"
	boxConf.Closed = false
	if err := box.SaveConf(boxConf); err != nil {
		t.Fatalf("save test notebook conf failed: %v", err)
	}
	t.Cleanup(func() {
		model.Conf = originalConf
		util.DataDir = originalDataDir
	})
	return box
}

func callMarkdownTool(t *testing.T, args map[string]any) CallToolResult {
	t.Helper()
	tool := LookupTool("markdown")
	if tool == nil {
		t.Fatal("markdown MCP tool is not registered")
	}
	result, err := tool.Handler(args)
	if err != nil {
		t.Fatalf("markdown MCP handler failed: %v", err)
	}
	return result
}

func markdownToolText(t *testing.T, result CallToolResult) string {
	t.Helper()
	if len(result.Content) != 1 {
		t.Fatalf("unexpected Markdown tool content: %+v", result.Content)
	}
	return result.Content[0].Text
}

func TestMarkdownToolLifecycle(t *testing.T) {
	box := setupMarkdownToolTest(t)
	created := callMarkdownTool(t, map[string]any{
		"action": "create", "notebook": box.ID, "parentPath": "/", "name": "notes",
	})
	if created.IsError || !strings.Contains(markdownToolText(t, created), "Path: /notes.md") {
		t.Fatalf("unexpected create result: %+v", created)
	}

	document, err := model.GetMarkdown(box.ID, "/notes.md")
	if err != nil {
		t.Fatal(err)
	}
	saved := callMarkdownTool(t, map[string]any{
		"action": "save", "notebook": box.ID, "path": document.Path,
		"content": "# Notes\n", "revision": document.Revision,
	})
	if saved.IsError || !strings.Contains(markdownToolText(t, saved), "# Notes") {
		t.Fatalf("unexpected save result: %+v", saved)
	}

	loaded := callMarkdownTool(t, map[string]any{
		"action": "get", "notebook": box.ID, "path": "/notes.md",
	})
	if loaded.IsError || !strings.Contains(markdownToolText(t, loaded), "# Notes") {
		t.Fatalf("unexpected get result: %+v", loaded)
	}

	renamed := callMarkdownTool(t, map[string]any{
		"action": "rename", "notebook": box.ID, "path": "/notes.md", "name": "renamed.markdown",
	})
	if renamed.IsError || !strings.Contains(markdownToolText(t, renamed), "Path: /renamed.markdown") {
		t.Fatalf("unexpected rename result: %+v", renamed)
	}

	removed := callMarkdownTool(t, map[string]any{
		"action": "remove", "notebook": box.ID, "path": "/renamed.markdown",
	})
	if removed.IsError {
		t.Fatalf("unexpected remove result: %+v", removed)
	}
	if _, err = model.GetMarkdown(box.ID, "/renamed.markdown"); err == nil {
		t.Fatal("removed Markdown file remains readable")
	}
}

func TestMarkdownToolSchema(t *testing.T) {
	tool := LookupTool("markdown")
	if tool == nil {
		t.Fatal("markdown MCP tool is not registered")
	}
	want := []string{"create", "get", "save", "rename", "remove"}
	if got := tool.InputSchema.Properties["action"].Enum; !reflect.DeepEqual(got, want) {
		t.Fatalf("markdown actions = %v, want %v", got, want)
	}
}

func TestMarkdownToolReportsRevisionConflict(t *testing.T) {
	box := setupMarkdownToolTest(t)
	created, err := model.CreateMarkdown(box.ID, "/", "conflict.md")
	if err != nil {
		t.Fatal(err)
	}
	first := callMarkdownTool(t, map[string]any{
		"action": "save", "notebook": box.ID, "path": created.Path,
		"content": "first\n", "revision": created.Revision,
	})
	if first.IsError {
		t.Fatalf("first save failed: %+v", first)
	}
	stale := callMarkdownTool(t, map[string]any{
		"action": "save", "notebook": box.ID, "path": created.Path,
		"content": "stale\n", "revision": created.Revision,
	})
	if !stale.IsError || !strings.Contains(markdownToolText(t, stale), "modified") {
		t.Fatalf("revision conflict was not reported: %+v", stale)
	}
}

func TestMarkdownToolRequiresActionArguments(t *testing.T) {
	setupMarkdownToolTest(t)
	for _, args := range []map[string]any{
		{"action": "create"},
		{"action": "get"},
		{"action": "save"},
		{"action": "rename"},
		{"action": "remove"},
	} {
		result := callMarkdownTool(t, args)
		if !result.IsError {
			t.Fatalf("missing arguments accepted for %q: %+v", args["action"], result)
		}
	}
}

func TestMarkdownToolAllowsEmptyContent(t *testing.T) {
	box := setupMarkdownToolTest(t)
	created, err := model.CreateMarkdown(box.ID, "/", "empty.md")
	if err != nil {
		t.Fatal(err)
	}
	missing := callMarkdownTool(t, map[string]any{
		"action": "save", "notebook": box.ID, "path": created.Path, "revision": created.Revision,
	})
	if !missing.IsError || !strings.Contains(markdownToolText(t, missing), "content is required") {
		t.Fatalf("missing content was not rejected: %+v", missing)
	}
	empty := callMarkdownTool(t, map[string]any{
		"action": "save", "notebook": box.ID, "path": created.Path,
		"content": "", "revision": created.Revision,
	})
	if empty.IsError {
		t.Fatalf("empty content was rejected: %+v", empty)
	}
}
