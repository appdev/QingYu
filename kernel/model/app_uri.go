package model

import "strings"

const (
	qingYuBlockURIPrefix       = "qingyu://blocks/"
	legacySiYuanBlockURIPrefix = "siyuan://blocks/"
)

func trimAppBlockURIPrefix(uri string) (id string, ok bool) {
	if id, ok = strings.CutPrefix(uri, qingYuBlockURIPrefix); ok {
		return
	}
	if id, ok = strings.CutPrefix(uri, legacySiYuanBlockURIPrefix); ok {
		return
	}
	return "", false
}

func appBlockURI(id string) string {
	return qingYuBlockURIPrefix + id
}
