package av

import (
	"math"
	"math/big"
	"strconv"

	"github.com/siyuan-note/siyuan/kernel/util"
)

// decimalSum 使用最短十进制表示累计数字，避免常见的二进制浮点误差。
type decimalSum struct {
	total     big.Rat
	nonFinite float64
}

func calculationNumber(value *Value) float64 {
	if value != nil && KeyTypeNumber == value.Type && value.Number != nil {
		return value.Number.Content
	}
	result, _ := util.Convert2Float(value.String(false))
	return result
}

func (sum *decimalSum) add(value float64) {
	if math.IsNaN(value) || math.IsInf(value, 0) {
		sum.nonFinite += value
		return
	}
	var term big.Rat
	if _, ok := term.SetString(strconv.FormatFloat(value, 'g', -1, 64)); ok {
		sum.total.Add(&sum.total, &term)
	}
}

func (sum *decimalSum) float64() float64 {
	if sum.nonFinite != 0 {
		return sum.nonFinite
	}
	result, _ := sum.total.Float64()
	return result
}

func (sum *decimalSum) average(count int) float64 {
	if sum.nonFinite != 0 {
		return sum.nonFinite
	}
	if count == 0 {
		return 0
	}
	var divisor, average big.Rat
	divisor.SetInt64(int64(count))
	average.Quo(&sum.total, &divisor)
	result, _ := average.Float64()
	return result
}

func numberMean(left, right float64) float64 {
	var sum decimalSum
	sum.add(left)
	sum.add(right)
	return sum.average(2)
}

func numberDifference(left, right float64) float64 {
	var sum decimalSum
	sum.add(left)
	sum.add(-right)
	return sum.float64()
}
