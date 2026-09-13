package model

import (
	"archive/zip"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"testing"

	"github.com/siyuan-note/siyuan/kernel/util"
)

func TestMarkdownAnnotationsZipIncludesPortableSidecar(t *testing.T) {
	box := setupMarkdownTest(t)
	oldTempDir := util.TempDir
	util.TempDir = t.TempDir()
	t.Cleanup(func() { util.TempDir = oldTempDir })
	doc, err := CreateMarkdown(box.ID, "/", "portable")
	if err != nil {
		t.Fatal(err)
	}
	doc, err = SaveMarkdownWithAnnotations(box.ID, doc.Path, "甲乙丙", doc.Revision, "", annotationFixture())
	if err != nil {
		t.Fatal(err)
	}
	artifact, err := ExportMarkdownDocumentZip(box.ID, doc.Path)
	if err != nil {
		t.Fatal(err)
	}
	reader, err := zip.OpenReader(filepath.Join(util.TempDir, "export", filepath.Base(artifact.Path)))
	if err != nil {
		t.Fatal(err)
	}
	defer reader.Close()
	for _, file := range reader.File {
		if file.Name != doc.Name+markdownAnnotationsSuffix {
			continue
		}
		stream, openErr := file.Open()
		if openErr != nil {
			t.Fatal(openErr)
		}
		data, readErr := io.ReadAll(stream)
		_ = stream.Close()
		if readErr != nil {
			t.Fatal(readErr)
		}
		var annotations MarkdownAnnotations
		if err = json.Unmarshal(data, &annotations); err != nil {
			t.Fatal(err)
		}
		if annotations.ETag != "" || annotations.DocumentID != doc.Annotations.DocumentID || len(annotations.Records) != 1 || annotations.Records[0].Note != "笔记" {
			t.Fatal("exported annotation payload is not portable", annotations)
		}
		return
	}
	t.Fatal("annotation sidecar missing from Markdown ZIP")
}

func annotationFixture() *MarkdownAnnotations {
	return &MarkdownAnnotations{SchemaVersion: 1, DocumentID: "test-document", Records: []MarkdownAnnotation{
		{ID: "a", From: 0, To: 2, Quote: "甲乙", Note: "笔记", Status: "attached", CreatedAt: 1, UpdatedAt: 1},
	}}
}

func TestMarkdownAnnotationsLifecycle(t *testing.T) {
	box := setupMarkdownTest(t)
	oldHistory := util.HistoryDir
	util.HistoryDir = filepath.Join(t.TempDir(), "history")
	t.Cleanup(func() { util.HistoryDir = oldHistory })
	doc, err := CreateMarkdown(box.ID, "/", "annotations")
	if err != nil {
		t.Fatal(err)
	}
	doc, err = SaveMarkdownWithAnnotations(box.ID, doc.Path, "甲乙丙", doc.Revision, "", annotationFixture())
	if err != nil {
		t.Fatal(err)
	}
	if doc.Annotations.Revision != 1 || doc.Annotations.ContentHash != markdownAnnotationHash("甲乙丙") {
		t.Fatal(doc.Annotations)
	}
	if _, err = SaveMarkdownWithAnnotations(box.ID, doc.Path, doc.Content, doc.Revision, "", annotationFixture()); !errors.Is(err, ErrMarkdownConflict) {
		t.Fatalf("expected stale annotation conflict: %v", err)
	}
	copyDoc, err := DuplicateMarkdown(box.ID, doc.Path, doc.Revision)
	if err != nil {
		t.Fatal(err)
	}
	if copyDoc.Annotations.DocumentID == doc.Annotations.DocumentID || len(copyDoc.Annotations.Records) != 1 {
		t.Fatal(copyDoc.Annotations)
	}
	if copyDoc.Annotations.Records[0].Status != "attached" || copyDoc.Annotations.ContentHash != markdownAnnotationHash(copyDoc.Content) {
		t.Fatal("copy did not adjust its frontmatter-relative anchor", copyDoc.Annotations)
	}
	oldPath := filepath.Join(util.DataDir, box.ID, doc.Path)
	doc, err = RenameMarkdownWithRevision(box.ID, doc.Path, "renamed.md", doc.Revision)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = os.Lstat(oldPath + markdownAnnotationsSuffix); !os.IsNotExist(err) {
		t.Fatal("old sidecar remains", err)
	}
	entry, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: doc.Path}, doc.Revision)
	if err != nil {
		t.Fatal(err)
	}
	doc, err = RestoreDeletedMarkdown(entry.ID, box.ID, "/", "restored.md")
	if err != nil {
		t.Fatal(err)
	}
	if doc.Annotations.Records[0].Note != "笔记" {
		t.Fatal(doc.Annotations)
	}
}

