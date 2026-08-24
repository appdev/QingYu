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

package api

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"testing"

	"github.com/gin-gonic/gin"
	"github.com/siyuan-note/siyuan/kernel/model"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func postMarkdownHandler(t *testing.T, handler gin.HandlerFunc, body string) map[string]any {
	t.Helper()
	gin.SetMode(gin.TestMode)
	recorder := httptest.NewRecorder()
	ctx, _ := gin.CreateTestContext(recorder)
	ctx.Set(model.RoleContextKey, model.RoleAdministrator)
	ctx.Request = httptest.NewRequest(http.MethodPost, "/", bytes.NewBufferString(body))
	ctx.Request.Header.Set("Content-Type", "application/json")
	handler(ctx)
	var ret map[string]any
	if err := json.Unmarshal(recorder.Body.Bytes(), &ret); err != nil {
		t.Fatalf("decode response %q: %v", recorder.Body.String(), err)
	}
	return ret
}

func assertMarkdownHandlerRejects(t *testing.T, handler gin.HandlerFunc, body string) {
	t.Helper()
	ret := postMarkdownHandler(t, handler, body)
	if code, _ := ret["code"].(float64); code == 0 {
		t.Fatalf("request %s was accepted: %#v", body, ret)
	}
}

func TestRemoveMarkdownRequiresRevision(t *testing.T) {
	assertMarkdownHandlerRejects(t, removeMarkdown,
		`{"notebook":"20260811000000-abcdefg","path":"/a.md"}`)
}

