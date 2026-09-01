// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package api

import (
	"testing"

	"github.com/siyuan-note/siyuan/kernel/model"
)

func TestListNotebookRootDocumentsRequiresNotebookAndReturnsDirectDocuments(t *testing.T) {
	assertMarkdownHandlerRejects(t, listNotebookRootDocuments, `{}`)
	boxID := setupFileTreeAPIBox(t)
	if _, err := model.CreateMarkdown(boxID, "/", "root.md"); err != nil {
		t.Fatal(err)
	}
	ret := postMarkdownHandler(t, listNotebookRootDocuments, `{"notebook":"`+boxID+`"}`)
	data, _ := ret["data"].(map[string]any)
	documents, _ := data["documents"].([]any)
	if ret["code"] != float64(0) || len(documents) != 1 {
		t.Fatalf("unexpected root listing response: %#v", ret)
	}
}
