// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package model

import (
	"archive/zip"
	"encoding/base64"
	"errors"
	"html"
	"io"
	"mime"
	"net/url"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strings"
	"unicode"

	"github.com/88250/gulu"
	"github.com/88250/lute/ast"
	"github.com/88250/lute/parse"
	"github.com/siyuan-note/filelock"
	"github.com/siyuan-note/siyuan/kernel/util"
)

type MarkdownExportResource struct {
	Raw         string `json:"raw"`
	SourcePath  string `json:"-"`
	ArchivePath string `json:"archivePath"`
	Missing     bool   `json:"missing"`
}

type MarkdownExportDocument struct {
	Notebook  string
	Path      string
	Name      string
	Extension string
	Title     string
	Content   []byte
	Resources []MarkdownExportResource
}

type MarkdownExportArtifact struct {
	Name    string   `json:"name"`
	Path    string   `json:"zip"`
	Missing []string `json:"missing"`
}

var markdownPandocFormats = map[string]string{
	"rst":       ".rst",
	"asciidoc":  ".adoc",
	"textile":   ".textile",
	"opml":      ".opml",
	"org":       ".org",
	"mediawiki": ".wiki",
	"odt":       ".odt",
	"rtf":       ".rtf",
	"epub":      ".epub",
}

func LoadMarkdownExportDocument(boxID, p string) (*MarkdownExportDocument, error) {
	canonicalPath, absPath, err := markdownFilePath(boxID, p)
	if err != nil {
		return nil, err
	}
	if strings.HasPrefix(strings.TrimPrefix(canonicalPath, "/"), ".qingyu/") {
		return nil, ErrInvalidMarkdownPath
	}
	content, err := readMarkdownFileContained(absPath)
	if err != nil {
		return nil, err
	}
	name := path.Base(canonicalPath)
	ext := path.Ext(name)
	title := markdownFrontmatterString(content, "title")
	if title == "" {
		title = strings.TrimSuffix(name, ext)
	}
	doc := &MarkdownExportDocument{
		Notebook:  boxID,
		Path:      canonicalPath,
		Name:      name,
		Extension: ext,
		Title:     title,
		Content:   content,
	}
	doc.Resources, err = markdownExportResources(boxID, canonicalPath, content)
	if err != nil {
		return nil, err
	}
	return doc, nil
}

func markdownExportResources(boxID, documentPath string, content []byte) ([]MarkdownExportResource, error) {
	engine := util.NewStdLute()
	engine.SetLinkRef(true)
	tree := parse.Parse("", append([]byte(nil), content...), engine.ParseOptions)
	dests := []string{}
	ast.Walk(tree.Root, func(node *ast.Node, entering bool) ast.WalkStatus {
		if entering && node.Type == ast.NodeLinkDest {
			dests = append(dests, string(node.Tokens))
		}
		return ast.WalkContinue
	})
	if cover := markdownFrontmatterCover(content); cover != "" {
		dests = append(dests, cover)
	}
	seen := map[string]struct{}{}
	resources := make([]MarkdownExportResource, 0, len(dests))
	for _, raw := range dests {
		resource, local, err := resolveMarkdownExportResource(boxID, documentPath, raw)
		if err != nil {
			return nil, err
		}
		if !local {
			continue
		}
		if _, ok := seen[resource.SourcePath]; ok {
			continue
		}
		seen[resource.SourcePath] = struct{}{}
		resources = append(resources, resource)
	}
	sort.Slice(resources, func(i, j int) bool { return resources[i].ArchivePath < resources[j].ArchivePath })
	return resources, nil
}

