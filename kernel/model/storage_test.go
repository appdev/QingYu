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
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/siyuan-note/siyuan/kernel/util"
)

func findRecentMarkdownTest(t *testing.T, ref MarkdownDocumentRef) *RecentDoc {
	t.Helper()
	docs, err := loadRecentDocsRaw()
	if err != nil {
		t.Fatal(err)
	}
	for _, doc := range docs {
		if recentDocKey(doc) == MarkdownRecentKey(ref) {
			return doc
		}
	}
	return nil
}

func TestNormalizeRecentDocsKeepsNativeAndMarkdown(t *testing.T) {
	setupMarkdownTest(t)
	Conf.FileTree.RecentDocsMaxListCount = 32
	docs := normalizeRecentDocs([]*RecentDoc{
		{RootID: "20260820000000-native", ViewedAt: 1},
		{Kind: "markdown", Notebook: "20260811000000-abcdefg", Path: "/a.md", ViewedAt: 2},
	})
	if len(docs) != 2 {
		t.Fatalf("unexpected documents: %#v", docs)
	}
}

func TestRecentDocLegacyJSONRemainsNative(t *testing.T) {
	var doc RecentDoc
	if err := json.Unmarshal([]byte(`{"rootID":"20260820000000-native","viewedAt":1}`), &doc); err != nil {
		t.Fatal(err)
	}
	if doc.RootID != "20260820000000-native" || doc.Kind != "" || recentDocKey(&doc) != "native:20260820000000-native" {
		t.Fatalf("legacy recent document changed identity: %#v", doc)
	}
}

func TestRecentMarkdownMovesWithoutLosingTimes(t *testing.T) {
	box := setupMarkdownTest(t)
	Conf.FileTree.RecentDocsMaxListCount = 32
	from := MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}
	to := MarkdownDocumentRef{Notebook: box.ID, Path: "/folder/a.md"}
	if err := UpdateRecentMarkdownOpenTime(from); err != nil {
		t.Fatal(err)
	}
	before := findRecentMarkdownTest(t, from)
	if before == nil || before.OpenAt == 0 || before.ViewedAt == 0 {
		t.Fatalf("open time was not recorded: %#v", before)
	}
	if err := MoveRecentMarkdown(from, to); err != nil {
		t.Fatal(err)
	}
	after := findRecentMarkdownTest(t, to)
	if after == nil || after.OpenAt != before.OpenAt || after.ViewedAt != before.ViewedAt || findRecentMarkdownTest(t, from) != nil {
		t.Fatalf("recent record was not migrated: before=%#v after=%#v", before, after)
	}
}

func TestRecentMarkdownViewCloseAndRemoveLifecycle(t *testing.T) {
	box := setupMarkdownTest(t)
	Conf.FileTree.RecentDocsMaxListCount = 32
	ref := MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}
	if err := UpdateRecentMarkdownViewTime(ref); err != nil {
		t.Fatal(err)
	}
	doc := findRecentMarkdownTest(t, ref)
	if doc == nil || doc.ViewedAt == 0 || doc.OpenAt != 0 {
		t.Fatalf("unexpected view record: %#v", doc)
	}
	if err := UpdateRecentMarkdownCloseTime(ref); err != nil {
		t.Fatal(err)
	}
	doc = findRecentMarkdownTest(t, ref)
	if doc == nil || doc.ClosedAt == 0 {
		t.Fatalf("close time was not recorded: %#v", doc)
	}
	if err := RemoveRecentMarkdown(ref); err != nil {
		t.Fatal(err)
	}
	if doc = findRecentMarkdownTest(t, ref); doc != nil {
		t.Fatalf("removed recent record survived: %#v", doc)
	}
}

func TestGetRecentDocsPrunesMissingMarkdownAndSortsUpdated(t *testing.T) {
	box := setupMarkdownTest(t)
	Conf.FileTree.RecentDocsMaxListCount = 32
	olderPath := writeMarkdownManagementFixture(t, box.ID, "/older.md", []byte("older"))
	newerPath := writeMarkdownManagementFixture(t, box.ID, "/newer.md", []byte("newer"))
	olderTime, newerTime := time.Unix(10, 0), time.Unix(20, 0)
	if err := os.Chtimes(olderPath, olderTime, olderTime); err != nil {
		t.Fatal(err)
	}
	if err := os.Chtimes(newerPath, newerTime, newerTime); err != nil {
		t.Fatal(err)
	}
	if err := setRecentDocs([]*RecentDoc{
		{Kind: "markdown", Notebook: box.ID, Path: "/missing.md", ViewedAt: 3},
		{Kind: "markdown", Notebook: box.ID, Path: "/older.md", ViewedAt: 2},
		{Kind: "markdown", Notebook: box.ID, Path: "/newer.md", ViewedAt: 1},
	}); err != nil {
		t.Fatal(err)
	}

	docs, err := GetRecentDocs("updated")
	if err != nil {
		t.Fatal(err)
	}
	var markdownDocs []*RecentDoc
	for _, doc := range docs {
		if doc.Kind == "markdown" {
			markdownDocs = append(markdownDocs, doc)
		}
	}
	if len(markdownDocs) != 2 || markdownDocs[0].Path != "/newer.md" || markdownDocs[1].Path != "/older.md" {
		t.Fatalf("unexpected updated Markdown documents: %#v", markdownDocs)
	}
	if markdownDocs[0].Title != "newer.md" || markdownDocs[0].Updated <= markdownDocs[1].Updated {
		t.Fatalf("Markdown metadata was not resolved: %#v", markdownDocs)
	}
	if findRecentMarkdownTest(t, MarkdownDocumentRef{Notebook: box.ID, Path: "/missing.md"}) != nil {
		t.Fatal("missing Markdown recent record was not pruned")
	}
}

func TestNormalizeRecentDocsCapsEachTimestampCategory(t *testing.T) {
	setupMarkdownTest(t)
	Conf.FileTree.RecentDocsMaxListCount = 2
	docs := normalizeRecentDocs([]*RecentDoc{
		{Kind: "markdown", Notebook: "box", Path: "/a.md", ViewedAt: 1, OpenAt: 1, ClosedAt: 1},
		{Kind: "markdown", Notebook: "box", Path: "/b.md", ViewedAt: 2, OpenAt: 2, ClosedAt: 2},
		{Kind: "markdown", Notebook: "box", Path: "/c.md", ViewedAt: 3, OpenAt: 3, ClosedAt: 3},
	})
	if len(docs) != 2 {
		t.Fatalf("timestamp categories were not capped: %#v", docs)
	}
}

func TestRecentMarkdownFileUsesCanonicalPath(t *testing.T) {
	box := setupMarkdownTest(t)
	Conf.FileTree.RecentDocsMaxListCount = 32
	if err := UpdateRecentMarkdownOpenTime(MarkdownDocumentRef{Notebook: box.ID, Path: "notes/a.md"}); err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(filepath.Join(util.DataDir, "storage", "recent-doc.json"))
	if err != nil {
		t.Fatal(err)
	}
	if !json.Valid(data) || findRecentMarkdownTest(t, MarkdownDocumentRef{Notebook: box.ID, Path: "/notes/a.md"}) == nil {
		t.Fatalf("canonical Markdown recent record was not persisted: %s", data)
	}
}
