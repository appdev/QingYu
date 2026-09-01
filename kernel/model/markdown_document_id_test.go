// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package model

import (
	"bytes"
	"strings"
	"testing"

	"github.com/google/uuid"
)

const (
	testMarkdownDocumentID      = "019cbf15-5f81-7d2e-93a4-3a12b67e02ac"
	testOtherMarkdownDocumentID = "019cbf15-5f81-7d2e-a3a4-3a12b67e02ad"
)

func TestMarkdownDocumentIDInspection(t *testing.T) {
	tests := []struct {
		name  string
		data  string
		state string
		kind  string
		id    string
	}{
		{name: "without front matter", data: "# Title\n", state: "missing", kind: "none"},
		{name: "yaml", data: "---\ntitle: Note\nqingyu-document-id: " + testMarkdownDocumentID + "\n---\n", state: "valid", kind: "yaml", id: testMarkdownDocumentID},
		{name: "toml", data: "+++\ntitle = \"Note\"\nqingyu-document-id = \"" + testMarkdownDocumentID + "\"\n+++\n", state: "valid", kind: "toml", id: testMarkdownDocumentID},
		{name: "json", data: "{\n  \"title\": \"Note\",\n  \"qingyu-document-id\": \"" + testMarkdownDocumentID + "\"\n}\n", state: "valid", kind: "json", id: testMarkdownDocumentID},
		{name: "misspelled field", data: "---\nqingyu-docment-id: " + testMarkdownDocumentID + "\n---\n", state: "missing", kind: "yaml"},
		{name: "invalid value", data: "---\nqingyu-document-id: legacy-id\n---\n", state: "invalid-value", kind: "yaml"},
		{name: "duplicate field", data: "---\nqingyu-document-id: " + testMarkdownDocumentID + "\nqingyu-document-id: " + testOtherMarkdownDocumentID + "\n---\n", state: "malformed", kind: "yaml"},
		{name: "malformed yaml", data: "---\ntitle: [\n---\n", state: "malformed", kind: "yaml"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			actual := InspectMarkdownDocumentID([]byte(test.data))
			if actual.State != test.state || actual.Kind != test.kind || actual.ID != test.id {
				t.Fatalf("unexpected inspection: state=%q kind=%q id=%q", actual.State, actual.Kind, actual.ID)
			}
		})
	}
}

