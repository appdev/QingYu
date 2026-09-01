// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

package model

import (
	"crypto/sha256"
	"fmt"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestNotebookHomeSyncConflictPreservesOtherVersion(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	home, err := GetNotebookHome(box.ID)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = SaveNotebookHome(box.ID, "selected winner\n", home.Revision, "sync-winner"); err != nil {
		t.Fatal(err)
	}
	other := []byte("other version\n")
	conflictPath := filepath.Join(t.TempDir(), "home.md")
	if err = os.WriteFile(conflictPath, other, 0600); err != nil {
		t.Fatal(err)
	}
	conflictTime := time.Date(2026, 8, 31, 12, 34, 56, 0, time.UTC)
	if err = preserveNotebookHomeSyncConflict(box.ID, conflictPath, conflictTime); err != nil {
		t.Fatal(err)
	}
	hash := fmt.Sprintf("%x", sha256.Sum256(other))[:16]
	recoveryPath := ".qingyu/recovery/sync-20260831T123456Z-" + hash + ".md"
	recovered, err := ReadNotebookInternalFile(box.ID, recoveryPath)
	if err != nil || string(recovered) != string(other) {
		t.Fatalf("unexpected recovery content: %q, %v", recovered, err)
	}
	current, err := GetNotebookHome(box.ID)
	if err != nil || current.Content != "selected winner\n" {
		t.Fatalf("selected winner changed: %#v, %v", current, err)
	}
}

func TestNotebookHomeSyncPathClassification(t *testing.T) {
	boxID := "20260831120000-home001"
	for _, path := range []string{"/" + boxID + "/.qingyu/home.md", boxID + "/.qingyu/home.json"} {
		if actual, ok := notebookHomeStateBoxFromRepoPath(path); !ok || actual != boxID {
			t.Fatalf("notebook home state path was not recognized: %s", path)
		}
	}
	for _, path := range []string{"/" + boxID + "/.qingyu/recovery/a.md", "/invalid/.qingyu/home.md"} {
		if _, ok := notebookHomeStateBoxFromRepoPath(path); ok {
			t.Fatalf("unrelated path was recognized: %s", path)
		}
	}
}
