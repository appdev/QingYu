package model

import (
	"strings"
	"testing"

	"github.com/88250/gulu"
)

func TestLegacyGraphConfigIsIgnored(t *testing.T) {
	appConf := NewAppConf()
	legacy := []byte(`{"logLevel":"info","graph":{"maxBlocks":10240,"local":{},"global":{}}}`)
	if err := gulu.JSON.UnmarshalJSON(legacy, appConf); err != nil {
		t.Fatalf("legacy graph config should be ignored: %v", err)
	}

	encoded, err := gulu.JSON.MarshalJSON(appConf)
	if err != nil {
		t.Fatalf("marshal app config: %v", err)
	}
	if strings.Contains(string(encoded), `"graph"`) {
		t.Fatalf("removed graph config should not be persisted: %s", encoded)
	}
}
