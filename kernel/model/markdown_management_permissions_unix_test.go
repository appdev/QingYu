//go:build !windows && !js && !wasip1

package model

import (
	"fmt"
	"os"
	"path/filepath"
	"syscall"
	"testing"

	"github.com/siyuan-note/siyuan/kernel/util"
)

func TestCreateMarkdownChmodsAfterRestrictiveUmask(t *testing.T) {
	for _, mask := range []int{0666, 0777} {
		t.Run(fmt.Sprintf("%#o", mask), func(t *testing.T) {
			box := setupMarkdownTest(t)
			oldMask := syscall.Umask(mask)
			t.Cleanup(func() { syscall.Umask(oldMask) })
			document, err := CreateMarkdown(box.ID, "/", "mode.md")
			if err != nil {
				t.Fatal(err)
			}
			info, err := os.Stat(filepath.Join(util.DataDir, box.ID, "mode.md"))
			if err != nil {
				t.Fatal(err)
			}
			if got := info.Mode().Perm(); got != 0644 {
				t.Fatalf("created mode = %#o", got)
			}
			_ = document
		})
	}
}
