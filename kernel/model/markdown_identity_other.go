//go:build !unix && !windows

package model

import "os"

func markdownPlatformFileIdentity(_ *os.File, _ os.FileInfo) string {
	return ""
}
