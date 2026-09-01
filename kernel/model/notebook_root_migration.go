// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package model

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/88250/gulu"
	"github.com/siyuan-note/siyuan/kernel/filesys"
	"github.com/siyuan-note/siyuan/kernel/treenode"
	"github.com/siyuan-note/siyuan/kernel/util"
	"gopkg.in/yaml.v3"
)

type NotebookRootMigrationResult struct {
	State   string   `json:"state"`
	Targets []string `json:"targets,omitempty"`
}

type notebookRootMigrationMarker struct {
	Version int      `json:"version"`
	Sources []string `json:"sources"`
	Targets []string `json:"targets"`
	Updated int64    `json:"updated"`
}

const notebookRootMigrationMarkerPath = ".qingyu/notebook-root-migration-v2.json"

func MigrateLegacyNotebookRootContent(boxID string) (*NotebookRootMigrationResult, error) {
	if err := validateNotebookHomeBox(boxID); err != nil {
		return nil, err
	}
	if IsEncryptedBox(boxID) {
		if _, err := GetDEKIfUnlocked(boxID); err != nil {
			return nil, err
		}
	}

	var sources []string
	legacyPath := boxDocPath(boxID)
	absLegacyPath := filepath.Join(util.DataDir, boxID, filepath.FromSlash(strings.TrimPrefix(legacyPath, "/")))
	if _, err := os.Stat(absLegacyPath); err == nil {
		tree, loadErr := filesys.LoadTree(boxID, legacyPath, NewLute())
		if loadErr != nil {
			return nil, loadErr
		}
		if notebookHomeTreeHasEffectiveContent(tree) {
			if markdown := strings.TrimSpace(treenode.ExportNodeStdMd(tree.Root, NewLute())); markdown != "" {
				sources = append(sources, markdown+"\n")
			}
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	if home, err := GetNotebookHome(boxID); err == nil {
		if home.Exists && strings.TrimSpace(home.Content) != "" {
			sources = append(sources, home.Content)
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}

	unique := make([]string, 0, len(sources))
	seen := map[string]bool{}
	var sourceHashes []string
	for _, source := range sources {
		sum := sha256.Sum256([]byte(source))
		hash := hex.EncodeToString(sum[:])
		if seen[hash] {
			continue
		}
		seen[hash] = true
		unique = append(unique, source)
		sourceHashes = append(sourceHashes, hash)
	}
	if len(unique) == 0 {
		return &NotebookRootMigrationResult{State: "empty"}, writeNotebookRootMigrationMarker(boxID,
			&notebookRootMigrationMarker{Version: 2, Sources: sourceHashes, Updated: time.Now().UnixMilli()})
	}

	if data, err := ReadNotebookInternalFile(boxID, notebookRootMigrationMarkerPath); err == nil {
		marker := &notebookRootMigrationMarker{}
		if gulu.JSON.UnmarshalJSON(data, marker) == nil && marker.Version == 2 && strings.Join(marker.Sources, "\x00") == strings.Join(sourceHashes, "\x00") {
			return &NotebookRootMigrationResult{State: "migrated", Targets: marker.Targets}, nil
		}
	}

	boxName := boxID
	if box := Conf.Box(boxID); box != nil && strings.TrimSpace(box.Name) != "" {
		boxName = strings.TrimSpace(box.Name)
	}
	baseName := sanitizeNotebookRootMigrationName(boxName)
	marker := &notebookRootMigrationMarker{Version: 2, Sources: sourceHashes, Updated: time.Now().UnixMilli()}
	for index, source := range unique {
		name := baseName + ".md"
		if index > 0 {
			name = baseName + "-recovered-" + sourceHashes[index][:8] + ".md"
		}
		if _, err := os.Stat(filepath.Join(util.DataDir, boxID, name)); err == nil {
			name = baseName + "-recovered-" + sourceHashes[index][:8] + ".md"
		} else if !errors.Is(err, os.ErrNotExist) {
			return nil, err
		}
		metadata, err := yaml.Marshal(map[string]string{"title": boxName})
		if err != nil {
			return nil, err
		}
		candidate := append([]byte("---\n"), metadata...)
		candidate = append(candidate, []byte("---\n\n"+source)...)
		identity, err := EnsureMarkdownDocumentID(candidate, false)
		if err != nil {
			return nil, err
		}
		created, err := CreateMarkdownWithOperationID(boxID, "/", name, false, "root-migration-v2-create")
		if err != nil {
			return nil, err
		}
		saved, err := SaveMarkdownWithOperationID(boxID, created.Path, string(identity.Data), created.Revision, "root-migration-v2-save")
		if err != nil {
			return nil, err
		}
		readBack, err := GetMarkdown(boxID, saved.Path)
		if err != nil {
			return nil, err
		}
		expected, _ := MarkdownPreviewContentRevision(identity.Data)
		actual, _ := MarkdownPreviewContentRevision([]byte(readBack.Content))
		if expected != actual {
			return nil, errors.New("notebook root migration verification failed")
		}
		marker.Targets = append(marker.Targets, saved.Path)
	}
	if err := writeNotebookRootMigrationMarker(boxID, marker); err != nil {
		return nil, err
	}
	return &NotebookRootMigrationResult{State: "migrated", Targets: marker.Targets}, nil
}

func sanitizeNotebookRootMigrationName(name string) string {
	name = strings.TrimSpace(strings.Map(func(r rune) rune {
		if strings.ContainsRune(`/\\:*?"<>|`, r) || r == '\n' || r == '\r' || r == '\t' {
			return '-'
		}
		return r
	}, name))
	if name == "" || util.IsReservedFilename(name) {
		return "QingYu-recovered"
	}
	return name
}

func writeNotebookRootMigrationMarker(boxID string, marker *notebookRootMigrationMarker) error {
	data, err := gulu.JSON.MarshalIndentJSON(marker, "", "  ")
	if err != nil {
		return err
	}
	return WriteNotebookInternalFile(boxID, notebookRootMigrationMarkerPath, data)
}
