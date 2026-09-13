package tools

import "testing"

func TestNativeDocumentToolRejectsCreation(t *testing.T) {
	for _, action := range []string{"create", "duplicate"} {
		result, err := DocumentTool.Handler(map[string]any{"action": action})
		if err != nil || !result.IsError {
			t.Fatalf("native creation action accepted: %s, %v", action, err)
		}
	}
}

func TestDatabaseToolRejectsWrites(t *testing.T) {
	for _, action := range []string{"key_add", "key_remove", "item_add", "item_remove", "item_update", "clean"} {
		for _, advertised := range DatabaseTool.InputSchema.Properties["action"].Enum {
			if advertised == action {
				t.Fatalf("write action advertised: %s", action)
			}
		}
		result, err := DatabaseTool.Handler(map[string]any{"action": action, "id": "missing"})
		if err != nil || !result.IsError {
			t.Fatalf("write action accepted: %s, %v", action, err)
		}
	}
}
