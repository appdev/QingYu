package api

import (
	"testing"
	"time"
)

func TestMarkdownSaveBarrierWaitsForEverySession(t *testing.T) {
	manager := newMarkdownSaveBarrierManager()
	wait := manager.begin("barrier", []string{"desktop", "window"})

	if !manager.ack("barrier", "desktop", true) {
		t.Fatal("first expected session should be acknowledged")
	}
	if !manager.ack("barrier", "desktop", true) {
		t.Fatal("duplicate acknowledgement should be idempotent")
	}
	if !manager.ack("barrier", "window", true) {
		t.Fatal("second expected session should be acknowledged")
	}
	if err := wait(100 * time.Millisecond); err != nil {
		t.Fatalf("wait for successful barrier: %v", err)
	}
}

func TestMarkdownSaveBarrierFailsClosed(t *testing.T) {
	t.Run("client failure", func(t *testing.T) {
		manager := newMarkdownSaveBarrierManager()
		wait := manager.begin("barrier", []string{"desktop"})
		manager.ack("barrier", "desktop", false)
		if err := wait(100 * time.Millisecond); err == nil {
			t.Fatal("failed client save should fail the barrier")
		}
	})

	t.Run("timeout", func(t *testing.T) {
		manager := newMarkdownSaveBarrierManager()
		wait := manager.begin("barrier", []string{"desktop"})
		if err := wait(time.Millisecond); err == nil {
			t.Fatal("missing acknowledgement should time out")
		}
	})

	t.Run("unknown session", func(t *testing.T) {
		manager := newMarkdownSaveBarrierManager()
		wait := manager.begin("barrier", []string{"desktop"})
		if manager.ack("barrier", "unknown", true) {
			t.Fatal("unknown session must not count toward the barrier")
		}
		if err := wait(time.Millisecond); err == nil {
			t.Fatal("unknown acknowledgement must not complete the barrier")
		}
	})
}

func TestMarkdownSaveBarrierWithNoSessionsCompletesImmediately(t *testing.T) {
	manager := newMarkdownSaveBarrierManager()
	if err := manager.begin("barrier", nil)(time.Millisecond); err != nil {
		t.Fatalf("empty barrier should complete immediately: %v", err)
	}
}

func TestMarkdownSaveBarriersKeepConcurrentRequestsIsolated(t *testing.T) {
	manager := newMarkdownSaveBarrierManager()
	waitFirst := manager.begin("first", []string{"desktop"})
	waitSecond := manager.begin("second", []string{"desktop"})

	manager.ack("first", "desktop", true)
	if err := waitFirst(100 * time.Millisecond); err != nil {
		t.Fatalf("first barrier should complete: %v", err)
	}
	if err := waitSecond(time.Millisecond); err == nil {
		t.Fatal("completing the first barrier must not complete the second")
	}
}
