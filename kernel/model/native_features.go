package model

import (
	"errors"
	"github.com/88250/lute/parse"
)

var ErrNativeDocumentCreationDisabled = errors.New("native document creation is disabled; create a Markdown document")

// 笔记本根节点是内部结构，普通原生文档不能通过新建事务生成。
func checkNativeDocumentCreation(tree *parse.Tree) error {
	if tree != nil && tree.Box != "" && tree.ID == tree.Box {
		return nil
	}
	return ErrNativeDocumentCreationDisabled
}
