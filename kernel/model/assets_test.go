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

package model

import (
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/88250/lute/ast"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func TestClearWorkspaceTempPreservesInstallPackages(t *testing.T) {
	originalDataDir, originalTempDir, originalWorkspaceDir := util.DataDir, util.TempDir, util.WorkspaceDir
	t.Cleanup(func() {
		util.DataDir, util.TempDir, util.WorkspaceDir = originalDataDir, originalTempDir, originalWorkspaceDir
	})
	root := t.TempDir()
	util.DataDir = filepath.Join(root, "data")
	util.TempDir = filepath.Join(root, "temp")
	util.WorkspaceDir = root
	installPkgPath := filepath.Join(util.TempDir, "install", "siyuan-test-win.exe")
	if err := os.MkdirAll(filepath.Dir(installPkgPath), 0755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(installPkgPath, []byte("test"), 0644); err != nil {
		t.Fatal(err)
	}
	oldTime := time.Now().Add(-8 * 24 * time.Hour)
	if err := os.Chtimes(installPkgPath, oldTime, oldTime); err != nil {
		t.Fatal(err)
	}

	clearWorkspaceTemp(true)
	if _, err := os.Stat(installPkgPath); err != nil {
		t.Fatalf("install package should be preserved during update: %v", err)
	}
	clearWorkspaceTemp(false)
	if _, err := os.Stat(installPkgPath); !os.IsNotExist(err) {
		t.Fatalf("old install package should be removed during normal exit: %v", err)
	}
}

func TestNormalizeMissingAssetLinkDest(t *testing.T) {
	tests := []struct {
		name string
		dest string
		want string
	}{
		{name: "asset", dest: "assets/image.png", want: "assets/image.png"},
		{name: "query", dest: "assets/document.pdf?page=2", want: "assets/document.pdf"},
		{name: "folder", dest: "assets/images/", want: ""},
		{name: "rtfd", dest: "assets/document.rtfd", want: ""},
		{name: "pdf annotation", dest: "assets/document.pdf/20200101000000-abcdefg", want: ""},
		{name: "external", dest: "https://example.com/image.png", want: ""},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := normalizeMissingAssetLinkDest(test.dest); got != test.want {
				t.Fatalf("normalize missing asset link destination: got %q, want %q", got, test.want)
			}
		})
	}
}

func TestMarkdownAssetLinkDests(t *testing.T) {
	markdown := []byte(`---
cover: assets/cover.png
---

![标准图片](assets/image.png)

[普通资源](assets/document.pdf?page=2)

![引用图片][diagram]

[diagram]: assets/diagram%20one.png

<img src="assets/html.png">

` + "`assets/inline-code.png`" + `

~~~markdown
![代码示例](assets/fenced-code.png)
~~~

![网络图片](https://example.com/remote.png)
`)

	got := markdownAssetLinkDests(markdown)
	want := []string{
		"assets/cover.png",
		"assets/diagram%20one.png",
		"assets/document.pdf?page=2",
		"assets/html.png",
		"assets/image.png",
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("Markdown asset destinations: got %v, want %v", got, want)
	}
}

func TestMarkdownFrontmatterCoverFormats(t *testing.T) {
	tests := []struct {
		name     string
		markdown string
		want     string
	}{
		{name: "yaml", markdown: "\ufeff---\r\ncover: assets/yaml.png\r\n---\r\nBody", want: "assets/yaml.png"},
		{name: "toml", markdown: "+++\ncover = 'assets/toml.png'\n+++\nBody", want: "assets/toml.png"},
		{name: "json", markdown: "{\n  \"cover\": \"assets/json.png\"\n}\nBody", want: "assets/json.png"},
		{name: "remote", markdown: "---\ncover: https://example.com/cover.png\n---\n", want: ""},
		{name: "malformed", markdown: "---\ncover: assets/missing.png\n", want: ""},
		{name: "body", markdown: "Text\ncover: assets/body.png\n", want: ""},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			got := ""
			for _, dest := range markdownAssetLinkDests([]byte(test.markdown)) {
				if strings.HasPrefix(dest, "assets/") {
					got = dest
				}
			}
			if got != test.want {
				t.Fatalf("front matter cover: got %q, want %q", got, test.want)
			}
		})
	}
}

