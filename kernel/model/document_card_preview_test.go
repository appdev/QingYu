// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package model

import (
	"bytes"
	"encoding/base64"
	"image"
	"image/jpeg"
	"strings"
	"testing"

	"github.com/siyuan-note/siyuan/kernel/util"
)

const mediumDocumentCardPreviewWebP = "UklGRjwAAABXRUJQVlA4TDAAAAAvf8LvAAfQ+ta3vv8BAEX6/58i+p/63//+97///e9///vf//73v//973//+9//EAA="
const invalidSizeDocumentCardPreviewWebP = "UklGRh4AAABXRUJQVlA4TBEAAAAvCUACAAfQ+ta3vv+BiOh/AAA="

func documentCardPreviewWebP(t *testing.T, encoded string) []byte {
	t.Helper()
	data, err := base64.StdEncoding.DecodeString(encoded)
	if err != nil {
		t.Fatal(err)
	}
	return data
}

func TestMarkdownDocumentCardPreviewCache(t *testing.T) {
	box := setupMarkdownTest(t)
	originalTempDir := util.TempDir
	util.TempDir = t.TempDir()
	t.Cleanup(func() { util.TempDir = originalTempDir })
	document, err := CreateMarkdown(box.ID, "/", "preview.md")
	if err != nil {
		t.Fatal(err)
	}
	ref := DocumentCardReference{Kind: "markdown", Notebook: box.ID, Path: document.Path}
	descriptor, err := PrepareDocumentCardPreview(ref, "light", "medium")
	if err != nil {
		t.Fatal(err)
	}
	if descriptor.Exists || len(descriptor.CacheKey) != 64 || descriptor.DocumentID != document.DocumentID {
		t.Fatalf("unexpected descriptor: %#v", descriptor)
	}
	if descriptor.RendererVersion != 4 || !strings.HasSuffix(descriptor.URL, ".webp") {
		t.Fatalf("preview does not use WebP: %#v", descriptor)
	}
	encoded := bytes.NewReader(documentCardPreviewWebP(t, mediumDocumentCardPreviewWebP))
	if err = StoreDocumentCardPreview(ref, *descriptor, encoded); err != nil {
		t.Fatal(err)
	}
	preparedAgain, err := PrepareDocumentCardPreview(ref, "light", "medium")
	if err != nil || !preparedAgain.Exists || preparedAgain.CacheKey != descriptor.CacheKey {
		t.Fatalf("stored preview was not reused: %#v, %v", preparedAgain, err)
	}
	dark, err := PrepareDocumentCardPreview(ref, "dark", "medium")
	if err != nil || dark.CacheKey == descriptor.CacheKey || dark.Exists {
		t.Fatalf("theme variant was not isolated: %#v, %v", dark, err)
	}
}

func TestDocumentCardPreviewRejectsWrongDimensions(t *testing.T) {
	box := setupMarkdownTest(t)
	originalTempDir := util.TempDir
	util.TempDir = t.TempDir()
	t.Cleanup(func() { util.TempDir = originalTempDir })
	document, err := CreateMarkdown(box.ID, "/", "wrong-size.md")
	if err != nil {
		t.Fatal(err)
	}
	ref := DocumentCardReference{Kind: "markdown", Notebook: box.ID, Path: document.Path}
	descriptor, err := PrepareDocumentCardPreview(ref, "light", "medium")
	if err != nil {
		t.Fatal(err)
	}
	encoded := bytes.NewReader(documentCardPreviewWebP(t, invalidSizeDocumentCardPreviewWebP))
	if err = StoreDocumentCardPreview(ref, *descriptor, encoded); err == nil {
		t.Fatal("wrong dimensions were accepted")
	}
}

func TestDocumentCardPreviewRejectsJPEG(t *testing.T) {
	box := setupMarkdownTest(t)
	originalTempDir := util.TempDir
	util.TempDir = t.TempDir()
	t.Cleanup(func() { util.TempDir = originalTempDir })
	document, err := CreateMarkdown(box.ID, "/", "jpeg.md")
	if err != nil {
		t.Fatal(err)
	}
	ref := DocumentCardReference{Kind: "markdown", Notebook: box.ID, Path: document.Path}
	descriptor, err := PrepareDocumentCardPreview(ref, "light", "medium")
	if err != nil {
		t.Fatal(err)
	}
	var encoded bytes.Buffer
	if err = jpeg.Encode(&encoded, image.NewRGBA(image.Rect(0, 0, 640, 960)), &jpeg.Options{Quality: 82}); err != nil {
		t.Fatal(err)
	}
	if err = StoreDocumentCardPreview(ref, *descriptor, &encoded); err == nil {
		t.Fatal("JPEG preview was accepted")
	}
}
