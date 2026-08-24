//go:build windows

package model

import (
	"fmt"
	"os"

	"golang.org/x/sys/windows"
)

func markdownPlatformFileIdentity(file *os.File, _ os.FileInfo) string {
	var info windows.ByHandleFileInformation
	if err := windows.GetFileInformationByHandle(windows.Handle(file.Fd()), &info); err != nil {
		return ""
	}
	return fmt.Sprintf("%d:%d:%d", info.VolumeSerialNumber, info.FileIndexHigh, info.FileIndexLow)
}