func TestRemoveReferencedAssetPathsHandlesMarkdownFilesAndFolders(t *testing.T) {
	assets := map[string]string{
		"assets/diagram one.png":   "/data/assets/diagram one.png",
		"assets/image.png":         "/data/assets/image.png",
		"assets/screenshots/":      "/data/assets/screenshots",
		"assets/screenshots/a.png": "/data/assets/screenshots/a.png",
		"assets/unused.png":        "/data/assets/unused.png",
	}
	links := map[string]bool{}
	removeReferencedAssetPaths(assets, map[string]bool{
		"assets/diagram%20one.png":     true,
		"assets/image.png?style=thumb": true,
		"assets/screenshots/":          true,
	}, links)

	wantAssets := map[string]string{"assets/unused.png": "/data/assets/unused.png"}
	if !reflect.DeepEqual(assets, wantAssets) {
		t.Fatalf("remaining assets: got %v, want %v", assets, wantAssets)
	}
	if !links["assets/image.png"] {
		t.Fatalf("normalized file reference should be recorded: %v", links)
	}
}

func TestContainsUnusedAssetRequiresAnExactPath(t *testing.T) {
	items := []*UnusedItem{{Item: "assets/image.png"}}
	if !containsUnusedAsset(items, "assets/image.png") {
		t.Fatal("exact unused asset path should match")
	}
	if containsUnusedAsset(items, "assets/image.png?style=thumb") {
		t.Fatal("a different path must not authorize deletion")
	}
	if containsUnusedAsset(items, "assets/image.png/child") {
		t.Fatal("a path prefix must not authorize deletion")
	}
}

func TestMarkdownUnusedAssetScan(t *testing.T) {
	box := setupMarkdownTest(t)

	assetsDir := filepath.Join(util.DataDir, "assets")
	if err := os.MkdirAll(assetsDir, 0755); err != nil {
		t.Fatal(err)
	}
	referencedPath := filepath.Join(assetsDir, "markdown-referenced.png")
	unusedPath := filepath.Join(assetsDir, "markdown-unused.png")
	for _, assetPath := range []string{referencedPath, unusedPath} {
		if err := os.WriteFile(assetPath, []byte("image"), 0644); err != nil {
			t.Fatal(err)
		}
	}

	first, err := CreateMarkdown(box.ID, "/", "first.md")
	if err != nil {
		t.Fatal(err)
	}
	first, err = SaveMarkdown(box.ID, first.Path, "![image](assets/markdown-referenced.png)\n", first.Revision)
	if err != nil {
		t.Fatal(err)
	}
	second, err := CreateMarkdown(box.ID, "/", "second.md")
	if err != nil {
		t.Fatal(err)
	}
	second, err = SaveMarkdown(box.ID, second.Path, "![shared](assets/markdown-referenced.png)\n", second.Revision)
	if err != nil {
		t.Fatal(err)
	}

	unused, err := UnusedAssetsWithError(false)
	if err != nil {
		t.Fatal(err)
	}
	if containsUnusedAsset(unused, "assets/markdown-referenced.png") {
		t.Fatalf("asset referenced by Markdown must not be reported unused: %+v", unused)
	}
	if !containsUnusedAsset(unused, "assets/markdown-unused.png") {
		t.Fatalf("unreferenced Markdown asset should be reported unused: %+v", unused)
	}
	first, err = SaveMarkdown(box.ID, first.Path, "", first.Revision)
	if err != nil {
		t.Fatal(err)
	}
	unused, err = UnusedAssetsWithError(false)
	if err != nil {
		t.Fatal(err)
	}
	if containsUnusedAsset(unused, "assets/markdown-referenced.png") {
		t.Fatal("asset must stay referenced while another Markdown document still uses it")
	}
	second, err = SaveMarkdown(box.ID, second.Path, "", second.Revision)
	if err != nil {
		t.Fatal(err)
	}
	unused, err = UnusedAssetsWithError(false)
	if err != nil {
		t.Fatal(err)
	}
	if !containsUnusedAsset(unused, "assets/markdown-referenced.png") {
		t.Fatal("asset should become unused after its final Markdown reference is removed")
	}
}

