// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package api

import (
	"encoding/json"
	"errors"
	"net/http"
	"testing"

	"github.com/88250/gulu"
	"github.com/siyuan-note/siyuan/kernel/model"
)

func TestPrepareDocumentCardPreviewAcceptsObjectReference(t *testing.T) {
	boxID := setupFileTreeAPIBox(t)
	document, err := model.CreateMarkdown(boxID, "/", "preview.md")
	if err != nil {
		t.Fatal(err)
	}
	body, err := json.Marshal(map[string]any{
		"reference": map[string]any{
			"kind":     "markdown",
			"notebook": boxID,
			"path":     document.Path,
			"id":       document.DocumentID,
		},
		"theme":         "light",
		"appearanceKey": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
		"size":          "medium",
	})
	if err != nil {
		t.Fatal(err)
	}
	ret := postMarkdownHandler(t, prepareDocumentCardPreview, string(body))
	if ret["code"] != float64(0) {
		t.Fatalf("object reference was rejected: %#v", ret)
	}
}

func TestDocumentCardPreviewConflictUsesRetryableCode(t *testing.T) {
	ret := gulu.Ret.NewResult()
	setDocumentCardPreviewError(ret, errors.Join(errors.New("store failed"), model.ErrMarkdownConflict))
	if ret.Code != http.StatusConflict || ret.Msg == "" {
		t.Fatalf("conflict was not marked retryable: %#v", ret)
	}

	setDocumentCardPreviewError(ret, errors.New("invalid preview"))
	if ret.Code != -1 {
		t.Fatalf("non-conflict error code changed: %#v", ret)
	}
}
