package main

import (
	"crypto/sha256"
	"fmt"
	"testing"
)

func TestLegacyCorpusMatchesPythonCharacterization(t *testing.T) {
	expected := map[string]string{"mixed": "af036c5ac454266cd78b206c5369d3a162fb5b1e4a5b059ae202b37f3e96d78f", "random": "5b64b12ad6e657f403f9e3e57e4ad6fbd1d8fb14c53a0c7e1dc5dbd2257166b1", "runs": "18b9210e9ee430bf2b536757618f542ce4bea2123ebaa14de2f06cab1cf90cb2", "text": "f66417102d0679a87e8f3ef8e21f37f2d79dcd82cad1605cdccc8bd168068eda"}
	for name, block := range legacyBlocks() {
		if got := fmt.Sprintf("%x", sha256.Sum256(block)); got != expected[name] {
			t.Fatalf("%s corpus changed: %s", name, got)
		}
	}
}
func TestLegacyParityUsesUnroundedMediansAndCompletePairs(t *testing.T) {
	items := []legacyMeasurement{{Mode: "decode", Threads: 1, Implementation: "reference", Median: 1}, {Mode: "decode", Threads: 1, Implementation: "candidate", Median: 1.000000001}}
	result, err := legacyParity(items)
	if err != nil || result["passed"].(bool) {
		t.Fatal("small regression escaped strict gate", result, err)
	}
	items[1].Median = 1
	result, err = legacyParity(items)
	if err != nil || !result["passed"].(bool) {
		t.Fatal("equal median failed", result, err)
	}
	if _, err = legacyParity(items[:1]); err == nil {
		t.Fatal("incomplete pair accepted")
	}
	if _, err = legacyParity(append(items, items[0])); err == nil {
		t.Fatal("duplicate pair accepted")
	}
}
func TestLegacyDarwinCountersRejectUnavailableAndDuplicateValues(t *testing.T) {
	raw := ""
	for label := range darwinCounters {
		raw += "  17 " + label + "\n"
	}
	values, err := parseDarwinResources(raw)
	if err != nil || len(values) != 8 {
		t.Fatal(values, err)
	}
	if _, err = parseDarwinResources(raw + "  18 instructions retired\n"); err == nil {
		t.Fatal("duplicate accepted")
	}
	if _, err = parseDarwinResources("0 instructions retired\n"); err == nil {
		t.Fatal("incomplete counters accepted")
	}
}

func TestLegacyCLIRejectsInvalidRoundsAndRepositoryArtifacts(t *testing.T) {
	for _, option := range [][]string{{"--runs", "2"}, {"--warmups", "0"}, {"--level", "10"}} {
		args := append([]string{"--codec", "unused", "--oracle", "unused", "--input", "unused", "--results", "unused"}, option...)
		if err := legacyCompare(args); err == nil {
			t.Fatal("invalid configuration accepted", option)
		}
	}
	root, err := legacyRoot()
	if err != nil {
		t.Fatal(err)
	}
	if err := legacyCorpus([]string{root, "--mib", "1"}); err == nil {
		t.Fatal("repository corpus accepted")
	}
	if err := legacyCorpus([]string{t.TempDir(), "--mib", "0"}); err == nil {
		t.Fatal("empty corpus accepted")
	}
}
