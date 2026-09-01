// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package model

import (
	"archive/zip"
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/siyuan-note/siyuan/kernel/util"
)

func TestLoadMarkdownExportDocumentPreservesSourceAndResources(t *testing.T) {
	box := setupMarkdownTest(t)
	content := []byte("\xef\xbb\xbf---\r\ntitle: Export title\r\ncover: assets/cover.png\r\n---\r\n![image](assets/image.png?x=1#part)\r\n[attachment](files/a.pdf)\r\n![remote](https://example.com/a.png)\r\n")
	documentPath := filepath.Join(util.DataDir, box.ID, "docs", "note.markdown")
	writeMarkdownExportFixture(t, documentPath, content)
	writeMarkdownExportFixture(t, filepath.Join(util.DataDir, box.ID, "assets", "cover.png"), []byte("cover"))
	writeMarkdownExportFixture(t, filepath.Join(util.DataDir, box.ID, "assets", "image.png"), []byte("image"))
	writeMarkdownExportFixture(t, filepath.Join(util.DataDir, box.ID, "docs", "files", "a.pdf"), []byte("pdf"))

	doc, err := LoadMarkdownExportDocument(box.ID, "/docs/note.markdown")
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(doc.Content, content) || doc.Title != "Export title" || doc.Extension != ".markdown" {
		t.Fatalf("source metadata changed: %+v", doc)
	}
	if len(doc.Resources) != 3 {
		t.Fatalf("unexpected resources: %+v", doc.Resources)
	}
	if doc.Resources[0].ArchivePath != "assets/cover.png" || doc.Resources[1].ArchivePath != "assets/image.png" ||
		doc.Resources[2].ArchivePath != "files/a.pdf" {
		t.Fatalf("unexpected archive paths: %+v", doc.Resources)
	}
	if doc.Resources[1].Raw != "assets/image.png?x=1#part" {
		t.Fatalf("query or fragment was lost: %+v", doc.Resources[1])
	}

	stageDir := t.TempDir()
	missing, err := doc.Stage(stageDir)
	if err != nil || len(missing) != 0 {
		t.Fatalf("stage failed: missing=%v err=%v", missing, err)
	}
	staged, err := os.ReadFile(filepath.Join(stageDir, doc.Name))
	if err != nil || !bytes.Equal(staged, content) {
		t.Fatalf("staged source changed: %v", err)
	}
}

func TestExportMarkdownDocumentPreviewResolvesGlobalAsset(t *testing.T) {
	box := setupMarkdownTest(t)
	writeMarkdownExportFixture(t, filepath.Join(util.DataDir, box.ID, "note.md"), []byte("![image](assets/global.png)"))
	writeMarkdownExportFixture(t, filepath.Join(util.DataDir, "assets", "global.png"), []byte("image"))

	_, content, missing, err := ExportMarkdownDocumentPreview(box.ID, "/note.md")
	if err != nil {
		t.Fatal(err)
	}
	if len(missing) != 0 {
		t.Fatalf("global asset was reported missing: %v", missing)
	}
	if !strings.Contains(content, "data:image/png;base64,aW1hZ2U=") {
		t.Fatalf("global asset was not embedded in preview: %s", content)
	}
}

func TestMarkdownExportResourcesWarnForMissingAndRejectEscape(t *testing.T) {
	box := setupMarkdownTest(t)
	documentPath := filepath.Join(util.DataDir, box.ID, "note.md")
	writeMarkdownExportFixture(t, documentPath, []byte("![missing](assets/missing.png)"))
	doc, err := LoadMarkdownExportDocument(box.ID, "/note.md")
	if err != nil || len(doc.Resources) != 1 || !doc.Resources[0].Missing {
		t.Fatalf("safe missing resource was not reported: %+v %v", doc, err)
	}
	missing, err := doc.Stage(t.TempDir())
	if err != nil || len(missing) != 1 || missing[0] != "assets/missing.png" {
		t.Fatalf("unexpected missing result: %v %v", missing, err)
	}

	for _, target := range []string{"../outside.png", "%2e%2e/outside.png", "%252e%252e/outside.png", "file:///tmp/a"} {
		writeMarkdownExportFixture(t, documentPath, []byte("![escape]("+target+")"))
		if _, loadErr := LoadMarkdownExportDocument(box.ID, "/note.md"); !errors.Is(loadErr, ErrInvalidMarkdownPath) {
			t.Fatalf("unsafe target %q was accepted: %v", target, loadErr)
		}
	}
}

