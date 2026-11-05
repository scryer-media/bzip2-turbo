package main

import (
	"context"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strconv"
	"testing"
	"time"
)

func TestScheduleKeepsCompatibilitySerial(t *testing.T) {
	ready, skipped := schedule([]string{"text", "random", "periodic", "runs"}, []int{1, 5, 9}, []int{1, 2, 4, 8}, []string{"native", "compat"})
	if len(ready) != 60 || len(skipped) != 36 {
		t.Fatalf("ready=%d skipped=%d", len(ready), len(skipped))
	}
	seen := map[scenario]bool{}
	for _, s := range ready {
		if seen[s] {
			t.Fatal("duplicate", s)
		}
		seen[s] = true
		if s.API == "compat" && s.Threads != 1 {
			t.Fatal("pretend compatible MT", s)
		}
	}
	for _, s := range skipped {
		if s.Skip == "" || s.API != "compat" || s.Threads == 1 {
			t.Fatal("invalid skip", s)
		}
	}
	for i := 0; i < 4; i++ {
		want := []int{i, (i + 1) % 4, (i + 2) % 4, (i + 3) % 4}
		if !reflect.DeepEqual(order(i), want) {
			t.Fatal("unbalanced rotating order")
		}
	}

}
func TestRSSMediansAndRatiosUseBytesAndRejectMissingSamples(t *testing.T) {
	ptr := func(n uint64) *uint64 { return &n }
	r := row{Reference: []sample{{Seconds: 4, RSSBytes: ptr(4096)}, {Seconds: 2, RSSBytes: ptr(1024)}, {Seconds: 3, RSSBytes: ptr(2048)}}, Ours: []sample{{Seconds: 2, RSSBytes: ptr(1024)}, {Seconds: 1, RSSBytes: ptr(512)}, {Seconds: 1.5, RSSBytes: ptr(1024)}}}
	summarize(&r)
	if r.ReferenceMedian != 3 || r.OursMedian != 1.5 || r.ReferenceOverOurs != 2 || *r.ReferenceRSSMedian != 2048 || *r.OursRSSMedian != 1024 || *r.ReferenceOverOursRSS != 2 {
		t.Fatalf("bad summary %+v", r)
	}
	r.Ours[0].RSSBytes = nil
	summarize(&r)
	if r.OursRSSMedian != nil || r.ReferenceOverOursRSS != nil {
		t.Fatal("missing RSS presented as ratio")
	}
}
func TestOutsideRejectsRepositoryAndSymlinkAliases(t *testing.T) {
	root := t.TempDir()
	external := t.TempDir()
	if outside(filepath.Join(root, "new", "report.json"), root) == nil {
		t.Fatal("accepted repository output")
	}
	alias := filepath.Join(external, "alias")
	if e := os.Symlink(root, alias); e != nil {
		t.Skip(e)
	}
	if outside(filepath.Join(alias, "missing.json"), root) == nil {
		t.Fatal("accepted symlink alias")
	}
	if e := outside(filepath.Join(external, "report.json"), root); e != nil {
		t.Fatal(e)
	}
}
func TestCorporaAreReproducibleAndDifferent(t *testing.T) {
	hashes := map[string]bool{}
	for _, kind := range []string{"text", "random", "periodic", "runs"} {
		a, b := filepath.Join(t.TempDir(), "a"), filepath.Join(t.TempDir(), "b")
		if e := generate(a, kind, 65539); e != nil {
			t.Fatal(e)
		}
		if e := generate(b, kind, 65539); e != nil {
			t.Fatal(e)
		}
		if e := equal(a, b); e != nil {
			t.Fatal(e)
		}
		h, e := digest(a)
		if e != nil {
			t.Fatal(e)
		}
		if hashes[h] {
			t.Fatal("duplicate corpus", kind)
		}
		hashes[h] = true
		info, e := os.Stat(a)
		if e != nil || info.Size() != 65539 {
			t.Fatal("wrong corpus size")
		}
	}
	if generate(filepath.Join(t.TempDir(), "bad"), "unknown", 1) == nil {
		t.Fatal("accepted invalid corpus")
	}
}
func TestListAndInvalidOptionsNeedNoTools(t *testing.T) {
	if e := run([]string{"suite", "--list", "--driver", "missing", "--levels", "1", "--threads", "1,2"}); e != nil {
		t.Fatal(e)
	}
	for _, args := range [][]string{{"suite", "--list", "--repeats", "2"}, {"suite", "--list", "--levels", "0"}, {"suite", "--list", "--threads", "0"}, {"suite", "--list", "--api", "unknown"}} {
		if run(args) == nil {
			t.Fatal("accepted bad options", args)
		}
	}
}
func TestProcessErrorsAndTimeoutsAreFailures(t *testing.T) {
	executable, e := os.Executable()
	if e != nil {
		t.Fatal(e)
	}
	input := filepath.Join(t.TempDir(), "input")
	if e = os.WriteFile(input, []byte("test"), 0600); e != nil {
		t.Fatal(e)
	}
	for _, mode := range []string{"fail", "sleep"} {
		ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
		_, e := process(ctx, []string{executable, "-test.run=^TestProcessHelper$", "--", mode}, input, os.DevNull)
		cancel()
		if e == nil {
			t.Fatal("accepted failed process", mode)
		}
	}
}
func TestProcessHelper(t *testing.T) {
	if len(os.Args) < 2 {
		return
	}
	switch os.Args[len(os.Args)-1] {
	case "fail":
		os.Exit(7)
	case "sleep":
		time.Sleep(time.Second)
		os.Exit(0)
	}
}

