package conf

import "testing"

func TestQingYuPublishPortDefault(t *testing.T) {
	if got := NewPublish().Port; got != 9808 {
		t.Fatalf("NewPublish().Port = %d, want 9808", got)
	}
}