func TestMarkdownExportResourcesRejectSymlink(t *testing.T) {
	box := setupMarkdownTest(t)
	outside := filepath.Join(t.TempDir(), "outside.png")
	writeMarkdownExportFixture(t, outside, []byte("outside"))
	assetPath := filepath.Join(util.DataDir, box.ID, "assets", "linked.png")
	if err := os.MkdirAll(filepath.Dir(assetPath), 0755); err != nil {
		t.Fatal(err)
	}
	symlinkOrSkip(t, outside, assetPath)
	writeMarkdownExportFixture(t, filepath.Join(util.DataDir, box.ID, "note.md"), []byte("![linked](assets/linked.png)"))
	if _, err := LoadMarkdownExportDocument(box.ID, "/note.md"); !errors.Is(err, ErrInvalidMarkdownPath) {
		t.Fatalf("symlink resource was accepted: %v", err)
	}
}

func TestMarkdownExportRejectsNotebookHomeStorage(t *testing.T) {
	box := setupMarkdownTest(t)
	writeMarkdownExportFixture(t, filepath.Join(util.DataDir, box.ID, ".qingyu", "home.md"), []byte("home"))
	if _, err := LoadMarkdownExportDocument(box.ID, "/.qingyu/home.md"); !errors.Is(err, ErrInvalidMarkdownPath) {
		t.Fatalf("notebook home storage was exportable: %v", err)
	}
}

func TestExportMarkdownDocumentZipPreservesRawSource(t *testing.T) {
	box := setupMarkdownTest(t)
	originalTempDir := util.TempDir
	util.TempDir = t.TempDir()
	t.Cleanup(func() { util.TempDir = originalTempDir })
	content := []byte("\xef\xbb\xbf# Raw\r\n![image](assets/image.png)\r\n")
	writeMarkdownExportFixture(t, filepath.Join(util.DataDir, box.ID, "raw.markdown"), content)
	writeMarkdownExportFixture(t, filepath.Join(util.DataDir, box.ID, "assets", "image.png"), []byte("image"))
	artifact, err := ExportMarkdownDocumentZip(box.ID, "/raw.markdown")
	if err != nil {
		t.Fatal(err)
	}
	zipPath := filepath.Join(util.TempDir, "export", filepath.Base(artifact.Path))
	reader, err := zip.OpenReader(zipPath)
	if err != nil {
		t.Fatal(err)
	}
	defer reader.Close()
	entries := map[string][]byte{}
	for _, file := range reader.File {
		stream, openErr := file.Open()
		if openErr != nil {
			t.Fatal(openErr)
		}
		var data bytes.Buffer
		if _, copyErr := data.ReadFrom(stream); copyErr != nil {
			t.Fatal(copyErr)
		}
		_ = stream.Close()
		entries[file.Name] = data.Bytes()
		if strings.HasSuffix(file.Name, ".sy") {
			t.Fatalf("ZIP contains QingYu source: %s", file.Name)
		}
	}
	if !bytes.Equal(entries["raw.markdown"], content) || !bytes.Equal(entries["assets/image.png"], []byte("image")) {
		t.Fatalf("unexpected ZIP entries: %+v", entries)
	}
}

func writeMarkdownExportFixture(t *testing.T, filePath string, content []byte) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(filePath), 0755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filePath, content, 0644); err != nil {
		t.Fatal(err)
	}
}