func TestMarkdownDocumentIDEnsure(t *testing.T) {
	t.Run("creates minimal yaml and preserves bom crlf", func(t *testing.T) {
		input := append([]byte{0xef, 0xbb, 0xbf}, []byte("# Title\r\nBody\r\n")...)
		mutation, err := EnsureMarkdownDocumentID(input, false)
		if err != nil {
			t.Fatal(err)
		}
		assertMarkdownDocumentID(t, mutation.ID)
		if !bytes.HasPrefix(mutation.Data, []byte{0xef, 0xbb, 0xbf}) {
			t.Fatal("UTF-8 BOM was not preserved")
		}
		if !bytes.Contains(mutation.Data, []byte("---\r\nqingyu-document-id: "+mutation.ID+"\r\n---\r\n\r\n# Title")) {
			t.Fatalf("unexpected data:\n%s", mutation.Data)
		}
	})

	t.Run("keeps misspelled field and appends canonical field", func(t *testing.T) {
		input := []byte("---\nqingyu-docment-id: legacy\ntitle: Note\n---\nBody\n")
		mutation, err := EnsureMarkdownDocumentID(input, false)
		if err != nil {
			t.Fatal(err)
		}
		output := string(mutation.Data)
		if !strings.Contains(output, "qingyu-docment-id: legacy") || !strings.Contains(output, MarkdownDocumentIDKey+": "+mutation.ID) {
			t.Fatalf("unexpected data:\n%s", output)
		}
	})

	t.Run("replaces only invalid field value", func(t *testing.T) {
		input := []byte("---\ntitle: Note\nqingyu-document-id: legacy # keep comment\ncustom: value\n---\nBody\n")
		mutation, err := EnsureMarkdownDocumentID(input, false)
		if err != nil {
			t.Fatal(err)
		}
		output := string(mutation.Data)
		if !strings.Contains(output, "qingyu-document-id: "+mutation.ID+" # keep comment") || !strings.Contains(output, "custom: value") {
			t.Fatalf("unexpected data:\n%s", output)
		}
	})

	t.Run("preserves malformed front matter after new block", func(t *testing.T) {
		input := []byte("---\ntitle: [\n---\nBody\n")
		mutation, err := EnsureMarkdownDocumentID(input, false)
		if err != nil {
			t.Fatal(err)
		}
		if !strings.HasSuffix(string(mutation.Data), string(input)) {
			t.Fatalf("malformed source was changed:\n%s", mutation.Data)
		}
		if InspectMarkdownDocumentID(mutation.Data).State != "valid" {
			t.Fatal("the prepended canonical front matter is not readable")
		}
	})

	t.Run("inserts into toml and json", func(t *testing.T) {
		inputs := [][]byte{
			[]byte("+++\ntitle = \"Note\"\n+++\nBody\n"),
			[]byte("{\n  \"title\": \"Note\"\n}\nBody\n"),
		}
		for _, input := range inputs {
			mutation, err := EnsureMarkdownDocumentID(input, false)
			if err != nil {
				t.Fatal(err)
			}
			inspection := InspectMarkdownDocumentID(mutation.Data)
			if inspection.State != "valid" || inspection.ID != mutation.ID {
				t.Fatalf("inserted ID is unreadable: state=%q id=%q\n%s", inspection.State, inspection.ID, mutation.Data)
			}
		}
	})

	t.Run("keeps valid id unless forced", func(t *testing.T) {
		input := []byte("---\nqingyu-document-id: " + testMarkdownDocumentID + "\n---\n")
		unchanged, err := EnsureMarkdownDocumentID(input, false)
		if err != nil {
			t.Fatal(err)
		}
		if unchanged.Changed || unchanged.ID != testMarkdownDocumentID || !bytes.Equal(unchanged.Data, input) {
			t.Fatal("valid ID was unexpectedly changed")
		}
		changed, err := EnsureMarkdownDocumentID(input, true)
		if err != nil {
			t.Fatal(err)
		}
		if !changed.Changed || changed.ID == testMarkdownDocumentID {
			t.Fatal("forced regeneration did not create a new ID")
		}
	})
}

func TestMarkdownPreviewContentRevision(t *testing.T) {
	first := []byte("---\ntitle: Note\nqingyu-document-id: " + testMarkdownDocumentID + "\n---\nBody\n")
	second := []byte("---\ntitle: Note\nqingyu-document-id: " + testOtherMarkdownDocumentID + "\n---\nBody\n")
	firstRevision, err := MarkdownPreviewContentRevision(first)
	if err != nil {
		t.Fatal(err)
	}
	secondRevision, err := MarkdownPreviewContentRevision(second)
	if err != nil {
		t.Fatal(err)
	}
	if firstRevision != secondRevision {
		t.Fatal("document ID must not affect preview content revision")
	}
	changedRevision, err := MarkdownPreviewContentRevision(bytes.ReplaceAll(second, []byte("title: Note"), []byte("title: Changed")))
	if err != nil {
		t.Fatal(err)
	}
	if firstRevision == changedRevision {
		t.Fatal("content change did not affect preview content revision")
	}
}

func TestDocumentCardRatio(t *testing.T) {
	first := DocumentCardRatio(testMarkdownDocumentID)
	second := DocumentCardRatio(testMarkdownDocumentID)
	if first != second {
		t.Fatal("ratio is not deterministic")
	}
	if first < 1.05 || first > 1.4 {
		t.Fatalf("ratio %f is outside the supported range", first)
	}
}

func assertMarkdownDocumentID(t *testing.T, value string) {
	t.Helper()
	id, err := uuid.Parse(value)
	if err != nil {
		t.Fatalf("invalid UUID: %v", err)
	}
	if id.Version() != 7 {
		t.Fatalf("expected UUIDv7, got version %d", id.Version())
	}
}
