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

package model

import (
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/siyuan-note/dejavu"
	"github.com/siyuan-note/eventbus"
	"github.com/siyuan-note/siyuan/kernel/conf"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func setupMarkdownTest(t *testing.T) *Box {
	t.Helper()
	originalConf := Conf
	originalDataDir := util.DataDir
	util.DataDir = filepath.Join(t.TempDir(), "data")
	Conf = NewAppConf()
	Conf.Sync = conf.NewSync()
	Conf.FileTree = conf.NewFileTree()
	Conf.NotebookCrypto = conf.NewNotebookCrypto()

	box := &Box{ID: "20260811000000-abcdefg"}
	boxConf := conf.NewBoxConf()
	boxConf.Name = "Markdown test"
	boxConf.Closed = false
	if err := box.SaveConf(boxConf); err != nil {
		t.Fatalf("save test notebook conf failed: %v", err)
	}
	t.Cleanup(func() {
		Conf = originalConf
		util.DataDir = originalDataDir
	})
	return box
}

func TestMarkdownFileLifecycle(t *testing.T) {
	box := setupMarkdownTest(t)

	created, err := CreateMarkdown(box.ID, "/", "notes")
	if err != nil {
		t.Fatal(err)
	}
	if created.Path != "/notes.md" || created.Content != "" {
		t.Fatalf("unexpected created document: %+v", created)
	}

	saved, err := SaveMarkdown(box.ID, created.Path, "# Notes\n", created.Revision)
	if err != nil {
		t.Fatal(err)
	}
	if saved.Content != "# Notes\n" || saved.Revision == created.Revision {
		t.Fatalf("unexpected saved document: %+v", saved)
	}
	if _, err = SaveMarkdown(box.ID, created.Path, "stale", created.Revision); !errors.Is(err, ErrMarkdownConflict) {
		t.Fatalf("expected revision conflict, got %v", err)
	}

	renamed, err := RenameMarkdown(box.ID, created.Path, "renamed.markdown")
	if err != nil {
		t.Fatal(err)
	}
	if renamed.Path != "/renamed.markdown" || renamed.Content != "# Notes\n" {
		t.Fatalf("unexpected renamed document: %+v", renamed)
	}
	if err = RemoveMarkdown(box.ID, renamed.Path); err != nil {
		t.Fatal(err)
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "renamed.markdown")); !os.IsNotExist(err) {
		t.Fatalf("Markdown file should be removed, got %v", err)
	}
}

