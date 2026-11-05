// Command bzip2-turbo-bench measures four complete codec processes.
package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

type identity struct {
	Name   string `json:"name"`
	SHA256 string `json:"sha256"`
}
type sample struct {
	Seconds       float64 `json:"wall_seconds"`
	UserSeconds   float64 `json:"user_cpu_seconds"`
	SystemSeconds float64 `json:"system_cpu_seconds"`
	RSSBytes      *uint64 `json:"peak_rss_bytes,omitempty"`
}
type codecPolicy struct {
	BlockBytes         int    `json:"block_bytes"`
	OptimizationPasses int    `json:"optimization_passes"`
	Policy             string `json:"policy"`
}
type row struct {
	Settings             string      `json:"settings"`
	ReferencePolicy      codecPolicy `json:"reference_settings"`
	OursPolicy           codecPolicy `json:"ours_settings"`
	SettingsMatch        bool        `json:"block_and_pass_settings_match"`
	ReferenceName        string      `json:"reference_name"`
	ReferenceThreads     int         `json:"reference_threads"`
	Corpus               string      `json:"corpus"`
	API                  string      `json:"api"`
	Level                int         `json:"level"`
	Mode                 string      `json:"mode"`
	Threads              int         `json:"threads"`
	Input                identity    `json:"input"`
	InputBytes           int64       `json:"input_bytes"`
	ReferenceOutput      identity    `json:"reference_encoded_output"`
	OursOutput           identity    `json:"ours_encoded_output"`
	ReferenceOutputBytes int64       `json:"reference_encoded_bytes"`
	OursOutputBytes      int64       `json:"ours_encoded_bytes"`
	Validation           string      `json:"validation"`
	Reference            []sample    `json:"reference_runs"`
	Ours                 []sample    `json:"ours_runs"`
	ReferenceMedian      float64     `json:"reference_median_wall_seconds"`
	OursMedian           float64     `json:"ours_median_wall_seconds"`
	ReferenceOverOurs    float64     `json:"reference_over_ours_wall_seconds"`
	ReferenceRSSMedian   *float64    `json:"reference_median_peak_rss_bytes,omitempty"`
	OursRSSMedian        *float64    `json:"ours_median_peak_rss_bytes,omitempty"`
	ReferenceOverOursRSS *float64    `json:"reference_over_ours_peak_rss,omitempty"`
}
type report struct {
	Settings                                                                              string `json:"settings"`
	Schema                                                                                int    `json:"schema"`
	Platform, StartedUTC, SourceSHA256, Revision, ReferenceVersion, RSSScope, TimingScope string
	Driver, Reference, Pure, C                                                            identity
	Repeats, Warmups                                                                      int
	RSSSupported                                                                          bool
	PerformanceCounters                                                                   string
	Rows                                                                                  []row
	Skipped                                                                               []scenario
}