func TestGetAssetAbsPathWithSymlinkedWorkspaceAncestor(t *testing.T) {
	originalDataDir, originalWorkspaceDir := util.DataDir, util.WorkspaceDir
	t.Cleanup(func() {
		util.DataDir, util.WorkspaceDir = originalDataDir, originalWorkspaceDir
	})

	realWorkspaceDir := t.TempDir()
	aliasBaseDir := t.TempDir()
	aliasWorkspaceDir := filepath.Join(aliasBaseDir, "workspace")
	if err := os.Symlink(realWorkspaceDir, aliasWorkspaceDir); err != nil {
		t.Skipf("create workspace symlink failed: %s", err)
	}
	util.WorkspaceDir = aliasWorkspaceDir
	util.DataDir = filepath.Join(aliasWorkspaceDir, "data")

	assetPath := filepath.Join(util.DataDir, "assets", "image.png")
	if err := os.MkdirAll(filepath.Dir(assetPath), 0755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(assetPath, []byte("image"), 0644); err != nil {
		t.Fatal(err)
	}

	got, err := GetAssetAbsPath("assets/image.png")
	if err != nil {
		t.Fatal(err)
	}
	if got != assetPath {
		t.Fatalf("get global asset path: got %q, want %q", got, assetPath)
	}

	outsideDir := t.TempDir()
	outsidePath := filepath.Join(outsideDir, "outside.png")
	if err = os.WriteFile(outsidePath, []byte("outside"), 0644); err != nil {
		t.Fatal(err)
	}
	linkedAssetPath := filepath.Join(util.DataDir, "assets", "outside.png")
	if err = os.Symlink(outsidePath, linkedAssetPath); err != nil {
		t.Skipf("create asset symlink failed: %s", err)
	}
	if _, err = GetAssetAbsPath("assets/outside.png"); err == nil {
		t.Fatal("asset symlink outside data/assets should be rejected")
	}
}

func TestGetAssetLinkDestsByNode(t *testing.T) {
	const blockID = "20200101000000-abcdefg"
	root := &ast.Node{Type: ast.NodeDocument}
	paragraph := &ast.Node{Type: ast.NodeParagraph, ID: blockID}
	paragraph.SetIALAttr("custom-data-assets", "assets/custom.png")
	linkDest := &ast.Node{Type: ast.NodeLinkDest, Tokens: []byte("assets/image.png")}
	root.AppendChild(paragraph)
	paragraph.AppendChild(linkDest)

	want := []string{"assets/custom.png", "assets/image.png"}
	if got := getAssetsLinkDests(root, false); !reflect.DeepEqual(got, want) {
		t.Fatalf("get asset link destinations: got %v, want %v", got, want)
	}
	if got := getAssetLinkDestsByNode(paragraph, false); !reflect.DeepEqual(got, []string{"assets/custom.png"}) {
		t.Fatalf("get block asset link destinations: got %v, want %v", got, []string{"assets/custom.png"})
	}
	if got := getAssetLinkDestsByNode(linkDest, false); !reflect.DeepEqual(got, []string{"assets/image.png"}) {
		t.Fatalf("get inline asset link destinations: got %v, want %v", got, []string{"assets/image.png"})
	}
	if got := assetLinkDestBlockID(linkDest); got != blockID {
		t.Fatalf("get asset link destination block ID: got %q, want %q", got, blockID)
	}
}
