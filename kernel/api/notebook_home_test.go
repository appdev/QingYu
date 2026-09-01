// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

package api

import (
	"net/http"
	"testing"

	"github.com/siyuan-note/siyuan/kernel/model"
)

func TestNotebookHomeAPIContract(t *testing.T) {
	boxID := setupFileTreeAPIBox(t)
	missing := postMarkdownHandler(t, getNotebookHome, `{"notebook":"`+boxID+`"}`)
	missingData, _ := missing["data"].(map[string]any)
	if missing["code"] != float64(0) || missingData["exists"] != false {
		t.Fatalf("unexpected missing home response: %#v", missing)
	}
	revision, _ := missingData["revision"].(string)

	saved := postMarkdownHandler(t, saveNotebookHome,
		`{"notebook":"`+boxID+`","content":"# Home\n","revision":"`+revision+`","operationID":"home-save"}`)
	savedData, _ := saved["data"].(map[string]any)
	if saved["code"] != float64(0) || savedData["content"] != "# Home\n" || savedData["operationID"] != "home-save" {
		t.Fatalf("unexpected save response: %#v", saved)
	}

	conflict := postMarkdownHandler(t, saveNotebookHome,
		`{"notebook":"`+boxID+`","content":"stale","revision":"`+revision+`"}`)
	if conflict["code"] != float64(http.StatusConflict) {
		t.Fatalf("stale save did not return conflict: %#v", conflict)
	}

	invalid := postMarkdownHandler(t, getNotebookHome, `{"notebook":"invalid"}`)
	if invalid["code"] == float64(0) {
		t.Fatalf("invalid notebook ID accepted: %#v", invalid)
	}

	readBack, err := model.GetNotebookHome(boxID)
	if err != nil || readBack.Content != "# Home\n" {
		t.Fatalf("unexpected stored home: %#v, %v", readBack, err)
	}
}
