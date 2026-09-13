package model

import (
	ignore "github.com/sabhiram/go-gitignore"
	"testing"
)

func TestMarkdownAnnotationsSyncKeepsSidecarButExcludesLocalTransactions(t *testing.T) {
	box := setupMarkdownTest(t)
	matcher := ignore.CompileIgnoreLines(getSyncIgnoreLines()...)
	if matcher.MatchesPath(box.ID + "/reading.md.annotations.json") {
		t.Fatal("annotation sidecar excluded")
	}
	if !matcher.MatchesPath(box.ID + "/.siyuan/markdown-transactions/transaction-id/transaction.json") {
		t.Fatal("local recovery journal can be synchronized")
	}
}
