package model

import (
	"encoding/json"
	"errors"
	"strings"

	"github.com/88250/lute/ast"
	"github.com/88250/lute/parse"
	"github.com/siyuan-note/siyuan/kernel/treenode"
	"golang.org/x/net/html"
)

var ErrDatabaseReadOnly = errors.New("databases are read-only")

// 在开始事务前拒绝数据库操作，避免混合事务部分写入普通笔记。
func checkDatabaseOperations(operations []*Operation) error {
	for _, op := range operations {
		if op == nil {
			continue
		}
		if strings.Contains(op.Action, "AttrView") || op.AvID != "" {
			return ErrDatabaseReadOnly
		}
		if data, ok := op.Data.(string); ok && databaseWritePayload(data) {
			return ErrDatabaseReadOnly
		}
		tree := op.Tree
		if data, ok := op.Data.(*parse.Tree); ok {
			tree = data
		}
		if tree != nil {
			found := false
			ast.Walk(tree.Root, func(node *ast.Node, entering bool) ast.WalkStatus {
				if entering && node.Type == ast.NodeAttributeView {
					found = true
					return ast.WalkStop
				}
				return ast.WalkContinue
			})
			if found {
				return ErrDatabaseReadOnly
			}
		}
		if op.Action == "create" {
			if err := checkNativeDocumentCreation(tree); err != nil {
				return err
			}
		}
		for _, id := range []string{op.ID, op.ParentID, op.BlockID} {
			if id == "" {
				continue
			}
			if block := treenode.GetBlockTree(id); block != nil && block.Type == "av" {
				return ErrDatabaseReadOnly
			}
			for _, boxID := range treenode.GetOpenedEncryptedBoxIDs() {
				if block := treenode.GetBlockTreeInBox(id, boxID); block != nil && block.Type == "av" {
					return ErrDatabaseReadOnly
				}
			}
		}
	}
	return nil
}

func databaseWritePayload(data string) bool {
	attrs := map[string]any{}
	if json.Unmarshal([]byte(data), &attrs) == nil {
		for name := range attrs {
			if name == "custom-avs" || strings.HasPrefix(name, "custom-sy-av-") {
				return true
			}
		}
	}
	tokens := html.NewTokenizer(strings.NewReader(data))
	for {
		switch tokens.Next() {
		case html.ErrorToken:
			return false
		case html.StartTagToken, html.SelfClosingTagToken:
			for _, attr := range tokens.Token().Attr {
				if attr.Key == "data-type" && attr.Val == "NodeAttributeView" ||
					attr.Key == "data-av-id" || attr.Key == "av-id" {
					return true
				}
			}
		}
	}
}
