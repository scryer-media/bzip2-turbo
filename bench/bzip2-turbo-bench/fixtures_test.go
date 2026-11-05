package main

import (
	"bytes"
	"compress/gzip"
	"crypto/sha256"
	"debug/buildinfo"
	"debug/elf"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

func TestVariedLogsKeepWholeRecordsAndStableDistribution(t *testing.T) {
	var first, second bytes.Buffer
	if e := generateLogs(&first, 1<<20); e != nil {
		t.Fatal(e)
	}
	if e := generateLogs(&second, 1<<20); e != nil {
		t.Fatal(e)
	}
	if !bytes.Equal(first.Bytes(), second.Bytes()) || first.Len() < 1<<20 || first.Len() > (1<<20)+512 || !bytes.HasSuffix(first.Bytes(), []byte{10}) {
		t.Fatal("non-deterministic or truncated log extent")
	}
	lines := strings.Split(strings.TrimSpace(first.String()), "\n")
	levels, events, paths := map[string]int{}, map[string]bool{}, map[string]bool{}
	seen := map[string]bool{}
	record := regexp.MustCompile(`^2026-01-01T\S+Z level=(DEBUG|INFO|WARN|ERROR) service=\w+ node=node-\d+ event=(\w+) request_id=([0-9a-f]{16}) method=(GET|POST|PUT|DELETE) path="([^"]+)" status=\d+ duration_ms=\d+ bytes=\d+ queue=\d+ remote=192\.0\.2\.\d+ message="[^"]+"$`)
	for id, line := range lines {
		match := record.FindStringSubmatch(line)
		if match == nil {
			t.Fatalf("invalid log record %d", id)
		}
		if seen[line] || match[3] != fmt.Sprintf("%016x", id) {
			t.Fatal("repeated record or wrong sequence")
		}
		seen[line] = true
		levels[match[1]]++
		events[match[2]] = true
		paths[match[5]] = true
	}
	if len(events) != 8 || len(paths) < 100 || len(levels) != 4 || levels["INFO"] < len(lines)/2 || levels["ERROR"] > len(lines)/10 {
		t.Fatal("unrealistic or unvaried log distribution", levels)
	}
}
func TestRequiredTextFixturesPreserveValidJSONAnd71ByteStress(t *testing.T) {
	root := t.TempDir()
	jsonInfo, e := createFixture(filepath.Join(root, "text.json"), "json", 65539)
	if e != nil {
		t.Fatal(e)
	}
	b, e := os.ReadFile(filepath.Join(root, "text.json"))
	if e != nil || !json.Valid(b) || jsonInfo.Bytes != 65539 {
		t.Fatal("JSON fixture changed")
	}
	path := filepath.Join(root, "repeated.txt")
	info, e := createFixture(path, "repeated-text", 65539)
	if e != nil {
		t.Fatal(e)
	}
	b, e = os.ReadFile(path)
	if e != nil || len(repeatedSentence) != 71 || info.Bytes != 65539 {
		t.Fatal("stress extent changed")
	}
	for i, value := range b {
		if value != repeatedSentence[i%71] {
			t.Fatal("stress period changed")
		}
	}
	if e := run([]string{"suite", "--list", "--corpora", "json,repeated-text,logs,mkv,elf", "--levels", "5", "--threads", "1", "--api", "native"}); e != nil {
		t.Fatal(e)
	}
}
func TestNoiseIsSeededContinuousAndHashesActualBytes(t *testing.T) {
	var one, two bytes.Buffer
	h, e := noise(&one, 2)
	if e != nil {
		t.Fatal(e)
	}
	if _, e = noise(&two, 2); e != nil {
		t.Fatal(e)
	}
	sum := sha256.Sum256(one.Bytes())
	if !bytes.Equal(one.Bytes(), two.Bytes()) || h != hex.EncodeToString(sum[:]) || bytes.Equal(one.Bytes()[:frameBytes], one.Bytes()[frameBytes:]) {
		t.Fatal("noise is not reproducible continuous PRNG output")
	}
}
func TestFailedFixturePreservesExistingOutput(t *testing.T) {
	path := filepath.Join(t.TempDir(), "input.json")
	if e := os.WriteFile(path, []byte("preserved"), 0600); e != nil {
		t.Fatal(e)
	}
	if _, e := createFixture(path, "json", 1); e == nil {
		t.Fatal("accepted invalid JSON extent")
	}
	b, e := os.ReadFile(path)
	if e != nil || string(b) != "preserved" {
		t.Fatal("failed generator replaced existing data")
	}
	names, e := os.ReadDir(filepath.Dir(path))
	if e != nil || len(names) != 1 {
		t.Fatal("failed generator retained temporary output")
	}
}
func oracleForFixture(t *testing.T) string {
	t.Helper()
	if path := os.Getenv("BZIP2_TURBO_7ZZ"); path != "" {
		return path
	}
	for _, name := range []string{"7zz", "7z"} {
		if path, e := exec.LookPath(name); e == nil {
			return path
		}
	}
	t.Skip("7-Zip unavailable for empirical BZip2 fraction check")
	return ""
}
func bzipFraction(t *testing.T, path string) float64 {
	t.Helper()
	oracle := oracleForFixture(t)
	input, e := os.Open(path)
	if e != nil {
		t.Fatal(e)
	}
	defer input.Close()
	cmd := exec.Command(oracle, "a", "-tbzip2", "-mx=5", "-mmt=1", "-md=500000b", "-mpass=1", "-si", "-so", "stream.bz2")
	cmd.Stdin = input
	var output bytes.Buffer
	cmd.Stdout = &output
	if e = cmd.Run(); e != nil {
		t.Fatal(e)
	}
	info, e := input.Stat()
	if e != nil {
		t.Fatal(e)
	}
	return float64(output.Len()) / float64(info.Size())
}
func TestMatroskaIsDeterministicValidLosslessAndNearIncompressible(t *testing.T) {
	for _, tool := range []string{"ffmpeg", "ffprobe"} {
		if _, e := exec.LookPath(tool); e != nil {
			t.Skip(tool + " unavailable")
		}
	}
	first := filepath.Join(t.TempDir(), "a.mkv")
	second := filepath.Join(t.TempDir(), "b.mkv")
	a, e := createFixtureWithOptions(first, "mkv", 1<<20, fixtureOptions{MKVSource: "noise"})
	if e != nil {
		t.Fatal(e)
	}
	b, e := createFixtureWithOptions(second, "mkv", 1<<20, fixtureOptions{MKVSource: "noise"})
	if e != nil {
		t.Fatal(e)
	}
	if a.Input.SHA256 != b.Input.SHA256 || a.RawSHA256 != b.RawSHA256 || a.Bytes <= 0 || a.Bytes == a.RequestedBytes || a.Media == nil || a.Media.Frames != 16 || a.ToolVersion == "" || a.Media.FFprobeVersion == "" {
		t.Fatal("Matroska identity or validated extent changed")
	}
	t.Run("BZip2 fraction", func(t *testing.T) {
		ratio := bzipFraction(t, first)
		if ratio < 0.99 {
			t.Fatalf("MKV compresses materially: %.6f", ratio)
		}
		t.Logf("MKV encoded/input %.6f", ratio)
	})
}
func TestELFBuildIsReproducibleCompressibleAndPrivate(t *testing.T) {
	first := filepath.Join(t.TempDir(), "a.elf")
	second := filepath.Join(t.TempDir(), "b.elf")
	a, e := createFixture(first, "elf", 1)
	if e != nil {
		t.Fatal(e)
	}
	b, e := createFixture(second, "elf", 1)
	if e != nil {
		t.Fatal(e)
	}
	if a.Input.SHA256 != b.Input.SHA256 || a.ToolVersion == "" || a.SourceSHA256 == "" || a.Bytes < 1<<20 {
		t.Fatal("ELF differs across independent build directories or is not a genuine executable")
	}
	file, e := elf.Open(first)
	if e != nil {
		t.Fatal(e)
	}
	defer file.Close()
	if file.Section(".note.go.buildid") != nil {
		t.Fatal("ELF retained build ID")
	}
	build, e := buildinfo.ReadFile(first)
	if e != nil {
		t.Fatal(e)
	}
	settings := map[string]string{}
	for _, setting := range build.Settings {
		settings[setting.Key] = setting.Value
	}
	for key, value := range map[string]string{"-trimpath": "true", "CGO_ENABLED": "0", "GOOS": "linux", "GOARCH": "amd64", "GOAMD64": "v1"} {
		if settings[key] != value {
			t.Fatal("unexpected ELF setting", key, settings[key])
		}
	}
	data, e := os.ReadFile(first)
	if e != nil {
		t.Fatal(e)
	}
	for _, private := range []string{filepath.Dir(first), filepath.Dir(second), os.Getenv("HOME"), "codec-elf-source-"} {
		if private != "" && bytes.Contains(data, []byte(private)) {
			t.Fatal("ELF embeds a private build path")
		}
	}
	var compressed bytes.Buffer
	writer := gzip.NewWriter(&compressed)
	if _, e = writer.Write(data); e != nil {
		t.Fatal(e)
	}
	if e = writer.Close(); e != nil {
		t.Fatal(e)
	}
	if float64(compressed.Len())/float64(len(data)) > 0.90 {
		t.Fatal("ELF is not materially compressible")
	}
	t.Run("BZip2 fraction", func(t *testing.T) {
		ratio := bzipFraction(t, first)
		if ratio > 0.90 {
			t.Fatalf("ELF is not materially compressible: %.6f", ratio)
		}
		t.Logf("ELF encoded/input %.6f", ratio)
	})
}

func TestPublicSourceRequiresConsentAndPinnedLocalBytes(t *testing.T) {
	directory := t.TempDir()
	if _, e := bbbSource(fixtureOptions{}, directory); e == nil {
		t.Fatal("public source downloaded without explicit consent")
	}
	bad := filepath.Join(t.TempDir(), "source.zip")
	if e := os.WriteFile(bad, []byte("not the pinned archive"), 0600); e != nil {
		t.Fatal(e)
	}
	if _, e := bbbSource(fixtureOptions{SourceFile: bad}, directory); e == nil {
		t.Fatal("unpinned public source accepted")
	}
	if _, e := bbbSource(fixtureOptions{SourceFile: bad, Download: true}, directory); e == nil {
		t.Fatal("ambiguous network/local source accepted")
	}
	large := filepath.Join(t.TempDir(), "large.zip")
	f, e := os.Create(large)
	if e != nil {
		t.Fatal(e)
	}
	if e = f.Truncate(bbbArchiveLimit + 1); e != nil {
		t.Fatal(e)
	}
	_ = f.Close()
	if _, e := bbbSource(fixtureOptions{SourceFile: large}, directory); e == nil {
		t.Fatal("oversized source accepted")
	}
}
func TestCanonicalPublicMovieIsValidatedAttributedAndNearIncompressible(t *testing.T) {
	source := os.Getenv("BZIP2_BBB_SOURCE")
	if source == "" {
		t.Skip("pinned public MP4/ZIP not supplied; tests never download source")
	}
	path := filepath.Join(t.TempDir(), "movie.mkv")
	info, e := createFixtureWithOptions(path, "mkv", 1, fixtureOptions{MKVSource: "big-buck-bunny", SourceFile: source})
	if e != nil {
		t.Fatal(e)
	}
	if info.Input.SHA256 != bbbOutputSHA || info.Bytes != 16360253 || info.Media == nil || info.Media.Codec != "h264" || info.Media.AudioCodec != "aac" || info.Source == nil || info.Source.Attribution != bbbAttribution || info.Source.LicenseURL != "https://creativecommons.org/licenses/by/3.0/" || info.Source.Changes == "" {
		t.Fatal("canonical movie identity, extent or attribution differs")
	}
	command := exec.Command("ffprobe", "-v", "error", "-show_entries", "format_tags", "-of", "json", path)
	metadata, e := command.Output()
	if e != nil || !bytes.Contains(metadata, []byte(bbbAttribution)) {
		t.Fatal("Matroska attribution metadata missing")
	}
	t.Run("BZip2 fraction", func(t *testing.T) {
		ratio := bzipFraction(t, path)
		if ratio < 0.99 {
			t.Fatalf("public movie compresses materially: %.6f", ratio)
		}
		t.Logf("public MKV encoded/input %.6f", ratio)
	})
}
func TestCanonicalReceiptBindsExactBytes(t *testing.T) {
	root := t.TempDir()
	input := filepath.Join(root, "logs.log")
	info, e := createFixture(input, "logs", 65536)
	if e != nil {
		t.Fatal(e)
	}
	manifest := filepath.Join(root, "receipt.json")
	data, e := json.Marshal(info)
	if e != nil {
		t.Fatal(e)
	}
	if e = os.WriteFile(manifest, data, 0600); e != nil {
		t.Fatal(e)
	}
	if _, e = readFixtureManifest(manifest, input); e != nil {
		t.Fatal(e)
	}
	if e = os.WriteFile(input, []byte("changed"), 0600); e != nil {
		t.Fatal(e)
	}
	if _, e = readFixtureManifest(manifest, input); e == nil {
		t.Fatal("mismatched fixture accepted")
	}
}

func TestCanonicalReceiptRejectsFakedBinaryKinds(t *testing.T) {
	root := t.TempDir()
	input := filepath.Join(root, "input.log")
	info, err := createFixture(input, "logs", 65536)
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(root, "receipt.json")
	for _, kind := range []string{"elf", "mkv"} {
		info.Kind = kind
		data, err := json.Marshal(info)
		if err != nil {
			t.Fatal(err)
		}
		if err = os.WriteFile(path, data, 0600); err != nil {
			t.Fatal(err)
		}
		if _, err = readFixtureManifest(path, input); err == nil {
			t.Fatal("accepted faked binary kind", kind)
		}
	}
}
