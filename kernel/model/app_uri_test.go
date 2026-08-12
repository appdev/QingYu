package model

import "testing"

func TestQingYuBlockURICompatibility(t *testing.T) {
	tests := []struct {
		uri string
		id  string
		ok  bool
	}{
		{"qingyu://blocks/20260810120000-abcdefg", "20260810120000-abcdefg", true},
		{"siyuan://blocks/20260810120000-abcdefg", "20260810120000-abcdefg", true},
		{"https://example.com/blocks/20260810120000-abcdefg", "", false},
	}
	for _, test := range tests {
		id, ok := trimAppBlockURIPrefix(test.uri)
		if id != test.id || ok != test.ok {
			t.Fatalf("trimAppBlockURIPrefix(%q) = %q, %v; want %q, %v", test.uri, id, ok, test.id, test.ok)
		}
	}

	if uri := appBlockURI("20260810120000-abcdefg"); uri != "qingyu://blocks/20260810120000-abcdefg" {
		t.Fatalf("appBlockURI generated %q", uri)
	}
}