func TestMarkdownFilesIncludedInSyncSnapshots(t *testing.T) {
	originalConf, originalLangs := Conf, util.Langs
	Conf = NewAppConf()
	Conf.Lang = "en"
	util.Langs = map[string]map[int]string{"en": {158: "Indexing %s", 159: "Reading %d/%d", 160: "Writing %d/%d"}}
	t.Cleanup(func() {
		Conf, util.Langs = originalConf, originalLangs
	})

	root := t.TempDir()
	dataDir := filepath.Join(root, "data")
	markdownPath := filepath.Join(dataDir, "20260811000000-abcdefg", "sync.md")
	assetPath := filepath.Join(dataDir, "assets", "sync.png")
	for _, dir := range []string{filepath.Dir(markdownPath), filepath.Dir(assetPath)} {
		if err := os.MkdirAll(dir, 0755); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(markdownPath, []byte("![image](assets/sync.png)\n"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(assetPath, []byte("image"), 0644); err != nil {
		t.Fatal(err)
	}

	repo, err := dejavu.NewRepo(
		dataDir,
		filepath.Join(root, "repo"),
		filepath.Join(root, "history"),
		filepath.Join(root, "temp"),
		"device-id",
		"device-name",
		"darwin",
		bytes.Repeat([]byte{1}, 32),
		nil,
		nil,
	)
	if err != nil {
		t.Fatal(err)
	}
	indexContext := map[string]any{eventbus.CtxPushMsg: eventbus.CtxPushMsgToNone}
	index, err := repo.Index("Markdown sync test", false, indexContext)
	if err != nil {
		t.Fatal(err)
	}
	files, err := repo.GetFiles(index)
	if err != nil {
		t.Fatal(err)
	}
	paths := map[string]bool{}
	for _, file := range files {
		paths[file.Path] = true
	}
	if !paths["/20260811000000-abcdefg/sync.md"] || !paths["/assets/sync.png"] {
		t.Fatalf("Markdown and its asset must be included in the sync snapshot: %v", paths)
	}

	if err = os.WriteFile(markdownPath, []byte("updated\n"), 0644); err != nil {
		t.Fatal(err)
	}
	updatedTime := time.Now().Add(2 * time.Second)
	if err = os.Chtimes(markdownPath, updatedTime, updatedTime); err != nil {
		t.Fatal(err)
	}
	if err = os.Remove(assetPath); err != nil {
		t.Fatal(err)
	}
	index, err = repo.Index("Markdown sync update", false, indexContext)
	if err != nil {
		t.Fatal(err)
	}
	files, err = repo.GetFiles(index)
	if err != nil {
		t.Fatal(err)
	}
	paths = map[string]bool{}
	var syncedMarkdown []byte
	for _, file := range files {
		paths[file.Path] = true
		if file.Path == "/20260811000000-abcdefg/sync.md" {
			syncedMarkdown, err = repo.OpenFile(file)
			if err != nil {
				t.Fatal(err)
			}
		}
	}
	if paths["/assets/sync.png"] {
		t.Fatalf("deleted asset must be removed from the next sync snapshot: %v", paths)
	}
	if string(syncedMarkdown) != "updated\n" {
		t.Fatalf("updated Markdown content missing from sync snapshot: %q", syncedMarkdown)
	}
}

func TestMarkdownRejectsUnsafePathAndEncryptedNotebook(t *testing.T) {
	box := setupMarkdownTest(t)

	if _, err := GetMarkdown(box.ID, "../outside.md"); err == nil {
		t.Fatal("unsafe Markdown path should be rejected")
	}
	boxConf := box.GetConf()
	boxConf.Encrypted = true
	if err := box.SaveConf(boxConf); err != nil {
		t.Fatal(err)
	}
	if _, err := CreateMarkdown(box.ID, "/", "encrypted.md"); !errors.Is(err, ErrMarkdownEncryptedNotebook) {
		t.Fatalf("expected encrypted notebook rejection, got %v", err)
	}
}

func TestMarkdownAutoName(t *testing.T) {
	box := setupMarkdownTest(t)

	first, err := CreateMarkdown(box.ID, "/", "Untitled.md", true)
	if err != nil {
		t.Fatal(err)
	}
	second, err := CreateMarkdown(box.ID, "/", "Untitled.md", true)
	if err != nil {
		t.Fatal(err)
	}
	if first.Path != "/Untitled.md" || second.Path != "/Untitled 2.md" {
		t.Fatalf("unexpected automatic Markdown names: %q, %q", first.Path, second.Path)
	}
}

func TestMarkdownWYSIWYGRoundTrip(t *testing.T) {
	markdown := "# 标题\n\n开头 **粗体** 和 *斜体*。\n\n> 引用\n\n- [x] 任务\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n\n```ts\nconst value = 42;\n```\n\n![图片](https://example.com/image.png)\n\n结尾。\n"
	html := MarkdownToProtylePreviewHTML(markdown)
	html = strings.Replace(html, "开头", "修改后的开头", 1)
	html = strings.Replace(html, "结尾。", "修改后的结尾。", 1)
	html = strings.Replace(html, `<code class="hljs">`, `<code class="language-ts">`, 1)
	html = strings.Replace(html, `<img src=`, `<img width="320px" src=`, 1)
	converted, err := util.NewLute().HTML2Markdown(html)
	if err != nil {
		t.Fatal(err)
	}
	for _, expected := range []string{
		"# 标题", "修改后的开头", "**粗体**", "*斜体*", "> 引用", "[X] 任务",
		"|A|B|", "```ts", "const value = 42;", "![图片](https://example.com/image.png)",
		`{: style="width: 320px;"}`, "修改后的结尾。",
	} {
		if !strings.Contains(converted, expected) {
			t.Fatalf("converted Markdown should contain %q:\n%s", expected, converted)
		}
	}
	rendered := MarkdownToProtylePreviewHTML(converted)
	if !strings.Contains(rendered, `style="width: 320px;"`) {
		t.Fatalf("rendered image should preserve its width:\n%s", rendered)
	}
}
