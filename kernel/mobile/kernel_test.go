package mobile

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/siyuan-note/siyuan/kernel/util"
)

func TestUpdateLocalIPs(t *testing.T) {
	original := util.LocalIPs
	t.Cleanup(func() { util.LocalIPs = original })

	UpdateLocalIPs("192.168.1.2,10.0.0.2")
	if expected := []string{"192.168.1.2", "10.0.0.2"}; !reflect.DeepEqual(expected, util.LocalIPs) {
		t.Fatalf("expected local IPs %v, got %v", expected, util.LocalIPs)
	}
}

func TestLanSyncActive(t *testing.T) {
	if LanSyncActive() {
		t.Fatal("LAN sync must remain disabled")
	}
}

func TestAcquireExportFile(t *testing.T) {
	originalTempDir := util.TempDir
	t.Cleanup(func() { util.TempDir = originalTempDir })
	util.TempDir = t.TempDir()

	exportDir := filepath.Join(util.TempDir, "export")
	if err := os.MkdirAll(exportDir, 0o755); err != nil {
		t.Fatal(err)
	}
	filePath := filepath.Join(exportDir, "note.zip")
	if err := os.WriteFile(filePath, []byte("qingyu"), 0o600); err != nil {
		t.Fatal(err)
	}

	leaseJSON := AcquireExportFile("/export/note.zip")
	if "" == leaseJSON {
		t.Fatal("expected an export file lease")
	}
	lease := &exportFileLease{}
	if err := json.Unmarshal([]byte(leaseJSON), lease); err != nil {
		t.Fatal(err)
	}
	if lease.Path != filePath || lease.Name != "note.zip" || lease.Size != 6 || lease.LeaseID == "" {
		t.Fatalf("unexpected export file lease: %+v", lease)
	}

	ReleaseExportFile(lease.LeaseID)
	exportFileLeases.Lock()
	_, exists := exportFileLeases.boxIDs[lease.LeaseID]
	exportFileLeases.Unlock()
	if exists {
		t.Fatal("released export file lease remained registered")
	}
}