func main() {
	command := run
	args := os.Args[1:]
	if len(args) > 0 && args[0] == "legacy-corpus" {
		command, args = legacyCorpus, args[1:]
	} else if len(args) > 0 && args[0] == "legacy-compare" {
		command, args = legacyCompare, args[1:]
	}
	if err := command(args); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
func digest(path string) (string, error) {
	f, e := os.Open(path)
	if e != nil {
		return "", e
	}
	defer f.Close()
	h := sha256.New()
	if _, e = io.Copy(h, f); e != nil {
		return "", e
	}
	return hex.EncodeToString(h.Sum(nil)), nil
}
func identify(path string) (identity, error) {
	h, e := digest(path)
	return identity{filepath.Base(path), h}, e
}
func median(xs []sample) float64 {
	a := make([]float64, len(xs))
	for i, x := range xs {
		a[i] = x.Seconds
	}
	sort.Float64s(a)
	if len(a)%2 == 1 {
		return a[len(a)/2]
	}
	return (a[len(a)/2-1] + a[len(a)/2]) / 2
}
func sourceDigest(root string) (string, error) {
	h := sha256.New()
	var files []string
	e := filepath.WalkDir(filepath.Join(root, "src"), func(path string, d os.DirEntry, e error) error {
		if e != nil {
			return e
		}
		if !d.IsDir() && strings.HasSuffix(path, ".rs") {
			files = append(files, path)
		}
		return nil
	})
	if e != nil {
		return "", e
	}
	files = append(files, filepath.Join(root, "Cargo.toml"), filepath.Join(root, "examples", "codec-bench.rs"))
	for _, subtree := range []string{"tools/bzip2-reference", "bench/bzip2-turbo-bench"} {
		if e := filepath.WalkDir(filepath.Join(root, subtree), func(path string, d os.DirEntry, e error) error {
			if e != nil {
				return e
			}
			if d.IsDir() && d.Name() == "target" {
				return filepath.SkipDir
			}
			if !d.IsDir() && (strings.HasSuffix(path, ".rs") || strings.HasSuffix(path, ".go") || d.Name() == "Cargo.toml" || d.Name() == "Cargo.lock" || d.Name() == "go.mod") {
				files = append(files, path)
			}
			return nil
		}); e != nil {
			return "", e
		}
	}
	if _, e := os.Stat(filepath.Join(root, "Cargo.lock")); e == nil {
		files = append(files, filepath.Join(root, "Cargo.lock"))
	}
	sort.Strings(files)
	for _, p := range files {
		rel, _ := filepath.Rel(root, p)
		v, e := digest(p)
		if e != nil {
			return "", e
		}
		fmt.Fprintf(h, "%s\x00%s\n", filepath.ToSlash(rel), v)
	}
	return hex.EncodeToString(h.Sum(nil)), nil
}
func canonical(path string) (string, error) {
	path, e := filepath.Abs(path)
	if e != nil {
		return "", e
	}
	if resolved, e := filepath.EvalSymlinks(path); e == nil {
		return resolved, nil
	}
	parent := filepath.Dir(path)
	if parent == path {
		return "", fmt.Errorf("cannot resolve %s", path)
	}
	resolved, e := canonical(parent)
	if e != nil {
		return "", e
	}
	return filepath.Join(resolved, filepath.Base(path)), nil
}
func outside(path, root string) error {
	p, e := canonical(path)
	if e != nil {
		return e
	}
	r, e := canonical(root)
	if e != nil {
		return e
	}
	rel, e := filepath.Rel(r, p)
	if e != nil {
		return e
	}
	if rel == "." || (!strings.HasPrefix(rel, ".."+string(os.PathSeparator)) && rel != "..") {
		return errors.New("output and corpus must be outside the repository")
	}
	return nil
}
func process(ctx context.Context, args []string, input, output string) (sample, error) {
	in, e := os.Open(input)
	if e != nil {
		return sample{}, e
	}
	defer in.Close()
	out, e := os.Create(output)
	if e != nil {
		return sample{}, e
	}
	defer out.Close()
	cmd := exec.CommandContext(ctx, args[0], args[1:]...)
	cmd.Stdin = in
	cmd.Stdout = out
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	started := time.Now()
	e = cmd.Run()
	elapsed := time.Since(started).Seconds()
	if e != nil {
		return sample{}, fmt.Errorf("%s failed: %w: %s", filepath.Base(args[0]), e, stderr.String())
	}
	return sample{Seconds: elapsed, UserSeconds: cmd.ProcessState.UserTime().Seconds(), SystemSeconds: cmd.ProcessState.SystemTime().Seconds(), RSSBytes: peakRSS(cmd.ProcessState)}, nil
}
func equal(a, b string) error {
	ha, e := digest(a)
	if e != nil {
		return e
	}
	hb, e := digest(b)
	if e != nil {
		return e
	}
	if ha != hb {
		return errors.New("byte validation failed")
	}
	return nil
}
