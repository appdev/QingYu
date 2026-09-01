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
	"testing"

	"github.com/siyuan-note/siyuan/kernel/util"
)

func TestNotebookHomeExport(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	originalWorkspaceDir := util.WorkspaceDir
	resolvedDataDir, err := filepath.EvalSymlinks(util.DataDir)
	if err != nil {
		t.Fatal(err)
	}
	util.DataDir = resolvedDataDir
	util.WorkspaceDir = filepath.Dir(resolvedDataDir)
	t.Cleanup(func() { util.WorkspaceDir = originalWorkspaceDir })
	home, err := GetNotebookHome(box.ID)
	if err != nil {
		t.Fatal(err)
	}
	content := "首页正文\n![图片](assets/home.png?box=" + box.ID + ")\n"
	assetPath := filepath.Join(util.DataDir, box.ID, "assets", "home.png")
	if err = os.MkdirAll(filepath.Dir(assetPath), 0755); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(assetPath, []byte("image"), 0600); err != nil {
		t.Fatal(err)
	}
	if resolved, resolveErr := GetAssetAbsPathInBox("assets/home.png", box.ID); resolveErr != nil || resolved != assetPath {
		t.Fatalf("asset did not resolve in notebook [path=%s, err=%v]", resolved, resolveErr)
	}
	if _, err = SaveNotebookHome(box.ID, content, home.Revision, "export-home"); err != nil {
		t.Fatal(err)
	}

	exportDir := t.TempDir()
	if err = exportNotebookHomeMarkdown(box.ID, exportDir); err != nil {
		t.Fatal(err)
	}
	assertFileContent(t, filepath.Join(exportDir, "README.md"), content)
	assertFileContent(t, filepath.Join(exportDir, "assets", "home.png"), "image")

	conflictDir := t.TempDir()
	if err = os.WriteFile(filepath.Join(conflictDir, "readme.MD"), []byte("ordinary document"), 0644); err != nil {
		t.Fatal(err)
	}
	if err = exportNotebookHomeMarkdown(box.ID, conflictDir); err != nil {
		t.Fatal(err)
	}
	assertFileContent(t, filepath.Join(conflictDir, "readme.MD"), "ordinary document")
	assertFileContent(t, filepath.Join(conflictDir, "笔记本首页.md"), content)
	if _, err = os.Stat(filepath.Join(conflictDir, ".qingyu")); !os.IsNotExist(err) {
		t.Fatalf("ordinary Markdown export leaked internal data: %v", err)
	}
}

func assertFileContent(t *testing.T, path, expected string) {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != expected {
		t.Fatalf("unexpected %s content: %q", path, data)
	}
}