func resolveMarkdownExportResource(boxID, documentPath, raw string) (MarkdownExportResource, bool, error) {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" || strings.HasPrefix(trimmed, "#") {
		return MarkdownExportResource{}, false, nil
	}
	for _, r := range trimmed {
		if r == 0 || unicode.IsControl(r) {
			return MarkdownExportResource{}, false, ErrInvalidMarkdownPath
		}
	}
	parsed, err := url.Parse(trimmed)
	if err != nil {
		return MarkdownExportResource{}, false, ErrInvalidMarkdownPath
	}
	if parsed.Scheme != "" {
		scheme := strings.ToLower(parsed.Scheme)
		if scheme == "http" || scheme == "https" || scheme == "data" || scheme == "mailto" {
			return MarkdownExportResource{}, false, nil
		}
		return MarkdownExportResource{}, false, ErrInvalidMarkdownPath
	}
	if parsed.Host != "" {
		return MarkdownExportResource{}, false, ErrInvalidMarkdownPath
	}
	decoded, err := url.PathUnescape(parsed.Path)
	if err != nil || decoded == "" || path.IsAbs(decoded) || filepath.IsAbs(decoded) || strings.Contains(decoded, "\\") {
		return MarkdownExportResource{}, false, ErrInvalidMarkdownPath
	}
	cleaned := path.Clean(decoded)
	if cleaned == ".." || strings.HasPrefix(cleaned, "../") || strings.Contains(cleaned, "/../") {
		return MarkdownExportResource{}, false, ErrInvalidMarkdownPath
	}
	if decodedAgain, decodeErr := url.PathUnescape(decoded); decodeErr != nil {
		return MarkdownExportResource{}, false, ErrInvalidMarkdownPath
	} else if decodedAgain != decoded {
		recleaned := path.Clean(decodedAgain)
		if recleaned == ".." || strings.HasPrefix(recleaned, "../") || strings.Contains(recleaned, "/../") {
			return MarkdownExportResource{}, false, ErrInvalidMarkdownPath
		}
	}
	archivePath := cleaned
	notebookRelative := cleaned
	if !strings.HasPrefix(cleaned, "assets/") {
		notebookRelative = path.Join(path.Dir(strings.TrimPrefix(documentPath, "/")), cleaned)
	}
	if strings.HasPrefix(notebookRelative, ".qingyu/") {
		return MarkdownExportResource{}, false, ErrInvalidMarkdownPath
	}
	notebookRoot := filepath.Join(util.DataDir, boxID)
	sourcePath := filepath.Join(notebookRoot, filepath.FromSlash(notebookRelative))
	resourceRoot := notebookRoot
	if strings.HasPrefix(cleaned, "assets/") && !gulu.File.IsExist(sourcePath) && !IsEncryptedBox(boxID) {
		resourceRoot = filepath.Join(util.DataDir, "assets")
		sourcePath = filepath.Join(util.DataDir, filepath.FromSlash(cleaned))
	}
	if _, rootErr := os.Lstat(resourceRoot); os.IsNotExist(rootErr) {
		return MarkdownExportResource{Raw: raw, SourcePath: sourcePath, ArchivePath: archivePath, Missing: true}, true, nil
	} else if rootErr != nil {
		return MarkdownExportResource{}, false, rootErr
	}
	if err = validatePathWithoutSymlinks(resourceRoot, sourcePath, true); err != nil {
		return MarkdownExportResource{}, false, err
	}
	info, statErr := os.Lstat(sourcePath)
	missing := os.IsNotExist(statErr)
	if statErr != nil && !missing {
		return MarkdownExportResource{}, false, statErr
	}
	if !missing && !info.Mode().IsRegular() {
		return MarkdownExportResource{}, false, ErrInvalidMarkdownPath
	}
	return MarkdownExportResource{Raw: raw, SourcePath: sourcePath, ArchivePath: archivePath, Missing: missing}, true, nil
}

func (doc *MarkdownExportDocument) Stage(root string) ([]string, error) {
	if err := os.MkdirAll(root, 0755); err != nil {
		return nil, err
	}
	if err := os.WriteFile(filepath.Join(root, doc.Name), doc.Content, 0644); err != nil {
		return nil, err
	}
	return doc.stageResources(root)
}

func (doc *MarkdownExportDocument) stageResources(root string) ([]string, error) {
	missing := []string{}
	for _, resource := range doc.Resources {
		if resource.Missing {
			missing = append(missing, resource.Raw)
			continue
		}
		target := filepath.Join(root, filepath.FromSlash(resource.ArchivePath))
		if err := os.MkdirAll(filepath.Dir(target), 0755); err != nil {
			return nil, err
		}
		if err := copyMarkdownExportFile(resource.SourcePath, target); err != nil {
			return nil, err
		}
	}
	return missing, nil
}

