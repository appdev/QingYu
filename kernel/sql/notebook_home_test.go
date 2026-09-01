// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

package sql

import (
	stdsql "database/sql"
	"strings"
	"testing"
)

func TestNotebookHomeSearchIndex(t *testing.T) {
	originalDB, originalEncryptedFn := db, IsEncryptedBoxFn
	testDB, err := stdsql.Open("sqlite3_extended", ":memory:")
	if err != nil {
		t.Fatal(err)
	}
	db = testDB
	IsEncryptedBoxFn = func(string) bool { return false }
	t.Cleanup(func() {
		db = originalDB
		IsEncryptedBoxFn = originalEncryptedFn
		testDB.Close()
	})
	if err = initNotebookHomeTables(testDB); err != nil {
		if strings.Contains(err.Error(), "no such module: fts5") {
			t.Skip("test SQLite driver was built without FTS5")
		}
		t.Fatal(err)
	}
	boxID := "20260831120000-home001"
	if err = UpsertNotebookHome(boxID, "轻语", "这里可以搜索首页内容", 1); err != nil {
		t.Fatal(err)
	}
	rows, err := SearchNotebookHomesInBox(boxID, "搜索", 0)
	if err != nil {
		t.Fatal(err)
	}
	if len(rows) != 1 || rows[0].Title != "轻语" {
		t.Fatalf("unexpected rows: %#v", rows)
	}
	if err = UpsertNotebookHome(boxID, "轻语新名", "更新后的首页", 2); err != nil {
		t.Fatal(err)
	}
	rows, err = SearchNotebookHomesInBox(boxID, "搜索", 0)
	if err != nil || len(rows) != 0 {
		t.Fatalf("old FTS content survived update: %#v, %v", rows, err)
	}
	rows, err = SearchNotebookHomesInBox(boxID, "更新", 0)
	if err != nil || len(rows) != 1 || rows[0].Title != "轻语新名" {
		t.Fatalf("updated row is not searchable: %#v, %v", rows, err)
	}
	if err = DeleteNotebookHome(boxID); err != nil {
		t.Fatal(err)
	}
	rows, err = SearchNotebookHomesInBox(boxID, "搜索", 0)
	if err != nil {
		t.Fatal(err)
	}
	if len(rows) != 0 {
		t.Fatalf("deleted home is still searchable: %#v", rows)
	}
}
