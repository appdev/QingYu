// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package model

import (
	"strings"
	"testing"

	"github.com/siyuan-note/siyuan/kernel/conf"
)

func TestMigrateLegacyNotebookRootContent(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	Conf.Editor = conf.NewEditor()
	Conf.Export = conf.NewExport()
	home, err := GetNotebookHome(box.ID)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = SaveNotebookHome(box.ID, "# 旧首页\n\n保留这段内容。\n", home.Revision, "legacy-home"); err != nil {
		t.Fatal(err)
	}

	result, err := MigrateLegacyNotebookRootContent(box.ID)
	if err != nil {
		t.Fatal(err)
	}
	if result.State != "migrated" || len(result.Targets) != 1 {
		t.Fatalf("unexpected migration result: %#v", result)
	}
	document, err := GetMarkdown(box.ID, result.Targets[0])
	if err != nil {
		t.Fatal(err)
	}
	if document.DocumentID == "" || !strings.Contains(document.Content, "保留这段内容") {
		t.Fatalf("migrated document is incomplete: %#v", document)
	}

	again, err := MigrateLegacyNotebookRootContent(box.ID)
	if err != nil || len(again.Targets) != 1 || again.Targets[0] != result.Targets[0] {
		t.Fatalf("migration is not idempotent: %#v, %v", again, err)
	}
}

func TestMigrateLegacyNotebookRootContentSkipsEmptySources(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	result, err := MigrateLegacyNotebookRootContent(box.ID)
	if err != nil {
		t.Fatal(err)
	}
	if result.State != "empty" || len(result.Targets) != 0 {
		t.Fatalf("empty source created a visible document: %#v", result)
	}
}
