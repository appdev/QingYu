package model

import (
	"errors"
	"testing"

	"github.com/88250/lute/ast"
	"github.com/88250/lute/parse"
)

func TestNativeDocumentCreationDisabled(t *testing.T) {
	tree := &parse.Tree{ID: "20260911000000-newdoc1", Box: "20260911000000-notebox", Root: &ast.Node{Type: ast.NodeDocument}}
	if err := checkDatabaseOperations([]*Operation{{Action: "create", Data: tree}}); !errors.Is(err, ErrNativeDocumentCreationDisabled) {
		t.Fatalf("native creation accepted: %v", err)
	}
	if err := DuplicateDoc(tree); !errors.Is(err, ErrNativeDocumentCreationDisabled) {
		t.Fatalf("native duplication accepted: %v", err)
	}
	if _, err := createDocsByHPath(tree.Box, "/new", "", "", tree.ID, false); !errors.Is(err, ErrNativeDocumentCreationDisabled) {
		t.Fatalf("native path creation accepted: %v", err)
	}
}

func TestNativeExistingContentRemainsEditable(t *testing.T) {
	for _, data := range []string{
		`<div data-type="NodeParagraph">edited</div>`,
		`<span data-type="block-ref">reference</span>`,
		`<span data-type="inline-memo">memo</span>`,
		`{"memo":"updated"}`,
	} {
		if err := checkDatabaseOperations([]*Operation{{Action: "update", Data: data}}); err != nil {
			t.Fatalf("existing content edit rejected: %v", err)
		}
	}
}

func TestNativeSQLTemplateFunctionsRemoved(t *testing.T) {
	previousConf := Conf
	Conf = &AppConf{Lang: "en"}
	t.Cleanup(func() { Conf = previousConf })
	for _, source := range []string{`{{querySQL "select 1"}}`, `{{queryBlocks "select * from blocks"}}`, `{{querySpans "select * from spans"}}`} {
		if _, err := RenderGoTemplate(source); err == nil {
			t.Fatalf("SQL template remains executable: %s", source)
		}
	}
	if text, err := RenderGoTemplate("plain Markdown"); err != nil || text != "plain Markdown" {
		t.Fatalf("ordinary template failed: %q, %v", text, err)
	}
}