func TestRenameMarkdownRequiresAndChecksRevision(t *testing.T) {
	boxID := setupFileTreeAPIBox(t)
	created, err := model.CreateMarkdown(boxID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	ret := postMarkdownHandler(t, renameMarkdown,
		`{"notebook":"`+boxID+`","path":"/a.md","name":"b.md"}`)
	if code, _ := ret["code"].(float64); code == 0 {
		t.Fatalf("rename without revision was accepted: %#v", ret)
	}
	ret = postMarkdownHandler(t, renameMarkdown,
		`{"notebook":"`+boxID+`","path":"/a.md","name":"b.md","revision":"stale"}`)
	if code, _ := ret["code"].(float64); code != http.StatusConflict {
		t.Fatalf("stale rename did not return conflict: %#v", ret)
	}
	ret = postMarkdownHandler(t, renameMarkdown,
		`{"notebook":"`+boxID+`","path":"/a.md","name":"b.md","revision":"`+created.Revision+`"}`)
	if code, _ := ret["code"].(float64); code != 0 {
		t.Fatalf("matching rename was rejected: %#v", ret)
	}
}

func TestRenameMarkdownEchoesClientOperationID(t *testing.T) {
	boxID := setupFileTreeAPIBox(t)
	created, err := model.CreateMarkdown(boxID, "/", "operation.md")
	if err != nil {
		t.Fatal(err)
	}
	ret := postMarkdownHandler(t, renameMarkdown,
		`{"notebook":"`+boxID+`","path":"/operation.md","name":"renamed.md","revision":"`+created.Revision+`","operationID":"client-op"}`)
	data, _ := ret["data"].(map[string]any)
	if ret["code"] != float64(0) || data["operationID"] != "client-op" {
		t.Fatalf("operation ID was not echoed: %#v", ret)
	}
}

func TestMarkdownMutationOperationIDs(t *testing.T) {
	boxID := setupFileTreeAPIBox(t)
	createdRet := postMarkdownHandler(t, createMarkdown,
		`{"notebook":"`+boxID+`","parentPath":"/","name":"ops.md","operationID":"create-client"}`)
	created, _ := createdRet["data"].(map[string]any)
	if created["operationID"] != "create-client" {
		t.Fatalf("create operation ID mismatch: %#v", createdRet)
	}
	revision, _ := created["revision"].(string)
	savedRet := postMarkdownHandler(t, saveMarkdown,
		`{"notebook":"`+boxID+`","path":"/ops.md","content":"saved","revision":"`+revision+`","operationID":"save-client"}`)
	saved, _ := savedRet["data"].(map[string]any)
	if saved["operationID"] != "save-client" {
		t.Fatalf("save operation ID mismatch: %#v", savedRet)
	}
	duplicateRet := postMarkdownHandler(t, duplicateMarkdown,
		`{"notebook":"`+boxID+`","path":"/ops.md","revision":"`+saved["revision"].(string)+`","operationID":"duplicate-client"}`)
	duplicate, _ := duplicateRet["data"].(map[string]any)
	if duplicate["operationID"] != "duplicate-client" {
		t.Fatalf("duplicate operation ID mismatch: %#v", duplicateRet)
	}
	invalidRet := postMarkdownHandler(t, renameMarkdown,
		`{"notebook":"`+boxID+`","path":"/ops.md","name":"bad.md","revision":"`+saved["revision"].(string)+`","operationID":"bad operation"}`)
	if invalidRet["code"] == float64(0) {
		t.Fatalf("invalid operation ID accepted: %#v", invalidRet)
	}
}

func TestMarkdownTrashOperationIDs(t *testing.T) {
	boxID := setupFileTreeAPIBox(t)
	originalHistoryDir := util.HistoryDir
	util.HistoryDir = filepath.Join(t.TempDir(), "history")
	t.Cleanup(func() { util.HistoryDir = originalHistoryDir })
	created, err := model.CreateMarkdown(boxID, "/", "trash.md")
	if err != nil {
		t.Fatal(err)
	}
	removedRet := postMarkdownHandler(t, removeMarkdown,
		`{"notebook":"`+boxID+`","path":"/trash.md","revision":"`+created.Revision+`","operationID":"remove-client"}`)
	removed, _ := removedRet["data"].(map[string]any)
	if removed["operationID"] != "remove-client" {
		t.Fatalf("remove operation ID mismatch: %#v", removedRet)
	}
	id, _ := removed["id"].(string)
	restoredRet := postMarkdownHandler(t, restoreDeletedMarkdown,
		`{"id":"`+id+`","toNotebook":"`+boxID+`","toParentPath":"/","name":"restored.md","operationID":"restore-client"}`)
	restored, _ := restoredRet["data"].(map[string]any)
	if restored["operationID"] != "restore-client" {
		t.Fatalf("restore operation ID mismatch: %#v", restoredRet)
	}
	purgedRet := postMarkdownHandler(t, purgeDeletedMarkdown,
		`{"id":"`+id+`","operationID":"purge-client"}`)
	purged, _ := purgedRet["data"].(map[string]any)
	if purged["operationID"] != "purge-client" {
		t.Fatalf("purge operation ID mismatch: %#v", purgedRet)
	}
}

func TestDeletedMarkdownEndpointsRequireArguments(t *testing.T) {
	tests := []struct {
		name    string
		handler gin.HandlerFunc
		body    string
	}{
		{name: "get id", handler: getDeletedMarkdown, body: `{}`},
		{name: "restore id", handler: restoreDeletedMarkdown, body: `{"toNotebook":"20260811000000-abcdefg","toParentPath":"/","name":"a.md"}`},
		{name: "restore notebook", handler: restoreDeletedMarkdown, body: `{"id":"entry","toParentPath":"/","name":"a.md"}`},
		{name: "restore name", handler: restoreDeletedMarkdown, body: `{"id":"entry","toNotebook":"20260811000000-abcdefg","toParentPath":"/"}`},
		{name: "purge id", handler: purgeDeletedMarkdown, body: `{}`},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			assertMarkdownHandlerRejects(t, test.handler, test.body)
		})
	}
}

func TestListDeletedMarkdownAcceptsEmptyRequest(t *testing.T) {
	ret := postMarkdownHandler(t, listDeletedMarkdown, `{}`)
	if code, _ := ret["code"].(float64); code != 0 {
		t.Fatalf("empty list request was rejected: %#v", ret)
	}
}
