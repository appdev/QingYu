// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

package tools

import (
	"archive/zip"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/88250/gulu"
	"github.com/siyuan-note/siyuan/kernel/util"
	"golang.org/x/text/encoding/simplifiedchinese"
)

const (
	maxArchiveEntries    = 10000
	maxArchiveEntryBytes = uint64(256 << 20)
	maxArchiveTotalBytes = uint64(1 << 30)
)

// UnzipTool 提供工作区内 zip 文件解压能力。
// zipPath 和 destPath 均为工作区相对路径，通过 resolvePath 校验防逃逸。
// 解压是写操作，自动触发 UI 确认和仓库快照（不在 safeActions 中）。
var UnzipTool = &Tool{
	Name:        "unzip",
	Description: "Extract a zip archive within the workspace. Provide the workspace-relative path to the zip file and the destination directory (also workspace-relative). The destination will be created if it does not exist.",
	InputSchema: ToolSchema{
		Type: "object",
		Properties: map[string]Property{
			"zipPath": {
				Type:        "string",
				Description: "Workspace-relative path to the .zip file to extract.",
			},
			"destPath": {
				Type:        "string",
				Description: "Workspace-relative destination directory to extract into.",
			},
		},
		Required: []string{"zipPath", "destPath"},
	},
	Handler: unzipHandler,
}

func init() {
	register(UnzipTool)
}

func unzipHandler(args map[string]any) (CallToolResult, error) {
	zipPath, _ := args["zipPath"].(string)
	destPath, _ := args["destPath"].(string)
	if zipPath == "" {
		return CallToolResult{Content: []ContentItem{{Type: "text", Text: "zipPath is required"}}, IsError: true}, nil
	}
	if destPath == "" {
		return CallToolResult{Content: []ContentItem{{Type: "text", Text: "destPath is required"}}, IsError: true}, nil
	}

	// 解析并校验路径在工作区内（防逃逸），复用 file 工具的 resolvePath。
	zipAbs, err := resolvePath(zipPath)
	if err != nil {
		return CallToolResult{Content: []ContentItem{{Type: "text", Text: "invalid zipPath: " + err.Error()}}, IsError: true}, nil
	}
	destAbs, err := resolvePath(destPath)
	if err != nil {
		return CallToolResult{Content: []ContentItem{{Type: "text", Text: "invalid destPath: " + err.Error()}}, IsError: true}, nil
	}

	// 检查 zip 文件存在。
	if !gulu.File.IsExist(zipAbs) {
		return CallToolResult{Content: []ContentItem{{Type: "text", Text: "zip file not found: " + zipPath}}, IsError: true}, nil
	}

	// 逐条校验归档成员的最终路径，避免 zip-slip、符号链接和敏感文件覆盖。
	if err := extractGuardedArchive(zipAbs, destAbs); err != nil {
		return CallToolResult{Content: []ContentItem{{Type: "text", Text: "unzip failed: " + err.Error()}}, IsError: true}, nil
	}

	return CallToolResult{
		Content: []ContentItem{{Type: "text", Text: fmt.Sprintf("Extracted %s to %s", zipPath, destPath)}},
	}, nil
}

// extractGuardedArchive 在解压前后都校验成员路径，防止归档内容逃逸到目标目录之外。
func extractGuardedArchive(zipAbs, destAbs string) error {
	reader, err := zip.OpenReader(zipAbs)
	if err != nil {
		return err
	}
	defer reader.Close()
	if err = validateArchiveLimits(reader.File); err != nil {
		return err
	}

	type archiveMember struct {
		name string
		path string
	}
	members := make([]archiveMember, len(reader.File))
	var extractedTotal int64
	for i, entry := range reader.File {
		name := entry.Name
		if !utf8.ValidString(name) {
			name, err = simplifiedchinese.GB18030.NewDecoder().String(name)
			if err != nil {
				return err
			}
		}
		name = strings.ReplaceAll(name, "\\", "/")
		if !filepath.IsLocal(filepath.FromSlash(name)) || entry.Mode()&os.ModeSymlink != 0 {
			return fmt.Errorf("invalid archive entry [%s]", name)
		}
		members[i] = archiveMember{name: name, path: filepath.Join(destAbs, filepath.FromSlash(name))}
		if err = authorizeArchiveEntry(destAbs, members[i].path, members[i].name); err != nil {
			return err
		}
	}

	for i, entry := range reader.File {
		if err = authorizeArchiveEntry(destAbs, members[i].path, members[i].name); err != nil {
			return err
		}
		if err = extractArchiveEntry(entry, members[i].path, &extractedTotal); err != nil {
			return err
		}
	}
	return nil
}

