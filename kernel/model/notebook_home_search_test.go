// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

package model

import (
	"strings"
	"testing"
)

func TestNotebookHomeSearchSnippetEscapesAndMarks(t *testing.T) {
	snippet := notebookHomeSnippet(`<script>alert("x")</script> 轻语首页`, "轻语")
	if strings.Contains(snippet, "<script>") || !strings.Contains(snippet, "&lt;script&gt;") {
		t.Fatalf("snippet was not escaped: %s", snippet)
	}
	if !strings.Contains(snippet, "<mark>轻语</mark>") {
		t.Fatalf("query was not marked: %s", snippet)
	}
}
