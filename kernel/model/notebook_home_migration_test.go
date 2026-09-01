// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

package model

import (
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/88250/lute/ast"
	"github.com/88250/lute/parse"
	"github.com/siyuan-note/siyuan/kernel/conf"
	"github.com/siyuan-note/siyuan/kernel/filesys"
	"github.com/siyuan-note/siyuan/kernel/treenode"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func TestNotebookHomeMigrationEffectiveContent(t *testing.T) {
	tests := []struct {
		name      string
		markdown  string
		effective bool
	}{
		{name: "missing body", markdown: "", effective: false},
		{name: "whitespace", markdown: "   \n", effective: false},
		{name: "text", markdown: "正文", effective: true},
		{name: "image", markdown: "![](assets/a.png)", effective: true},
		{name: "empty code block", markdown: "```\n```", effective: true},
		{name: "table", markdown: "| A |\n| - |\n| B |", effective: true},
		{name: "block reference", markdown: "((20260831120000-home001))", effective: true},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			engine := util.NewLute()
			tree := parse.Parse("", []byte(test.markdown), engine.ParseOptions)
			if actual := notebookHomeTreeHasEffectiveContent(tree); actual != test.effective {
				t.Fatalf("effective=%v, want %v", actual, test.effective)
			}
		})
	}
}

func TestNotebookHomeMigrationPreservesLegacySource(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	Conf.Editor = conf.NewEditor()
	Conf.Export = conf.NewExport()
	tree := treenode.NewTree(box.ID, boxDocPath(box.ID), "/旧首页", "旧首页")
	tree.Root.SetIALAttr(DocHiddenAttr, "true")
	tree.Root.FirstChild.AppendChild(&ast.Node{Type: ast.NodeText, Tokens: []byte("旧首页正文")})
	_, err := filesys.WriteTree(tree)
	if err != nil {
		t.Fatal(err)
	}
	legacyPath := filepath.Join(util.DataDir, box.ID, box.ID+".sy")
	legacyBefore, err := os.ReadFile(legacyPath)
	if err != nil {
		t.Fatal(err)
	}

	result, err := MigrateNotebookHome(box.ID)
	if err != nil {
		t.Fatal(err)
	}
	if result.State != "migrated" {
		t.Fatalf("unexpected migration result: %#v", result)
	}
	home, err := GetNotebookHome(box.ID)
	if err != nil || !home.Exists || !bytes.Contains([]byte(home.Content), []byte("旧首页正文")) {
		t.Fatalf("unexpected migrated home: %#v, %v", home, err)
	}
	legacyAfter, err := os.ReadFile(legacyPath)
	if err != nil || !bytes.Equal(legacyBefore, legacyAfter) {
		t.Fatalf("legacy source changed: %v", err)
	}

	if err = os.Remove(filepath.Join(util.DataDir, box.ID, ".qingyu", "home.json")); err != nil {
		t.Fatal(err)
	}
	result, err = MigrateNotebookHome(box.ID)
	if err != nil || result.State != "migrated" || result.RecoveryPath != "" {
		t.Fatalf("idempotent migration produced a conflict: %#v, %v", result, err)
	}
}

func TestNotebookHomeEmptyMigrationDoesNotCreateHome(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	Conf.Editor = conf.NewEditor()
	Conf.Export = conf.NewExport()
	tree := treenode.NewTree(box.ID, boxDocPath(box.ID), "/旧首页", "旧首页")
	tree.Root.SetIALAttr(DocHiddenAttr, "true")
	if _, err := filesys.WriteTree(tree); err != nil {
		t.Fatal(err)
	}
	result, err := MigrateNotebookHome(box.ID)
	if err != nil || result.State != "empty" {
		t.Fatalf("unexpected empty migration: %#v, %v", result, err)
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, notebookHomePath)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("empty migration created a home: %v", err)
	}
}

func TestNotebookHomeMissingLegacyCreatesNoCompatibilityFiles(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	result, err := MigrateNotebookHome(box.ID)
	if err != nil || result.State != "" {
		t.Fatalf("unexpected missing-source migration: %#v, %v", result, err)
	}
	for _, relativePath := range []string{box.ID + ".sy", ".siyuan/boxDoc.json", ".qingyu/home.md", ".qingyu/home.json"} {
		if _, statErr := os.Stat(filepath.Join(util.DataDir, box.ID, filepath.FromSlash(relativePath))); !errors.Is(statErr, os.ErrNotExist) {
			t.Fatalf("missing source created %s: %v", relativePath, statErr)
		}
	}
}
