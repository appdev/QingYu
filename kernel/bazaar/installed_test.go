package bazaar

import "testing"

func TestIsBelowRequiredAppVersionUsesSiYuanCompatibleVersion(t *testing.T) {
	tests := []struct {
		name          string
		minAppVersion string
		want          bool
	}{
		{name: "missing minimum version", minAppVersion: "", want: false},
		{name: "below compatibility baseline", minAppVersion: "3.6.9", want: false},
		{name: "equal to compatibility baseline", minAppVersion: "3.7.0", want: false},
		{name: "above compatibility baseline", minAppVersion: "3.7.1", want: true},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			pkg := &Package{MinAppVersion: test.minAppVersion}
			if got := isBelowRequiredAppVersion(pkg); got != test.want {
				t.Fatalf("isBelowRequiredAppVersion() = %v, want %v", got, test.want)
			}
		})
	}
}
