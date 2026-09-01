// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package model

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/88250/gulu"
	"github.com/siyuan-note/siyuan/kernel/util"
	_ "golang.org/x/image/webp"
)

const documentCardPreviewRendererVersion = 4

type DocumentCardReference struct {
	Kind     string `json:"kind"`
	Notebook string `json:"notebook"`
	Path     string `json:"path,omitempty"`
	ID       string `json:"id,omitempty"`
}

type DocumentCardPreviewDescriptor struct {
	DocumentID       string `json:"documentID"`
	ContentRevision  string `json:"contentRevision"`
	ResourceRevision string `json:"resourceRevision"`
	Theme            string `json:"theme"`
	Size             string `json:"size"`
	RendererVersion  int    `json:"rendererVersion"`
	CacheKey         string `json:"cacheKey"`
	URL              string `json:"url"`
	Exists           bool   `json:"exists"`
}

func PrepareDocumentCardPreview(ref DocumentCardReference, theme, size string) (*DocumentCardPreviewDescriptor, error) {
	if theme != "light" && theme != "dark" {
		return nil, errors.New("invalid document card preview theme")
	}
	if size != "medium" && size != "small" {
		return nil, errors.New("invalid document card preview size")
	}
	if !validNotebookRootReference(ref) {
		return nil, errors.New("invalid document card reference")
	}
	descriptor := &DocumentCardPreviewDescriptor{Theme: theme, Size: size, RendererVersion: documentCardPreviewRendererVersion}
	if ref.Kind == "markdown" {
		document, err := LoadMarkdownExportDocument(ref.Notebook, ref.Path)
		if err != nil {
			return nil, err
		}
		inspection := InspectMarkdownDocumentID(document.Content)
		if inspection.State != "valid" {
			return nil, errors.New("Markdown document identity is not ready")
		}
		descriptor.DocumentID = inspection.ID
		descriptor.ContentRevision, _ = MarkdownPreviewContentRevision(document.Content)
		descriptor.ResourceRevision = markdownCardResourceRevision(document.Resources)
	} else {
		tree, err := LoadTreeByBlockID(ref.ID)
		if err != nil {
			return nil, err
		}
		descriptor.DocumentID = tree.ID
		data, err := gulu.JSON.MarshalJSON(tree)
		if err != nil {
			return nil, err
		}
		sum := sha256.Sum256(data)
		descriptor.ContentRevision = hex.EncodeToString(sum[:])
		descriptor.ResourceRevision = descriptor.ContentRevision
	}
	canonical, _ := json.Marshal(struct {
		DocumentID       string
		ContentRevision  string
		ResourceRevision string
		Theme            string
		Size             string
		RendererVersion  int
	}{descriptor.DocumentID, descriptor.ContentRevision, descriptor.ResourceRevision, descriptor.Theme,
		descriptor.Size, descriptor.RendererVersion})
	sum := sha256.Sum256(canonical)
	descriptor.CacheKey = hex.EncodeToString(sum[:])
	descriptor.URL = "/card-preview/" + ref.Notebook + "/" + descriptor.CacheKey + ".webp"
	_, err := os.Stat(documentCardPreviewPath(ref.Notebook, descriptor.CacheKey))
	descriptor.Exists = err == nil
	return descriptor, nil
}

func StoreDocumentCardPreview(ref DocumentCardReference, descriptor DocumentCardPreviewDescriptor, reader io.Reader) error {
	current, err := PrepareDocumentCardPreview(ref, descriptor.Theme, descriptor.Size)
	if err != nil {
		return err
	}
	if current.CacheKey != descriptor.CacheKey {
		return ErrMarkdownConflict
	}
	data, err := io.ReadAll(io.LimitReader(reader, 2*1024*1024+1))
	if err != nil {
		return err
	}
	if len(data) > 2*1024*1024 {
		return errors.New("document card preview is too large")
	}
	config, format, err := image.DecodeConfig(bytes.NewReader(data))
	if err != nil {
		return errors.New("document card preview must be WebP")
	}
	if format != "webp" {
		return errors.New("document card preview must be WebP")
	}
	expectedWidth, expectedHeight := 640, 960
	if descriptor.Size == "small" {
		expectedWidth, expectedHeight = 112, 168
	}
	if config.Width != expectedWidth || config.Height != expectedHeight {
		return fmt.Errorf("invalid document card preview dimensions [%dx%d]", config.Width, config.Height)
	}
	target := documentCardPreviewPath(ref.Notebook, descriptor.CacheKey)
	if err = os.MkdirAll(filepath.Dir(target), 0755); err != nil {
		return err
	}
	stage, err := os.CreateTemp(filepath.Dir(target), ".card-preview-*")
	if err != nil {
		return err
	}
	stagePath := stage.Name()
	committed := false
	defer func() {
		_ = stage.Close()
		if !committed {
			_ = os.Remove(stagePath)
		}
	}()
	if _, err = stage.Write(data); err != nil {
		return err
	}
	if err = stage.Sync(); err != nil {
		return err
	}
	if err = stage.Close(); err != nil {
		return err
	}
	if err = os.Rename(stagePath, target); err != nil {
		return err
	}
	committed = true
	go cleanupDocumentCardPreviews()
	return nil
}