func copyMarkdownExportFile(source, target string) error {
	input, root, err := openMarkdownFileRead(source)
	if err != nil {
		return err
	}
	defer root.Close()
	defer input.Close()
	output, err := os.OpenFile(target, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0644)
	if err != nil {
		return err
	}
	_, copyErr := io.Copy(output, input)
	return errors.Join(copyErr, output.Close())
}

func ExportMarkdownDocumentZip(boxID, p string) (*MarkdownExportArtifact, error) {
	doc, err := LoadMarkdownExportDocument(boxID, p)
	if err != nil {
		return nil, err
	}
	tmpDir := filepath.Join(util.TempDir, "export", "markdown-"+gulu.Rand.String(7))
	if err = os.MkdirAll(tmpDir, 0755); err != nil {
		return nil, err
	}
	defer os.RemoveAll(tmpDir)
	stageDir := filepath.Join(tmpDir, strings.TrimSuffix(doc.Name, doc.Extension))
	missing, err := doc.Stage(stageDir)
	if err != nil {
		return nil, err
	}
	zipName := strings.TrimSuffix(doc.Name, doc.Extension) + ".zip"
	zipPath := filepath.Join(util.TempDir, "export", zipName)
	zipPath = util.GetUniqueFilename(zipPath)
	if err = zipDirectory(stageDir, zipPath); err != nil {
		return nil, err
	}
	return &MarkdownExportArtifact{Name: zipName, Path: "/export/" + url.PathEscape(filepath.Base(zipPath)), Missing: missing}, nil
}

func zipDirectory(root, output string) error {
	file, err := os.Create(output)
	if err != nil {
		return err
	}
	writer := zip.NewWriter(file)
	walkErr := filepath.Walk(root, func(current string, info os.FileInfo, walkErr error) error {
		if walkErr != nil || info.IsDir() {
			return walkErr
		}
		rel, relErr := filepath.Rel(root, current)
		if relErr != nil {
			return relErr
		}
		entry, createErr := writer.Create(filepath.ToSlash(rel))
		if createErr != nil {
			return createErr
		}
		input, openErr := os.Open(current)
		if openErr != nil {
			return openErr
		}
		_, copyErr := io.Copy(entry, input)
		closeErr := input.Close()
		return errors.Join(copyErr, closeErr)
	})
	return errors.Join(walkErr, writer.Close(), file.Close())
}

func SaveMarkdownDocumentAsTemplate(boxID, p, name string, overwrite bool) (int, error) {
	doc, err := LoadMarkdownExportDocument(boxID, p)
	if err != nil {
		return 0, err
	}
	return CreateTemplate(name, string(doc.Content), overwrite)
}

func ExportMarkdownDocumentPreview(boxID, p string) (name, content string, missing []string, err error) {
	return exportMarkdownDocumentPreview(boxID, p, false)
}

func ExportMarkdownDocumentCardPreview(boxID, p string) (name, content string, missing []string, err error) {
	return exportMarkdownDocumentPreview(boxID, p, true)
}

func exportMarkdownDocumentPreview(boxID, p string, hideFrontmatter bool) (name, content string, missing []string, err error) {
	doc, err := LoadMarkdownExportDocument(boxID, p)
	if err != nil {
		return "", "", nil, err
	}
	previewContent := doc.Content
	if hideFrontmatter {
		previewContent = markdownDocumentPreviewContent(previewContent)
	}
	content = MarkdownToProtylePreviewHTML(string(previewContent))
	for _, resource := range doc.Resources {
		if resource.Missing {
			missing = append(missing, resource.Raw)
			continue
		}
		data, readErr := readMarkdownFileContained(resource.SourcePath)
		if readErr != nil {
			return "", "", nil, readErr
		}
		mediaType := mime.TypeByExtension(filepath.Ext(resource.SourcePath))
		if mediaType == "" {
			mediaType = "application/octet-stream"
		}
		dataURL := "data:" + mediaType + ";base64," + base64.StdEncoding.EncodeToString(data)
		content = strings.ReplaceAll(content, resource.Raw, dataURL)
		content = strings.ReplaceAll(content, html.EscapeString(resource.Raw), dataURL)
	}
	return doc.Title, content, missing, nil
}