func validateArchiveLimits(entries []*zip.File) error {
	if len(entries) > maxArchiveEntries {
		return fmt.Errorf("archive contains too many entries: %d", len(entries))
	}
	var totalSize uint64
	for _, entry := range entries {
		if entry.UncompressedSize64 > maxArchiveEntryBytes {
			return fmt.Errorf("archive entry is too large [%s]", entry.Name)
		}
		if totalSize > maxArchiveTotalBytes-entry.UncompressedSize64 {
			return fmt.Errorf("archive expands beyond the total size limit")
		}
		totalSize += entry.UncompressedSize64
	}
	return nil
}

func authorizeArchiveEntry(destAbs, entryAbs, display string) error {
	rel, err := filepath.Rel(destAbs, entryAbs)
	if err != nil || !filepath.IsLocal(rel) {
		return fmt.Errorf("archive entry escapes destination [%s]", display)
	}
	resolved := util.ResolveLongestExistingParent(entryAbs)
	resolvedDest := util.ResolveLongestExistingParent(destAbs)
	if rel, err = filepath.Rel(resolvedDest, resolved); err != nil || !filepath.IsLocal(rel) {
		return fmt.Errorf("archive entry resolves outside destination [%s]", display)
	}
	if !gulu.File.IsSubPath(util.WorkspaceDir, entryAbs) {
		return fmt.Errorf("archive entry escapes workspace [%s]", display)
	}
	if boxID, encrypted := rejectEncryptedPath(entryAbs); encrypted {
		return fmt.Errorf("archive entry belongs to encrypted notebook [%s]", boxID)
	}
	if filepath.Clean(entryAbs) == filepath.Clean(filepath.Join(util.ConfDir, "conf.json")) {
		return fmt.Errorf("archive entry targets conf.json [%s]", display)
	}
	return nil
}

func extractArchiveEntry(entry *zip.File, destination string, extractedTotal *int64) error {
	if entry.FileInfo().IsDir() {
		return os.MkdirAll(destination, 0755)
	}
	if err := os.MkdirAll(filepath.Dir(destination), 0755); err != nil {
		return err
	}
	source, err := entry.Open()
	if err != nil {
		return err
	}
	defer source.Close()
	remainingTotal := int64(maxArchiveTotalBytes) - *extractedTotal
	if remainingTotal <= 0 {
		return fmt.Errorf("archive expands beyond the total size limit")
	}
	target, err := os.Create(destination)
	if err != nil {
		return err
	}
	limit := int64(maxArchiveEntryBytes) + 1
	if remainingTotal+1 < limit {
		limit = remainingTotal + 1
	}
	limitedSource := io.LimitReader(source, limit)
	written, copyErr := io.Copy(target, limitedSource)
	if copyErr == nil && (written > int64(maxArchiveEntryBytes) || written > remainingTotal) {
		copyErr = fmt.Errorf("archive entry exceeds the size limit")
	}
	closeErr := target.Close()
	if copyErr != nil {
		_ = os.Remove(destination)
		return copyErr
	}
	if closeErr != nil {
		return closeErr
	}
	*extractedTotal += written
	return os.Chtimes(destination, entry.Modified, entry.Modified)
}
