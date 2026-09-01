// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

package model

import (
	"html"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/gin-gonic/gin"
	ksql "github.com/siyuan-note/siyuan/kernel/sql"
)

type NotebookHomeSearchResult struct {
	Kind     string `json:"kind"`
	Notebook string `json:"notebook"`
	Title    string `json:"title"`
	Snippet  string `json:"snippet"`
	Updated  int64  `json:"updated"`
}

func FilterNotebookHomeSearchResultsByPublishAccess(c *gin.Context, publishAccess PublishAccess,
	homes []*NotebookHomeSearchResult) (ret []*NotebookHomeSearchResult) {
	for _, home := range homes {
		if CheckNotebookHomeAccessableByPublishAccess(c, publishAccess, home.Notebook) {
			ret = append(ret, home)
		}
	}
	return
}

func CheckNotebookHomeAccessableByPublishAccess(c *gin.Context, publishAccess PublishAccess, boxID string) bool {
	publishIgnore := GetInvisiblePublishAccess(publishAccess)
	passwordID, password := GetPathPasswordByPublishAccess(boxID, "/", publishAccess)
	return CheckPathAccessableByPublishIgnore(boxID, "/", publishIgnore) &&
		(password == "" || CheckPublishAuthCookie(c, passwordID, password))
}

func SearchNotebookHomes(query string, boxes []string, method int) ([]*NotebookHomeSearchResult, error) {
	if strings.TrimSpace(query) == "" || method == 2 || nil == Conf || !IsBoxDocEnabled() {
		return nil, nil
	}
	allowed := map[string]bool{}
	for _, boxID := range boxes {
		allowed[boxID] = true
	}
	var ret []*NotebookHomeSearchResult
	for _, box := range Conf.GetOpenedBoxes() {
		if len(allowed) > 0 && !allowed[box.ID] {
			continue
		}
		if IsEncryptedBox(box.ID) && !IsBoxUnlocked(box.ID) {
			continue
		}
		rows, err := ksql.SearchNotebookHomesInBox(box.ID, query, method)
		if err != nil {
			return nil, err
		}
		for _, row := range rows {
			ret = append(ret, &NotebookHomeSearchResult{
				Kind: "notebook-home", Notebook: row.Box, Title: row.Title,
				Snippet: notebookHomeSnippet(row.Content, query), Updated: row.Updated,
			})
		}
	}
	sort.SliceStable(ret, func(i, j int) bool { return ret[i].Updated > ret[j].Updated })
	return ret, nil
}

func notebookHomeSnippet(content, query string) string {
	content = strings.TrimSpace(content)
	if content == "" {
		return ""
	}
	query = strings.TrimSpace(query)
	lowerContent, lowerQuery := strings.ToLower(content), strings.ToLower(query)
	index := strings.Index(content, query)
	if index < 0 && len(lowerContent) == len(content) && len(lowerQuery) == len(query) {
		index = strings.Index(lowerContent, lowerQuery)
	}
	if index < 0 {
		index = 0
	}
	start := index - 64
	if start < 0 {
		start = 0
	}
	for start > 0 && !utf8.RuneStart(content[start]) {
		start--
	}
	end := start + 192
	if end > len(content) {
		end = len(content)
	}
	for end < len(content) && !utf8.RuneStart(content[end]) {
		end--
	}
	rawSnippet := strings.ReplaceAll(content[start:end], "\n", " ")
	matchStart := strings.Index(rawSnippet, query)
	if matchStart < 0 {
		lowerSnippet := strings.ToLower(rawSnippet)
		if len(lowerSnippet) == len(rawSnippet) && len(lowerQuery) == len(query) {
			matchStart = strings.Index(lowerSnippet, lowerQuery)
		}
	}
	var snippet string
	if lowerQuery != "" && matchStart >= 0 {
		matchEnd := matchStart + len(query)
		if matchEnd <= len(rawSnippet) {
			snippet = html.EscapeString(rawSnippet[:matchStart]) + "<mark>" +
				html.EscapeString(rawSnippet[matchStart:matchEnd]) + "</mark>" + html.EscapeString(rawSnippet[matchEnd:])
		}
	}
	if snippet == "" {
		snippet = html.EscapeString(rawSnippet)
	}
	if start > 0 {
		snippet = "..." + snippet
	}
	if end < len(content) {
		snippet += "..."
	}
	return snippet
}
