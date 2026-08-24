// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

package model

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/siyuan-note/siyuan/kernel/util"
)

func withMarkdownTableAppearanceDataDir(t *testing.T) {
	t.Helper()
	originalDataDir := util.DataDir
	util.DataDir = filepath.Join(t.TempDir(), "data")
	t.Cleanup(func() {
		util.DataDir = originalDataDir
	})
}

func TestMarkdownTableAppearancePersistsMigratesAndRemoves(t *testing.T) {
	withMarkdownTableAppearanceDataDir(t)
	widthMode := "even"
	contentFingerprint := "content"
	contextFingerprint := "context"
	headerFingerprint := "header"
	columnCount := 3
	ordinalHint := 2
	matchedAt := int64(123)

	result, err := PatchMarkdownTableAppearance("workspace:box:/old.md", "table-1", MarkdownTableAppearancePatch{
		ContentFingerprint: &contentFingerprint,
		ContextFingerprint: &contextFingerprint,
		ColumnCount:        &columnCount,
		HeaderFingerprint:  &headerFingerprint,
		OrdinalHint:        &ordinalHint,
		WidthMode:          &widthMode,
		LastMatchedAt:      &matchedAt,
	})
	if err != nil {
		t.Fatal(err)
	}
	if result.DocumentRevision != 1 || result.Record.Attributes.WidthMode != "even" {
		t.Fatalf("unexpected patch result: %+v", result)
	}

	document, err := GetMarkdownTableAppearance("workspace:box:/old.md")
	if err != nil {
		t.Fatal(err)
	}
	if document.Tables["table-1"].Structure.ColumnCount != 3 {
		t.Fatalf("unexpected persisted record: %+v", document.Tables["table-1"])
	}
	if err = MigrateMarkdownTableAppearanceDocument("workspace:box:/old.md", "workspace:new-box:/new.md"); err != nil {
		t.Fatal(err)
	}
	oldDocument, err := GetMarkdownTableAppearance("workspace:box:/old.md")
	if err != nil {
		t.Fatal(err)
	}
	if len(oldDocument.Tables) != 0 {
		t.Fatalf("old appearance key was not removed: %+v", oldDocument)
	}
	newDocument, err := GetMarkdownTableAppearance("workspace:new-box:/new.md")
	if err != nil {
		t.Fatal(err)
	}
	if newDocument.Tables["table-1"].Attributes.WidthMode != "even" {
		t.Fatalf("appearance was not migrated: %+v", newDocument)
	}
	if err = RemoveMarkdownTableAppearanceDocument("workspace:new-box:/new.md"); err != nil {
		t.Fatal(err)
	}
	removed, err := GetMarkdownTableAppearance("workspace:new-box:/new.md")
	if err != nil {
		t.Fatal(err)
	}
	if len(removed.Tables) != 0 {
		t.Fatalf("appearance was not removed: %+v", removed)
	}
}

func TestMarkdownTableAppearancePreservesCorruptStorage(t *testing.T) {
	withMarkdownTableAppearanceDataDir(t)
	storePath := markdownTableAppearancePath()
	if err := os.MkdirAll(filepath.Dir(storePath), 0755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(storePath, []byte("not-json"), 0644); err != nil {
		t.Fatal(err)
	}

	document, err := GetMarkdownTableAppearance("external:capability")
	if err != nil {
		t.Fatal(err)
	}
	if len(document.Tables) != 0 {
		t.Fatalf("corrupt storage should fall back to an empty document: %+v", document)
	}
	entries, err := os.ReadDir(filepath.Dir(storePath))
	if err != nil {
		t.Fatal(err)
	}
	preserved := false
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), filepath.Base(storePath)+".corrupt.") {
			preserved = true
			break
		}
	}
	if !preserved {
		t.Fatal("corrupt storage was not preserved")
	}
}

func TestMarkdownTableAppearanceRejectsInvalidInput(t *testing.T) {
	withMarkdownTableAppearanceDataDir(t)
	invalidWidthMode := "full"
	if _, err := PatchMarkdownTableAppearance("workspace:box:/note.md", "table-1", MarkdownTableAppearancePatch{
		WidthMode: &invalidWidthMode,
	}); err == nil {
		t.Fatal("invalid width mode was accepted")
	}
	if _, err := GetMarkdownTableAppearance("unknown:key"); err == nil {
		t.Fatal("invalid document key was accepted")
	}
}