func TestMarkdownAnnotationsRecoverInterruptedInstall(t *testing.T) {
	box := setupMarkdownTest(t)
	doc, err := CreateMarkdown(box.ID, "/", "recovery")
	if err != nil {
		t.Fatal(err)
	}
	oldHook := markdownTransactionCrashHook
	t.Cleanup(func() { markdownTransactionCrashHook = oldHook })
	markdownTransactionCrashHook = func(kind, phase string) error {
		if kind == "annotations" && phase == "installed" {
			return ErrMarkdownSimulatedCrash
		}
		return nil
	}
	_, err = SaveMarkdownWithAnnotations(box.ID, doc.Path, "甲乙丙", doc.Revision, "", annotationFixture())
	if !errors.Is(err, ErrMarkdownSimulatedCrash) {
		t.Fatal(err)
	}
	markdownTransactionCrashHook = oldHook
	doc, err = GetMarkdown(box.ID, doc.Path)
	if err != nil {
		t.Fatal(err)
	}
	if doc.Content != "甲乙丙" || doc.Annotations.Revision != 1 {
		t.Fatal(doc)
	}
	doc.Annotations.Records[0].Note = "更新"
	doc, err = SaveMarkdownWithAnnotations(box.ID, doc.Path, doc.Content, doc.Revision, "", doc.Annotations)
	if err != nil || doc.Annotations.Revision != 2 {
		t.Fatal(doc, err)
	}
}

func TestMarkdownAnnotationsRejectUnknownAndSymlink(t *testing.T) {
	box := setupMarkdownTest(t)
	doc, err := CreateMarkdown(box.ID, "/", "invalid")
	if err != nil {
		t.Fatal(err)
	}
	sidecar := filepath.Join(util.DataDir, box.ID, "invalid.md") + markdownAnnotationsSuffix
	if err = os.WriteFile(sidecar, []byte(`{"schemaVersion":99}`), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = GetMarkdown(box.ID, doc.Path); !errors.Is(err, ErrInvalidMarkdownAnnotations) {
		t.Fatal(err)
	}
	if err = os.Remove(sidecar); err != nil {
		t.Fatal(err)
	}
	other := filepath.Join(t.TempDir(), "private.json")
	if err = os.WriteFile(other, []byte("untouched"), 0600); err != nil {
		t.Fatal(err)
	}
	if err = os.Symlink(other, sidecar); err != nil {
		t.Fatal(err)
	}
	if _, err = SaveMarkdownWithAnnotations(box.ID, doc.Path, "甲乙丙", doc.Revision, "", annotationFixture()); err == nil {
		t.Fatal("accepted symlink")
	}
	data, err := os.ReadFile(other)
	if err != nil || string(data) != "untouched" {
		t.Fatal(string(data), err)
	}
}

func TestMarkdownAnnotationsEqualRevisionDifferentContentsConflict(t *testing.T) {
	box := setupMarkdownTest(t)
	doc, err := CreateMarkdown(box.ID, "/", "etag")
	if err != nil {
		t.Fatal(err)
	}
	doc, err = SaveMarkdownWithAnnotations(box.ID, doc.Path, "甲乙丙", doc.Revision, "", annotationFixture())
	if err != nil {
		t.Fatal(err)
	}
	remote := *doc.Annotations
	remote.Records = append([]MarkdownAnnotation(nil), remote.Records...)
	remote.Records[0].Note = "另一台设备"
	remote.ETag = ""
	data, _ := json.Marshal(remote)
	if err = os.WriteFile(filepath.Join(util.DataDir, box.ID, "etag.md.annotations.json"), data, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = SaveMarkdownWithAnnotations(box.ID, doc.Path, doc.Content, doc.Revision, "", doc.Annotations); !errors.Is(err, ErrMarkdownConflict) {
		t.Fatal("equal revision lost remote annotation", err)
	}
}

func TestMarkdownAnnotationsPrecommitFailuresPreserveBothFiles(t *testing.T) {
	for _, phase := range []string{"staged", "source-isolated", "linked"} {
		t.Run(phase, func(t *testing.T) {
			box := setupMarkdownTest(t)
			doc, err := CreateMarkdown(box.ID, "/", "failure")
			if err != nil {
				t.Fatal(err)
			}
			doc, err = SaveMarkdownWithAnnotations(box.ID, doc.Path, "甲乙丙", doc.Revision, "", annotationFixture())
			if err != nil {
				t.Fatal(err)
			}
			oldCrash, oldCommit := markdownTransactionCrashHook, markdownSaveCommitHook
			t.Cleanup(func() { markdownTransactionCrashHook, markdownSaveCommitHook = oldCrash, oldCommit })
			markdownTransactionCrashHook = func(kind, step string) error {
				if kind == "save" && step == phase {
					return ErrMarkdownSimulatedCrash
				}
				return nil
			}
			markdownSaveCommitHook = func(step, _ string) error {
				if step == phase {
					return ErrMarkdownSimulatedCrash
				}
				return nil
			}
			doc.Annotations.Records[0].Note = "未提交"
			_, err = SaveMarkdownWithAnnotations(box.ID, doc.Path, "甲乙丙丁", doc.Revision, "", doc.Annotations)
			if !errors.Is(err, ErrMarkdownSimulatedCrash) {
				t.Fatal(err)
			}
			markdownTransactionCrashHook, markdownSaveCommitHook = oldCrash, oldCommit
			restored, err := GetMarkdown(box.ID, doc.Path)
			if err != nil {
				t.Fatal(err)
			}
			if restored.Content != "甲乙丙" || restored.Annotations.Records[0].Note != "笔记" {
				t.Fatal(restored)
			}
		})
	}
}