func markdownDocumentPreviewContent(data []byte) []byte {
	frontmatter := inspectMarkdownDocumentIDFrontmatter(data)
	if !frontmatter.valid || frontmatter.kind == "none" || frontmatter.blockEnd <= frontmatter.bomLen {
		return data
	}
	ret := make([]byte, 0, len(data)-(frontmatter.blockEnd-frontmatter.bomLen))
	ret = append(ret, data[:frontmatter.bomLen]...)
	ret = append(ret, data[frontmatter.blockEnd:]...)
	return ret
}

func ExportMarkdownDocumentHTML(boxID, p, savePath string) (name, content string, missing []string, err error) {
	doc, err := LoadMarkdownExportDocument(boxID, p)
	if err != nil {
		return "", "", nil, err
	}
	if err = os.MkdirAll(savePath, 0755); err != nil {
		return "", "", nil, err
	}
	missing, err = doc.stageResources(savePath)
	if err != nil {
		return "", "", nil, err
	}
	for _, source := range []string{"stage/build/export", "stage/protyle"} {
		if err = filelock.Copy(filepath.Join(util.WorkingDir, source), filepath.Join(savePath, source)); err != nil {
			return "", "", nil, err
		}
	}
	return doc.Title, MarkdownToMarkdownStrHTML(string(doc.Content)), missing, nil
}

func ExportMarkdownDocumentDocx(boxID, p string) (*MarkdownExportArtifact, error) {
	doc, err := LoadMarkdownExportDocument(boxID, p)
	if err != nil {
		return nil, err
	}
	tmpDir := filepath.Join(util.TempDir, "export", "markdown-docx-"+gulu.Rand.String(7))
	if err = os.MkdirAll(tmpDir, 0755); err != nil {
		return nil, err
	}
	defer os.RemoveAll(tmpDir)
	missing, err := doc.Stage(tmpDir)
	if err != nil {
		return nil, err
	}
	outputName := strings.TrimSuffix(doc.Name, doc.Extension) + ".docx"
	outputPath := filepath.Join(util.TempDir, "export", outputName)
	outputPath = util.GetUniqueFilename(outputPath)
	if err = util.PandocWithResourcePath("gfm+footnotes+hard_line_breaks", "docx", outputPath, string(doc.Content), tmpDir); err != nil {
		return nil, err
	}
	return &MarkdownExportArtifact{
		Name:    outputName,
		Path:    "/export/" + url.PathEscape(filepath.Base(outputPath)),
		Missing: missing,
	}, nil
}

func ExportMarkdownDocumentPandoc(boxID, p, format string) (*MarkdownExportArtifact, error) {
	ext, ok := markdownPandocFormats[format]
	if !ok {
		return nil, errors.New("unsupported Markdown export format")
	}
	doc, err := LoadMarkdownExportDocument(boxID, p)
	if err != nil {
		return nil, err
	}
	tmpDir := filepath.Join(util.TempDir, "export", "markdown-pandoc-"+gulu.Rand.String(7))
	if err = os.MkdirAll(tmpDir, 0755); err != nil {
		return nil, err
	}
	defer os.RemoveAll(tmpDir)
	missing, err := doc.Stage(tmpDir)
	if err != nil {
		return nil, err
	}
	outputName := strings.TrimSuffix(doc.Name, doc.Extension) + ext
	outputPath := filepath.Join(tmpDir, outputName)
	if err = util.PandocWithResourcePath("gfm+footnotes+hard_line_breaks", format, outputPath, string(doc.Content), tmpDir); err != nil {
		return nil, err
	}
	if err = os.Remove(filepath.Join(tmpDir, doc.Name)); err != nil {
		return nil, err
	}
	zipPath := util.GetUniqueFilename(filepath.Join(util.TempDir, "export", outputName+".zip"))
	if err = zipDirectory(tmpDir, zipPath); err != nil {
		return nil, err
	}
	return &MarkdownExportArtifact{Name: outputName, Path: "/export/" + url.PathEscape(filepath.Base(zipPath)), Missing: missing}, nil
}
