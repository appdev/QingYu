// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package model

import (
	"bytes"
	"encoding/base64"
	"image"
	"image/jpeg"
	"image/png"
	"os"
	"strings"
	"testing"

	"github.com/siyuan-note/siyuan/kernel/util"
)

const mediumDocumentCardPreviewWebP = "UklGRjwAAABXRUJQVlA4TDAAAAAvf8LvAAfQ+ta3vv8BAEX6/58i+p/63//+97///e9///vf//73v//973//+9//EAA="
const invalidSizeDocumentCardPreviewWebP = "UklGRh4AAABXRUJQVlA4TBEAAAAvCUACAAfQ+ta3vv+BiOh/AAA="
const testDocumentCardPreviewAppearanceKey = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

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
	descriptor, err := PrepareDocumentCardPreview(ref, "light", testDocumentCardPreviewAppearanceKey, "medium", "")
	if err != nil {
		t.Fatal(err)
	}
	if descriptor.Exists || len(descriptor.CacheKey) != 64 || descriptor.DocumentID != document.DocumentID {
		t.Fatalf("unexpected descriptor: %#v", descriptor)
	}
	if descriptor.RendererVersion != 7 || !strings.HasSuffix(descriptor.URL, ".webp") {
		t.Fatalf("preview does not use WebP: %#v", descriptor)
	}
	encoded := bytes.NewReader(documentCardPreviewWebP(t, mediumDocumentCardPreviewWebP))
	if err = StoreDocumentCardPreview(ref, *descriptor, encoded); err != nil {
		t.Fatal(err)
	}
	preparedAgain, err := PrepareDocumentCardPreview(ref, "light", testDocumentCardPreviewAppearanceKey, "medium", "")
	if err != nil || !preparedAgain.Exists || preparedAgain.CacheKey != descriptor.CacheKey {
		t.Fatalf("stored preview was not reused: %#v, %v", preparedAgain, err)
	}
	dark, err := PrepareDocumentCardPreview(ref, "dark", testDocumentCardPreviewAppearanceKey, "medium", "")
	if err != nil || dark.CacheKey == descriptor.CacheKey || dark.Exists {
		t.Fatalf("theme variant was not isolated: %#v, %v", dark, err)
	}
	otherAppearance, err := PrepareDocumentCardPreview(ref, "light", strings.Repeat("a", 64), "medium", "")
	if err != nil || otherAppearance.CacheKey == descriptor.CacheKey || otherAppearance.Exists {
		t.Fatalf("appearance variant was not isolated: %#v, %v", otherAppearance, err)
	}
}

func TestDocumentCardPreviewRejectsInvalidAppearanceKey(t *testing.T) {
	box := setupMarkdownTest(t)
	document, err := CreateMarkdown(box.ID, "/", "invalid-appearance.md")
	if err != nil {
		t.Fatal(err)
	}
	ref := DocumentCardReference{Kind: "markdown", Notebook: box.ID, Path: document.Path}
	if _, err = PrepareDocumentCardPreview(ref, "light", "Savor", "medium", ""); err == nil {
		t.Fatal("invalid appearance key was accepted")
	}
	if _, err = PrepareDocumentCardPreview(ref, "light", strings.Repeat("A", 64), "medium", ""); err == nil {
		t.Fatal("uppercase appearance key was accepted")
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
	descriptor, err := PrepareDocumentCardPreview(ref, "light", testDocumentCardPreviewAppearanceKey, "medium", "")
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
	descriptor, err := PrepareDocumentCardPreview(ref, "light", testDocumentCardPreviewAppearanceKey, "medium", "")
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

func TestDocumentCardPreviewPNGIsolation(t *testing.T) {
	box := setupMarkdownTest(t)
	originalTempDir := util.TempDir
	util.TempDir = t.TempDir()
	t.Cleanup(func() { util.TempDir = originalTempDir })
	document, err := CreateMarkdown(box.ID, "/", "png-preview.md")
	if err != nil {
		t.Fatal(err)
	}
	ref := DocumentCardReference{Kind: "markdown", Notebook: box.ID, Path: document.Path}
	descriptor, err := PrepareDocumentCardPreview(ref, "light", testDocumentCardPreviewAppearanceKey, "medium", "png")
	if err != nil {
		t.Fatal(err)
	}
	webp, err := PrepareDocumentCardPreview(ref, "light", testDocumentCardPreviewAppearanceKey, "medium", "webp")
	if err != nil || descriptor.CacheKey == webp.CacheKey || !strings.HasSuffix(descriptor.URL, ".png") {
		t.Fatalf("formats not isolated: %#v, %v", descriptor, err)
	}
	var encoded bytes.Buffer
	if err = png.Encode(&encoded, image.NewRGBA(image.Rect(0, 0, 640, 960))); err != nil {
		t.Fatal(err)
	}
	data := encoded.Bytes()
	if err = StoreDocumentCardPreview(ref, *webp, bytes.NewReader(data)); err == nil {
		t.Fatal("PNG accepted for WebP descriptor")
	}
	if err = StoreDocumentCardPreview(ref, *descriptor, bytes.NewReader(data)); err != nil {
		t.Fatal(err)
	}
	again, err := PrepareDocumentCardPreview(ref, "light", testDocumentCardPreviewAppearanceKey, "medium", "png")
	if err != nil || !again.Exists {
		t.Fatalf("PNG cache not reused: %#v, %v", again, err)
	}
	file, err := DocumentCardPreviewFile(box.ID, descriptor.CacheKey, "png")
	if err != nil {
		t.Fatal(err)
	}
	stored, err := os.ReadFile(file)
	if err != nil || !bytes.Equal(stored, data) {
		t.Fatal("PNG cache changed bytes", err)
	}
	if _, err = PrepareDocumentCardPreview(ref, "light", testDocumentCardPreviewAppearanceKey, "medium", "jpeg"); err == nil {
		t.Fatal("unsupported format accepted")
	}
	if _, err = DocumentCardPreviewFile(box.ID, descriptor.CacheKey, "../png"); err == nil {
		t.Fatal("invalid extension accepted")
	}
	encoded.Reset()
	_ = png.Encode(&encoded, image.NewRGBA(image.Rect(0, 0, 1, 1)))
	if err = StoreDocumentCardPreview(ref, *descriptor, &encoded); err == nil {
		t.Fatal("wrong PNG dimensions accepted")
	}
	if err = StoreDocumentCardPreview(ref, *descriptor, bytes.NewReader(make([]byte, 3*1024*1024+1))); err == nil {
		t.Fatal("oversized PNG accepted")
	}
}