func RemoveDocumentCardPreviews(notebook string) {
	if notebook == "" {
		return
	}
	_ = os.RemoveAll(filepath.Join(util.TempDir, "thumbnails", "document-cards", notebook))
}

func DocumentCardPreviewFile(notebook, cacheKey string) (string, error) {
	if !validDocumentCardCacheKey(cacheKey) || Conf.Box(notebook) == nil {
		return "", errors.New("invalid document card preview path")
	}
	if IsEncryptedBox(notebook) && !IsBoxUnlocked(notebook) {
		return "", errors.New("encrypted notebook is locked")
	}
	return documentCardPreviewPath(notebook, cacheKey), nil
}

func markdownCardResourceRevision(resources []MarkdownExportResource) string {
	sort.Slice(resources, func(i, j int) bool { return resources[i].ArchivePath < resources[j].ArchivePath })
	hash := sha256.New()
	for _, resource := range resources {
		_, _ = io.WriteString(hash, resource.ArchivePath+"\x00")
		info, err := os.Stat(resource.SourcePath)
		if err != nil {
			_, _ = io.WriteString(hash, "missing\x00")
			continue
		}
		_, _ = io.WriteString(hash, fmt.Sprintf("%d\x00%d\x00", info.Size(), info.ModTime().UnixNano()))
		if data, readErr := os.ReadFile(resource.SourcePath); readErr == nil {
			sum := sha256.Sum256(data)
			_, _ = hash.Write(sum[:])
		}
	}
	return hex.EncodeToString(hash.Sum(nil))
}

func validNotebookRootReference(ref DocumentCardReference) bool {
	if Conf.Box(ref.Notebook) == nil {
		return false
	}
	if ref.Kind == "markdown" {
		return isMarkdownFileName(filepath.Base(ref.Path)) && !strings.Contains(ref.Path, "..")
	}
	return ref.Kind == "sy" && ref.ID != ""
}

func validDocumentCardCacheKey(key string) bool {
	if len(key) != 64 || strings.ToLower(key) != key {
		return false
	}
	_, err := hex.DecodeString(key)
	return err == nil
}

func documentCardPreviewPath(notebook, cacheKey string) string {
	return filepath.Join(util.TempDir, "thumbnails", "document-cards", notebook, cacheKey+".webp")
}

func cleanupDocumentCardPreviews() {
	const maxSize int64 = 512 * 1024 * 1024
	root := filepath.Join(util.TempDir, "thumbnails", "document-cards")
	type cachedFile struct {
		path  string
		size  int64
		mtime int64
	}
	var files []cachedFile
	var total int64
	_ = filepath.WalkDir(root, func(current string, entry os.DirEntry, err error) error {
		extension := filepath.Ext(current)
		if err != nil || entry.IsDir() || (extension != ".webp" && extension != ".jpg") {
			return nil
		}
		info, infoErr := entry.Info()
		if infoErr == nil {
			files = append(files, cachedFile{path: current, size: info.Size(), mtime: info.ModTime().UnixNano()})
			total += info.Size()
		}
		return nil
	})
	if total <= maxSize {
		return
	}
	sort.Slice(files, func(i, j int) bool { return files[i].mtime < files[j].mtime })
	target := maxSize * 8 / 10
	for _, file := range files {
		if total <= target {
			break
		}
		if os.Remove(file.path) == nil {
			total -= file.size
		}
	}
}
