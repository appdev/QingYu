// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

package model

import (
	"bytes"
	"encoding/json"
	"io/fs"
	"path/filepath"
	"sort"
	"strings"

	"github.com/88250/lute/parse"
	"github.com/BurntSushi/toml"
	"github.com/siyuan-note/filelock"
	"github.com/siyuan-note/siyuan/kernel/util"
	"gopkg.in/yaml.v3"
)

func markdownAssetLinkDests(markdown []byte) []string {
	engine := util.NewStdLute()
	engine.SetLinkRef(true)
	tree := parse.Parse("", markdown, engine.ParseOptions)
	ret := getAssetsLinkDests(tree.Root, false)
	if cover := markdownFrontmatterCover(markdown); strings.HasPrefix(cover, "assets/") {
		ret = append(ret, cover)
	}
	ret = uniqueStrings(ret)
	sort.Strings(ret)
	return ret
}

func markdownFrontmatterCover(markdown []byte) string {
	markdown = bytes.TrimPrefix(markdown, []byte{0xef, 0xbb, 0xbf})
	var metadata map[string]any
	if bytes.HasPrefix(markdown, []byte("---\n")) || bytes.HasPrefix(markdown, []byte("---\r\n")) {
		content, ok := fencedMarkdownFrontmatter(markdown, "---")
		if !ok || yaml.Unmarshal(content, &metadata) != nil {
			return ""
		}
	} else if bytes.HasPrefix(markdown, []byte("+++\n")) || bytes.HasPrefix(markdown, []byte("+++\r\n")) {
		content, ok := fencedMarkdownFrontmatter(markdown, "+++")
		if !ok || toml.Unmarshal(content, &metadata) != nil {
			return ""
		}
	} else if bytes.HasPrefix(markdown, []byte("{")) {
		decoder := json.NewDecoder(bytes.NewReader(markdown))
		if decoder.Decode(&metadata) != nil {
			return ""
		}
	} else {
		return ""
	}
	cover, _ := metadata["cover"].(string)
	return strings.TrimSpace(cover)
}

func fencedMarkdownFrontmatter(markdown []byte, delimiter string) ([]byte, bool) {
	lines := bytes.SplitAfter(markdown, []byte("\n"))
	if len(lines) < 2 || strings.TrimSpace(string(lines[0])) != delimiter {
		return nil, false
	}
	contentEnd := len(lines[0])
	for _, line := range lines[1:] {
		if strings.TrimSpace(string(line)) == delimiter {
			return markdown[len(lines[0]):contentEnd], true
		}
		contentEnd += len(line)
	}
	return nil, false
}

func uniqueStrings(values []string) []string {
	seen := map[string]struct{}{}
	ret := make([]string, 0, len(values))
	for _, value := range values {
		if _, ok := seen[value]; ok {
			continue
		}
		seen[value] = struct{}{}
		ret = append(ret, value)
	}
	return ret
}

func collectMarkdownAssetLinkDests(notebookDir string) (ret map[string]bool, walkErr error) {
	ret = map[string]bool{}
	walkErr = filelock.Walk(notebookDir, func(localPath string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if entry.IsDir() {
			if localPath != notebookDir && (strings.HasPrefix(entry.Name(), ".") || entry.Name() == "assets") {
				return filepath.SkipDir
			}
			return nil
		}
		ext := strings.ToLower(filepath.Ext(entry.Name()))
		if ext != ".md" && ext != ".markdown" {
			return nil
		}
		data, readErr := filelock.ReadFile(localPath)
		if readErr != nil {
			return readErr
		}
		for _, dest := range markdownAssetLinkDests(data) {
			ret[dest] = true
		}
		return nil
	})
	return
}
