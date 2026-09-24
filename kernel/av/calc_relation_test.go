package av

import "testing"

func TestCalcRelationAndRollupCountValues(t *testing.T) {
	for _, keyType := range []KeyType{KeyTypeRelation, KeyTypeRollup} {
		t.Run(string(keyType), func(t *testing.T) {
			field := &TableColumn{BaseInstanceField: &BaseInstanceField{
				Type: keyType,
				Calc: &FieldCalc{Operator: CalcOperatorCountValues},
			}}
			table := &Table{Columns: []*TableColumn{field}}
			for _, contents := range [][]string{{}, {"a", "b"}, {"a"}} {
				value := &Value{Type: keyType}
				if keyType == KeyTypeRelation {
					value.Relation = &ValueRelation{BlockIDs: contents}
				} else {
					value.Rollup = &ValueRollup{}
					for _, content := range contents {
						value.Rollup.Contents = append(value.Rollup.Contents, &Value{Type: KeyTypeText, Text: &ValueText{Content: content}})
					}
				}
				table.Rows = append(table.Rows, &TableRow{Cells: []*TableCell{{BaseValue: &BaseValue{Value: value}}}})
			}
			calcField(table, field, 0, nil)
			if got := field.Calc.Result.Number.Content; got != 3 {
				t.Fatalf("count values = %v, want 3", got)
			}
		})
	}
}
