package cmd

import "testing"

func TestNativeFeatureCommandsRemoved(t *testing.T) {
	for _, command := range documentCmd.Commands() {
		if command.Name() == "create" || command.Name() == "duplicate" {
			t.Fatalf("native document creation command remains: %s", command.Name())
		}
	}
	for _, command := range rootCmd.Commands() {
		if command.Name() == "sql" {
			t.Fatal("SQL command remains registered")
		}
	}
	for _, command := range databaseCmd.Commands() {
		switch command.Name() {
		case "search", "get", "render", "keys", "unused":
		default:
			t.Fatalf("database write command remains registered: %s", command.Name())
		}
	}
}
