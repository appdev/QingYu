package util

import (
	"net/http"
	"testing"
)

func TestAuthCodeEquals(t *testing.T) {
	if !AuthCodeEquals("secret", "secret") {
		t.Fatal("equal authentication codes were rejected")
	}
	if AuthCodeEquals("secret", "secret2") {
		t.Fatal("different authentication codes were accepted")
	}
}

func TestAuthThrottleLocksAndResets(t *testing.T) {
	key := "test-auth-throttle-lock"
	AuthThrottleReset(key)
	defer AuthThrottleReset(key)
	for range authThrottleMaxFail + 1 {
		AuthThrottleFail(key)
	}
	if retryAfter := AuthThrottleCheck(key); retryAfter <= 0 {
		t.Fatal("expected authentication throttle lock")
	}
	AuthThrottleReset(key)
	if retryAfter := AuthThrottleCheck(key); retryAfter != 0 {
		t.Fatalf("authentication throttle was not reset: %d", retryAfter)
	}
}

func TestGetAuthThrottleKeyIgnoresForwardedHeaders(t *testing.T) {
	req, err := http.NewRequest(http.MethodGet, "http://example.com", nil)
	if err != nil {
		t.Fatal(err)
	}
	req.RemoteAddr = "192.0.2.10:1234"
	req.Header.Set("X-Forwarded-For", "198.51.100.20")
	req.Header.Set("X-Real-IP", "203.0.113.30")
	if got := GetAuthThrottleKey(req); got != "192.0.2.10" {
		t.Fatalf("unexpected authentication throttle key: %q", got)
	}
}
