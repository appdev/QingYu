// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package model

import (
	"errors"
	"math"
	"path/filepath"
	"sort"
	"strings"
	"unicode"

	"github.com/88250/lute/ast"
	"github.com/88250/lute/parse"
	"github.com/siyuan-note/siyuan/kernel/sql"
	"github.com/siyuan-note/siyuan/kernel/util"
)

const notebookRootPreviewTextLimit = 160

type NotebookRootListing struct {
	Notebook  string                  `json:"notebook"`
	Name      string                  `json:"name"`
	Icon      string                  `json:"icon"`
	SortMode  int                     `json:"sortMode"`
	Documents []*NotebookRootDocument `json:"documents"`
}

type NotebookRootDocument struct {
	Kind             string  `json:"kind"`
	Notebook         string  `json:"notebook"`
	Path             string  `json:"path"`
	DocumentID       string  `json:"documentID"`
	IdentityState    string  `json:"identityState"`
	IdentityConflict bool    `json:"identityConflict"`
	Revision         string  `json:"revision"`
	CardRatio        float64 `json:"cardRatio"`
	Title            string  `json:"title"`
	PreviewText      string  `json:"previewText"`
	Icon             string  `json:"icon"`
	Created          int64   `json:"created"`
	Updated          int64   `json:"updated"`
	Size             uint64  `json:"size"`
	Sort             int     `json:"sort"`
	SubFileCount     int     `json:"subFileCount"`
}

func ListNotebookRootDocuments(boxID string) (*NotebookRootListing, error) {
	box := Conf.Box(boxID)
	if box == nil {
		return nil, errors.New(Conf.Language(0))
	}
	listing, err := listNotebookRootDocuments(box, true)
	if err != nil {
		return nil, err
	}

	byID := map[string][]*NotebookRootDocument{}
	for _, openedBox := range Conf.GetOpenedBoxes() {
		if IsEncryptedBox(openedBox.ID) && !IsBoxUnlocked(openedBox.ID) {
			continue
		}
		openedListing, listErr := listNotebookRootDocuments(openedBox, false)
		if listErr != nil {
			continue
		}
		for _, document := range openedListing.Documents {
			if document.Kind == "markdown" && document.IdentityState == "valid" {
				byID[document.DocumentID] = append(byID[document.DocumentID], document)
			}
		}
	}
	for _, documents := range byID {
		if len(documents) < 2 {
			continue
		}
		sort.Slice(documents, func(i, j int) bool {
			return documents[i].Notebook+"\x00"+documents[i].Path < documents[j].Notebook+"\x00"+documents[j].Path
		})
		for _, duplicate := range documents[1:] {
			if duplicate.Notebook == boxID {
				for _, document := range listing.Documents {
					if document.Path == duplicate.Path {
						document.IdentityConflict = true
					}
				}
			}
		}
	}
	return listing, nil
}

func listNotebookRootDocuments(box *Box, includePreview bool) (*NotebookRootListing, error) {
	files, _, err := ListDocTree(box.ID, "/", util.SortModeUnassigned, false, math.MaxInt)
	if err != nil {
		return nil, err
	}
	listing := &NotebookRootListing{
		Notebook:  box.ID,
		Name:      box.Name,
		Icon:      box.Icon,
		SortMode:  EffectiveFileTreeSortMode(box, util.SortModeUnassigned),
		Documents: make([]*NotebookRootDocument, 0, len(files)),
	}
	nativeDocuments := map[string]*NotebookRootDocument{}
	nativeDocumentIDs := []string{}
	for _, file := range files {
		document := &NotebookRootDocument{
			Kind:          file.DocType,
			Notebook:      box.ID,
			Path:          file.Path,
			DocumentID:    file.ID,
			IdentityState: "valid",
			Title:         file.Name,
			Icon:          file.Icon,
			Created:       file.CTime,
			Updated:       file.Mtime,
			Size:          file.Size,
			Sort:          file.Sort,
			SubFileCount:  file.SubFileCount,
		}
		if file.DocType == "markdown" {
			markdown, getErr := GetMarkdown(box.ID, file.Path)
			if getErr != nil {
				return nil, getErr
			}
			inspection := InspectMarkdownDocumentID([]byte(markdown.Content))
			document.DocumentID = inspection.ID
			document.IdentityState = inspection.State
			document.Revision = markdown.Revision
			if document.DocumentID == "" {
				document.DocumentID = "markdown:" + box.ID + ":" + filepath.ToSlash(file.Path)
			}
			if title := strings.TrimSpace(markdownFrontmatterString([]byte(markdown.Content), "title")); title != "" {
				document.Title = title
			}
			if icon := strings.TrimSpace(markdownFrontmatterString([]byte(markdown.Content), "icon")); icon != "" {
				document.Icon = icon
			}
			if includePreview {
				document.PreviewText = markdownNotebookRootPreviewText([]byte(markdown.Content), document.Title)
			}
		} else if includePreview {
			nativeDocuments[document.DocumentID] = document
			nativeDocumentIDs = append(nativeDocumentIDs, document.DocumentID)
		}
		document.CardRatio = DocumentCardRatio(document.DocumentID)
		listing.Documents = append(listing.Documents, document)
	}
	if includePreview {
		previewTextByID := sql.QueryFirstContentByRootIDsInBox(nativeDocumentIDs, box.ID)
		for documentID, document := range nativeDocuments {
			document.PreviewText = notebookRootPreviewText(previewTextByID[documentID], document.Title)
		}
	}
	return listing, nil
}

func markdownNotebookRootPreviewText(markdown []byte, title string) string {
	engine := util.NewStdLute()
	engine.SetYamlFrontMatter(true)
	tree := parse.Parse("", append([]byte(nil), markdown...), engine.ParseOptions)
	for node := tree.Root.FirstChild; node != nil; node = node.Next {
		if node.Type == ast.NodeYamlFrontMatter {
			continue
		}
		if preview := notebookRootPreviewText(node.Text(), title); preview != "" {
			return preview
		}
	}
	return ""
}

func notebookRootPreviewText(text, title string) string {
	text = strings.Join(strings.Fields(strings.Map(func(r rune) rune {
		if unicode.IsControl(r) {
			return ' '
		}
		return r
	}, text)), " ")
	if text == "" || text == strings.TrimSpace(title) {
		return ""
	}
	runes := []rune(text)
	if len(runes) > notebookRootPreviewTextLimit {
		text = string(runes[:notebookRootPreviewTextLimit])
	}
	return text
}
