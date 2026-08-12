// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

package model

import (
	"io/fs"
	"path/filepath"
	"sort"
	"strings"

	"github.com/88250/lute/parse"
	"github.com/siyuan-note/filelock"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func markdownAssetLinkDests(markdown []byte) []string {
	engine := util.NewStdLute()
	engine.SetLinkRef(true)
	tree := parse.Parse("", markdown, engine.ParseOptions)
	ret := getAssetsLinkDests(tree.Root, false)
	sort.Strings(ret)
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
