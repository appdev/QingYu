package av

import "testing"

func TestDecimalSumPreservesCommonDecimalPrecision(t *testing.T) {
	var sum decimalSum
	sum.add(0.1)
	sum.add(0.2)
	if got := sum.float64(); got != 0.3 {
		t.Fatalf("decimal sum = %.17g, want 0.3", got)
	}
}

func TestNumberDifferencePreservesCommonDecimalPrecision(t *testing.T) {
	if got := numberDifference(0.3, 0.1); got != 0.2 {
		t.Fatalf("decimal difference = %.17g, want 0.2", got)
	}
}
