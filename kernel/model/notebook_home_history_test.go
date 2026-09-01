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

	"github.com/siyuan-note/siyuan/kernel/conf"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func TestNotebookHomeHistoryPreviewAndRollback(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	Conf.Editor = conf.NewEditor()
	Conf.Export = conf.NewExport()
	originalWorkspaceDir, originalHistoryDir := util.WorkspaceDir, util.HistoryDir
	util.WorkspaceDir = filepath.Dir(util.DataDir)
	util.HistoryDir = filepath.Join(util.WorkspaceDir, "history")
	t.Cleanup(func() {
		util.WorkspaceDir, util.HistoryDir = originalWorkspaceDir, originalHistoryDir
	})

	home, err := GetNotebookHome(box.ID)
	if err != nil {
		t.Fatal(err)
	}
	home, err = SaveNotebookHome(box.ID, "当前内容\n", home.Revision, "history-current")
	if err != nil {
		t.Fatal(err)
	}
	historyPath := filepath.Join(util.HistoryDir, "2026-08-31-180000-update", box.ID, ".qingyu", "home.md")
	if err = os.MkdirAll(filepath.Dir(historyPath), 0755); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(historyPath, []byte("历史内容\n"), 0600); err != nil {
		t.Fatal(err)
	}
	relativePath, err := filepath.Rel(util.WorkspaceDir, historyPath)
	if err != nil {
		t.Fatal(err)
	}
	id, rootID, preview, _, err := GetDocHistoryContent(relativePath, "", false)
	if err != nil || id != box.ID || rootID != box.ID || !strings.Contains(preview, "历史内容") {
		t.Fatalf("unexpected history preview [id=%s, root=%s, content=%s, err=%v]", id, rootID, preview, err)
	}
	if err = RollbackDocHistory(relativePath); err != nil {
		t.Fatal(err)
	}
	rolledBack, err := GetNotebookHome(box.ID)
	if err != nil || rolledBack.Content != "历史内容\n" || rolledBack.Revision == home.Revision {
		t.Fatalf("unexpected rolled back home: %#v, %v", rolledBack, err)
	}
}
