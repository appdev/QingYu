// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

package model

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/88250/gulu"
	"github.com/88250/lute/ast"
	"github.com/88250/lute/parse"
	"github.com/siyuan-note/siyuan/kernel/filesys"
	"github.com/siyuan-note/siyuan/kernel/treenode"
	"github.com/siyuan-note/siyuan/kernel/util"
)

type NotebookHomeMigrationResult struct {
	State        string `json:"state"`
	RecoveryPath string `json:"recoveryPath,omitempty"`
}

type notebookHomeMigrationMetadata struct {
	Spec           int    `json:"spec"`
	Version        int    `json:"version"`
	State          string `json:"state"`
	SourceRevision string `json:"sourceRevision"`
	RecoveryPath   string `json:"recoveryPath,omitempty"`
	Updated        int64  `json:"updated"`
}

func MigrateNotebookHome(boxID string) (*NotebookHomeMigrationResult, error) {
	if err := validateNotebookHomeBox(boxID); err != nil {
		return nil, err
	}
	if IsEncryptedBox(boxID) {
		if _, err := GetDEKIfUnlocked(boxID); err != nil {
			return nil, err
		}
	}
	legacyPath := boxDocPath(boxID)
	absLegacyPath := filepath.Join(util.DataDir, boxID, filepath.FromSlash(strings.TrimPrefix(legacyPath, "/")))
	if _, err := os.Stat(absLegacyPath); err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return &NotebookHomeMigrationResult{}, nil
		}
		return nil, err
	}
	tree, err := filesys.LoadTree(boxID, legacyPath, NewLute())
	if err != nil {
		return nil, err
	}
	sourceData, err := gulu.JSON.MarshalJSON(tree)
	if err != nil {
		return nil, err
	}
	sourceRevision := markdownRevision(sourceData)
	metadata := &notebookHomeMigrationMetadata{
		Spec: 1, Version: 1, SourceRevision: sourceRevision, Updated: time.Now().UnixMilli(),
	}
	if existing, readErr := ReadNotebookInternalFile(boxID, ".qingyu/home.json"); readErr == nil {
		stored := &notebookHomeMigrationMetadata{}
		if unmarshalErr := gulu.JSON.UnmarshalJSON(existing, stored); unmarshalErr == nil && stored.Spec == metadata.Spec &&
			stored.Version == metadata.Version && stored.SourceRevision == sourceRevision {
			return &NotebookHomeMigrationResult{State: stored.State, RecoveryPath: stored.RecoveryPath}, nil
		}
	} else if !errors.Is(readErr, os.ErrNotExist) {
		return nil, readErr
	}
	if !notebookHomeTreeHasEffectiveContent(tree) {
		metadata.State = "empty"
		if err = writeNotebookHomeMigrationMetadata(boxID, metadata); err != nil {
			return nil, err
		}
		IncSync()
		return &NotebookHomeMigrationResult{State: metadata.State}, nil
	}

	markdown := strings.TrimSpace(treenode.ExportNodeStdMd(tree.Root, NewLute()))
	if markdown != "" {
		markdown += "\n"
	}
	if parsed := parse.Parse("", []byte(markdown), NewLute().ParseOptions); nil == parsed || nil == parsed.Root {
		return nil, errors.New("parse migrated notebook home failed")
	}
	home, err := GetNotebookHome(boxID)
	if err != nil {
		return nil, err
	}
	if !home.Exists || strings.TrimSpace(home.Content) == "" {
		if _, err = SaveNotebookHome(boxID, markdown, home.Revision, "migration-v1"); err != nil {
			return nil, err
		}
		metadata.State = "migrated"
	} else if home.Content == markdown {
		metadata.State = "migrated"
	} else {
		hash := sha256.Sum256([]byte(markdown))
		recoveryName := "legacy-box-doc-v1-" + hex.EncodeToString(hash[:])[:16] + ".md"
		metadata.State = "conflict"
		metadata.RecoveryPath = ".qingyu/recovery/" + recoveryName
		if existing, readErr := ReadNotebookInternalFile(boxID, metadata.RecoveryPath); readErr == nil {
			if string(existing) != markdown {
				return nil, errors.New("notebook home recovery content mismatch")
			}
		} else if !errors.Is(readErr, os.ErrNotExist) {
			return nil, readErr
		} else if err = WriteNotebookInternalFile(boxID, metadata.RecoveryPath, []byte(markdown)); err != nil {
			return nil, err
		}
	}
	if err = writeNotebookHomeMigrationMetadata(boxID, metadata); err != nil {
		return nil, err
	}
	IncSync()
	if metadata.State == "conflict" && nil != Conf {
		name := boxID
		if boxConf := (&Box{ID: boxID}).GetConf(); nil != boxConf && boxConf.Name != "" {
			name = boxConf.Name
		}
		util.PushMsg(fmt.Sprintf(Conf.Language(344), name), 0)
	}
	return &NotebookHomeMigrationResult{State: metadata.State, RecoveryPath: metadata.RecoveryPath}, nil
}

func notebookHomeTreeHasEffectiveContent(tree *parse.Tree) bool {
	if nil == tree || nil == tree.Root {
		return false
	}
	effective := false
	ast.Walk(tree.Root, func(node *ast.Node, entering bool) ast.WalkStatus {
		if !entering || node == tree.Root || ast.NodeKramdownBlockIAL == node.Type || ast.NodeKramdownSpanIAL == node.Type {
			return ast.WalkContinue
		}
		if node.IsBlock() && ast.NodeParagraph != node.Type {
			effective = true
			return ast.WalkStop
		}
		if ast.NodeImage == node.Type || ast.NodeBlockRef == node.Type || ast.NodeFileAnnotationRef == node.Type ||
			ast.NodeAttributeView == node.Type || ast.NodeBlockQueryEmbed == node.Type {
			effective = true
			return ast.WalkStop
		}
		text := string(node.Tokens)
		if ast.NodeTextMark == node.Type {
			text = node.TextMarkTextContent
		}
		if strings.TrimSpace(text) != "" {
			effective = true
			return ast.WalkStop
		}
		return ast.WalkContinue
	})
	return effective
}

func writeNotebookHomeMigrationMetadata(boxID string, metadata *notebookHomeMigrationMetadata) error {
	data, err := gulu.JSON.MarshalIndentJSON(metadata, "", "  ")
	if err != nil {
		return err
	}
	return WriteNotebookInternalFile(boxID, ".qingyu/home.json", data)
}
