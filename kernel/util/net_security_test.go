package util

import (
	"net/http/httptest"
	"testing"
)

func TestIsSessionOriginAllowedRequest(t *testing.T) {
	tests := []struct {
		name   string
		origin string
		fetch  string
		host   string
		want   bool
	}{
		{name: "same host", origin: "https://notes.example.test", host: "notes.example.test", want: true},
		{name: "cross origin", origin: "https://evil.example.test", host: "notes.example.test", want: false},
		{name: "cross site fetch", origin: "https://notes.example.test", fetch: "cross-site", host: "notes.example.test", want: false},
		{name: "native client", host: "notes.example.test", want: true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			req := httptest.NewRequest("GET", "https://"+tt.host+"/api/test", nil)
			req.Host = tt.host
			req.Header.Set("Origin", tt.origin)
			req.Header.Set("Sec-Fetch-Site", tt.fetch)
			if got := IsSessionOriginAllowedRequest(req); got != tt.want {
				t.Fatalf("IsSessionOriginAllowedRequest() = %v, want %v", got, tt.want)
			}
		})
	}
}