func TestFourWayCommandsNormalizeBlockSizeAndPasses(t *testing.T) {
	tools := tools{driver: "turbo", sevenzip: "7zz", pure: "pure", c: "c", settings: "block-size"}
	for _, level := range []int{1, 5, 9} {
		s := scenario{API: "native", Level: level, Threads: 4}
		block := strconv.Itoa(level * 100000)
		want := []string{"turbo", "--api", "native", "encode", strconv.Itoa(level), "4", "in", "out", block, "1"}
		if got := tools.command(1, s, "encode", "in", "out"); !reflect.DeepEqual(got, want) {
			t.Fatalf("native command: %v", got)
		}
		seven := tools.command(0, s, "encode", "in", "out")
		if !slices.Contains(seven, "-md="+block+"b") || !slices.Contains(seven, "-mpass=1") {
			t.Fatal(seven)
		}
		for _, which := range []int{2, 3} {
			got := tools.command(which, s, "encode", "in", "out")
			if got[3] != "1" || len(got) != 6 {
				t.Fatal("reference is not genuinely serial", got)
			}
			if tools.policy(which, s).BlockBytes != level*100000 {
				t.Fatal("reference block size")
			}
		}
	}
}
func TestHistoricalPolicyIsExplicitAndCompatibilityStaysNormalized(t *testing.T) {
	tools := tools{driver: "turbo", sevenzip: "7zz", settings: "sevenzip-level"}
	for _, level := range []int{5, 9} {
		s := scenario{API: "native", Level: level, Threads: 1}
		p := tools.policy(1, s)
		if p.BlockBytes != 900000 || (level == 9 && p.OptimizationPasses != 7) {
			t.Fatal(p)
		}
		if len(tools.ours(s, "encode", "in", "out")) != 8 {
			t.Fatal("historical defaults overridden")
		}
		s.API = "compat"
		if p := tools.policy(0, s); p.BlockBytes != level*100000 || p.OptimizationPasses != 1 {
			t.Fatal(p)
		}
		if !slices.Contains(tools.command(0, s, "encode", "in", "out"), "-mpass=1") {
			t.Fatal("compat reference not aligned")
		}
	}
	if run([]string{"suite", "--list", "--settings", "unknown"}) == nil {
		t.Fatal("accepted unknown policy")
	}
}
