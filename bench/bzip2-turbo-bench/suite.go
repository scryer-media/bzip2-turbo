package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"time"
)

type scenario struct {
	Corpus  string `json:"corpus"`
	API     string `json:"api"`
	Level   int    `json:"level"`
	Threads int    `json:"threads"`
	Mode    string `json:"mode,omitempty"`
	Skip    string `json:"skip,omitempty"`
}

// nativeDecodeThreadCap is the most threads the native decoder uses: one
// parser and one reconstruction worker. It mirrors the `threads.clamp(1, 2)`
// in `Decoder::with_mode` (src/decoder.rs); a larger request runs the same
// two-thread pipeline, so decode is not timed above it.
const nativeDecodeThreadCap = 2

// modes lists the timed modes for a runnable scenario. Native scenarios above
// the decoder's thread cap time encoding only; their decode mode is recorded
// as a skipped scenario by schedule.
func modes(s scenario) []string {
	if s.API == "native" && s.Threads > nativeDecodeThreadCap {
		return []string{"encode"}
	}
	return []string{"encode", "decode"}
}

func integers(text string, maximum int) ([]int, error) {
	var result []int
	seen := map[int]bool{}
	for _, part := range strings.Split(text, ",") {
		n, e := strconv.Atoi(strings.TrimSpace(part))
		if e != nil || n < 1 || n > maximum {
			return nil, fmt.Errorf("invalid value %q (want 1..%d)", part, maximum)
		}
		if !seen[n] {
			result = append(result, n)
			seen[n] = true
		}
	}
	return result, nil
}
func apiNames(text string) ([]string, error) {
	switch text {
	case "native", "compat":
		return []string{text}, nil
	case "both":
		return []string{"native", "compat"}, nil
	}
	return nil, errors.New("api must be native, compat or both")
}
func schedule(corpora []string, levels, threads []int, apis []string) ([]scenario, []scenario) {
	var ready, skipped []scenario
	for _, corpus := range corpora {
		for _, level := range levels {
			for _, n := range threads {
				for _, api := range apis {
					s := scenario{Corpus: corpus, Level: level, Threads: n, API: api}
					if api == "compat" && n != 1 {
						s.Skip = "compatible API is serial; no implicit worker threads"
						skipped = append(skipped, s)
					} else {
						ready = append(ready, s)
						if api == "native" && n > nativeDecodeThreadCap {
							d := s
							d.Mode = "decode"
							d.Skip = "native decoding uses at most two threads (parser and one worker); decode is measured at 1 and 2 threads"
							skipped = append(skipped, d)
						}
					}
				}
			}
		}
	}
	return ready, skipped
}
func order(repetition int) []int {
	result := make([]int, 4)
	for i := range result {
		result[i] = (i + repetition) % 4
	}
	return result
}
func rssMedian(samples []sample) *float64 {
	values := make([]float64, 0, len(samples))
	for _, s := range samples {
		if s.RSSBytes == nil {
			return nil
		}
		values = append(values, float64(*s.RSSBytes))
	}
	if len(values) == 0 {
		return nil
	}
	sort.Float64s(values)
	value := values[len(values)/2]
	if len(values)%2 == 0 {
		value = (values[len(values)/2-1] + value) / 2
	}
	return &value
}
func summarize(r *row) {
	if r.InputBytes > 0 {
		r.ReferenceEncodedFraction = float64(r.ReferenceOutputBytes) / float64(r.InputBytes)
		r.OursEncodedFraction = float64(r.OursOutputBytes) / float64(r.InputBytes)
	}
	r.ReferenceMedian = median(r.Reference)
	r.OursMedian = median(r.Ours)
	r.ReferenceOverOurs = r.ReferenceMedian / r.OursMedian
	r.ReferenceRSSMedian = rssMedian(r.Reference)
	r.OursRSSMedian = rssMedian(r.Ours)
	r.ReferenceOverOursRSS = nil
	if r.ReferenceRSSMedian != nil && r.OursRSSMedian != nil && *r.OursRSSMedian > 0 {
		ratio := *r.ReferenceRSSMedian / *r.OursRSSMedian
		r.ReferenceOverOursRSS = &ratio
	}
}

// generate streams a reproducible corpus without retaining it in memory.
func generate(path, kind string, size int64) error {
	if kind == "json" || kind == "repeated-text" || kind == "logs" || kind == "mkv" || kind == "elf" {
		_, err := createFixture(path, kind, size)
		return err
	}
	if size < 1 {
		return errors.New("corpus size must be positive")
	}
	if kind != "text" && kind != "random" && kind != "periodic" && kind != "runs" {
		return fmt.Errorf("unknown corpus %q", kind)
	}
	if kind == "text" && size < 256 {
		return errors.New("JSON text corpus requires at least 256 bytes")
	}
	f, e := os.Create(path)
	if e != nil {
		return e
	}
	defer f.Close()
	if kind == "text" {
		if e := generateJSON(f, size); e != nil {
			return e
		}
		return f.Close()
	}
	state := uint32(0x12345678)
	buffer := make([]byte, 65536)
	for base := int64(0); base < size; {
		n := int64(len(buffer))
		if size-base < n {
			n = size - base
		}
		for i := int64(0); i < n; i++ {
			position := base + i
			switch kind {
			case "random":
				state ^= state << 13
				state ^= state >> 17
				state ^= state << 5
				buffer[i] = byte(state)
			case "periodic":
				buffer[i] = byte(position)
			case "runs":
				if position%65 == 64 {
					buffer[i] = 17
				} else {
					buffer[i] = 42
				}
			}
		}
		if _, e = f.Write(buffer[:n]); e != nil {
			return e
		}
		base += n
	}
	return f.Close()
}
func header(path string, level int) error {
	f, e := os.Open(path)
	if e != nil {
		return e
	}
	defer f.Close()
	b := make([]byte, 4)
	if n, e := f.Read(b); e != nil || n != 4 {
		return errors.New("missing BZip2 header")
	}
	if string(b) != "BZh"+strconv.Itoa(level) {
		return fmt.Errorf("unexpected BZip2 block setting %q", b)
	}
	return nil
}

type tools struct {
	driver, sevenzip, pure, c string
	timeout                   time.Duration
	settings                  string
}

func (t tools) execute(argv []string, in, out string) (sample, error) {
	ctx, cancel := context.WithTimeout(context.Background(), t.timeout)
	defer cancel()
	return process(ctx, argv, in, out)
}
func (t tools) ours(s scenario, mode, in, out string) []string {
	argv := []string{t.driver, "--api", s.API, mode, strconv.Itoa(s.Level), strconv.Itoa(s.Threads), in, out}
	if s.API == "native" && t.settings != "sevenzip-level" && mode == "encode" {
		argv = append(argv, strconv.Itoa(s.Level*100000), "1")
	}
	return argv
}
func (t tools) command(which int, s scenario, mode, in, out string) []string {
	if which == 1 {
		return t.ours(s, mode, in, out)
	}
	if which >= 2 {
		binary := t.pure
		if which == 3 {
			binary = t.c
		}
		return []string{binary, mode, strconv.Itoa(s.Level), "1", in, out}
	}
	action := "a"
	if mode == "decode" {
		action = "x"
	}
	argv := []string{t.sevenzip, action, "-tbzip2", fmt.Sprintf("-mmt=%d", s.Threads), "-si", "-so"}
	if mode == "encode" {
		argv = append(argv, fmt.Sprintf("-mx=%d", s.Level), "stream.bz2")
		if s.API == "compat" || t.settings != "sevenzip-level" {
			argv = append(argv, fmt.Sprintf("-md=%db", s.Level*100000), "-mpass=1")
		}
	}
	return argv
}

// policy records effective settings; SDK recursion and standard bzip2's
// single pipeline are distinct algorithms even at matching block sizes.
func (t tools) policy(which int, s scenario) codecPolicy {
	p := codecPolicy{BlockBytes: s.Level * 100000, OptimizationPasses: 1, Policy: "standard-bzip2"}
	if which >= 2 {
		return p
	}
	p.Policy = "7zip-explicit-block-single-pass"
	if s.API == "native" && t.settings == "sevenzip-level" {
		p.Policy = "7zip-level-defaults"
		blocks := []int{1, 1, 3, 5, 7, 9, 9, 9, 9, 9}
		p.BlockBytes = blocks[s.Level] * 100000
		if s.Level == 9 {
			p.OptimizationPasses = 7
		} else if s.Level >= 7 {
			p.OptimizationPasses = 2
		}
	}
	return p
}
func (t tools) measure(s scenario, input, scratch string, repeats, warmups int) ([]row, error) {
	names := []string{"7zz", "turbo", "bzip2-pure", "bzip2-c"}
	packed := make([]string, 4)
	ids := make([]identity, 4)
	sizes := make([]int64, 4)
	decoded := filepath.Join(scratch, "decoded")
	for i, name := range names {
		packed[i] = filepath.Join(scratch, name+".bz2")
		output := os.DevNull
		if i == 0 {
			output = packed[i]
		}
		if _, e := t.execute(t.command(i, s, "encode", input, packed[i]), input, output); e != nil {
			return nil, fmt.Errorf("%s encode: %w", name, e)
		}
		var e error
		ids[i], e = identify(packed[i])
		if e != nil {
			return nil, e
		}
		info, e := os.Stat(packed[i])
		if e != nil {
			return nil, e
		}
		sizes[i] = info.Size()
		if e := header(packed[i], t.policy(i, s).BlockBytes/100000); e != nil {
			return nil, fmt.Errorf("%s effective block size: %w", name, e)
		}
	}
	if s.API == "native" {
		if e := equal(packed[0], packed[1]); e != nil {
			return nil, fmt.Errorf("native 7zz parity: %w", e)
		}
	}
	// Every decoder consumes every encoder's stream before any timing begins,
	// at the scenario's thread count even when decode timing is skipped.
	for encoder, path := range packed {
		for decoder, name := range names {
			output := os.DevNull
			if decoder == 0 {
				output = decoded
			}
			if _, e := t.execute(t.command(decoder, s, "decode", path, decoded), path, output); e != nil {
				return nil, fmt.Errorf("%s decode %s: %w", name, names[encoder], e)
			}
			if e := equal(input, decoded); e != nil {
				return nil, fmt.Errorf("%s decode %s: %w", name, names[encoder], e)
			}
		}
	}
	inputID, e := identify(input)
	if e != nil {
		return nil, e
	}
	info, e := os.Stat(input)
	if e != nil {
		return nil, e
	}
	if s.Corpus == "mkv" && float64(sizes[0])/float64(info.Size()) < 0.99 {
		return nil, errors.New("generated MKV is compressible: 7zz encoded/input fraction is below 0.99")
	}
	if s.Corpus == "elf" && float64(sizes[0])/float64(info.Size()) > 0.90 {
		return nil, errors.New("generated ELF is not materially compressible: 7zz encoded/input fraction exceeds 0.90")
	}
	var rows []row
	// 7zz decode is dropped with ours: every row pairs both at one budget.
	for _, mode := range modes(s) {
		source := input
		if mode == "decode" {
			source = packed[0]
		}
		samples := make([][]sample, 4)
		for repetition := 0; repetition < warmups+repeats; repetition++ {
			for _, which := range order(repetition) {
				value, e := t.execute(t.command(which, s, mode, source, os.DevNull), source, os.DevNull)
				if e != nil {
					return nil, e
				}
				if repetition >= warmups {
					samples[which] = append(samples[which], value)
				}
			}
		}
		for _, reference := range []int{0, 2, 3} {
			n := s.Threads
			if reference >= 2 {
				n = 1
			}
			validation := "all four encoders cross-decoded by all four decoders; native turbo/7zz byte parity checked; other compressed identities may differ"
			if s.API == "compat" {
				validation = "all four encoders cross-decoded by all four decoders; compressed identities may differ"
			}
			r := row{Settings: t.settings, ReferencePolicy: t.policy(reference, s), OursPolicy: t.policy(1, s), SettingsMatch: t.policy(reference, s).BlockBytes == t.policy(1, s).BlockBytes && t.policy(reference, s).OptimizationPasses == t.policy(1, s).OptimizationPasses, Corpus: s.Corpus, API: s.API, Level: s.Level, Threads: s.Threads, ReferenceName: names[reference], ReferenceThreads: n, Mode: mode, Input: inputID, InputBytes: info.Size(), ReferenceOutput: ids[reference], OursOutput: ids[1], ReferenceOutputBytes: sizes[reference], OursOutputBytes: sizes[1], Validation: validation, Reference: samples[reference], Ours: samples[1]}
			summarize(&r)
			rows = append(rows, r)
		}
	}
	return rows, nil
}

func run(args []string) error {
	if len(args) == 0 || args[0] == "-h" || args[0] == "--help" {
		fmt.Println("usage: bzip2-turbo-bench <run|suite|toolchain|fixture> [flags]; suite -h for matrix options")
		return nil
	}
	command := args[0]
	if command != "run" && command != "suite" && command != "toolchain" {
		return fmt.Errorf("unknown command %q", command)
	}
	fs := flag.NewFlagSet(command, flag.ContinueOnError)
	repo := fs.String("repo", "../..", "standalone repository root")
	driver := fs.String("driver", "", "codec-bench binary (default <repo>/target/release/examples/codec-bench)")
	pure := fs.String("pure", "", "pure Rust reference driver")
	c := fs.String("c", "", "C-backed Rust reference driver")
	sevenzip := fs.String("sevenzip", "7zz", "7-Zip reference binary")
	fixtureManifest := fs.String("fixture-manifest", "", "run: canonical fixture JSON receipt; SHA256 and actual bytes must match input")
	input := fs.String("input", "", "run: existing corpus file outside repository")
	out := fs.String("out", "", "JSON report outside repository (default temporary file)")
	settings := fs.String("settings", "block-size", "block-size: level*100000 bytes and one pass; sevenzip-level: native 7zip defaults, differing from standard bzip2")
	level := fs.Int("level", 5, "run: setting index 1..9; meaning selected by --settings")
	levelsText := fs.String("levels", "1,5,9", "suite: comma-separated compression levels")
	threadsText := fs.String("threads", "1,2,4,8", "comma-separated thread counts (1..64)")
	apiText := fs.String("api", "native", "native, compat or both; suite defaults to both")
	corporaText := fs.String("corpora", "text,random,periodic,runs", "suite: text/json, repeated-text, logs, mkv, elf, random, periodic or runs; defaults need no extra tools")
	mkvSource := fs.String("mkv-source", "big-buck-bunny", "suite MKV source: big-buck-bunny or noise")
	mkvSourceFile := fs.String("mkv-source-file", "", "verified public MP4/ZIP outside checkout")
	downloadSource := fs.Bool("download-source", false, "explicitly download pinned public MKV source")
	sizeKiB := fs.Int64("size-kib", 32768, "suite: target KiB for text/logs/noise; public MKV and ELF use actual fixed excerpt/build bytes")
	repeats := fs.Int("repeats", 4, "measured repeats, at least 3")
	warmups := fs.Int("warmups", 1, "discarded four-contender warmup rounds")
	timeout := fs.Duration("timeout", 10*time.Minute, "timeout per child process")
	list := fs.Bool("list", false, "print scenario plan without building, generating or timing")
	if e := fs.Parse(args[1:]); e != nil {
		if errors.Is(e, flag.ErrHelp) {
			return nil
		}
		return e
	}
	if *fixtureManifest != "" && command != "run" {
		return errors.New("fixture-manifest requires run")
	}
	if *settings != "block-size" && *settings != "sevenzip-level" {
		return errors.New("settings must be block-size or sevenzip-level")
	}
	if fs.NArg() != 0 {
		return errors.New("unexpected positional arguments")
	}
	if *repeats < 3 || *warmups < 0 || *level < 1 || *level > 9 || *timeout <= 0 || *sizeKiB < 1 || *sizeKiB > 1<<40 {
		return errors.New("invalid repeat, warmup, level, size or timeout option")
	}
	if command == "suite" {
		explicit := false
		fs.Visit(func(f *flag.Flag) {
			if f.Name == "api" {
				explicit = true
			}
		})
		if !explicit {
			*apiText = "both"
		}
	}
	apis, e := apiNames(*apiText)
	if e != nil {
		return e
	}
	threads, e := integers(*threadsText, 64)
	if e != nil {
		return e
	}
	levels := []int{*level}
	corpora := []string{"input"}
	if command == "suite" {
		levels, e = integers(*levelsText, 9)
		if e != nil {
			return e
		}
		corpora = strings.Split(*corporaText, ",")
		for _, name := range corpora {
			if !validCorpus(name) {
				return fmt.Errorf("unknown corpus %q", name)
			}
		}
	}
	plan, skipped := schedule(corpora, levels, threads, apis)
	if *list {
		b, e := json.MarshalIndent(struct{ Ready, Skipped []scenario }{plan, skipped}, "", "  ")
		if e == nil {
			fmt.Println(string(b))
		}
		return e
	}
	if command != "toolchain" && len(plan) == 0 {
		return errors.New("no runnable scenarios; compatible API requires --threads 1")
	}
	root, e := filepath.Abs(*repo)
	if e != nil {
		return e
	}
	if *mkvSourceFile != "" {
		if e = outside(*mkvSourceFile, root); e != nil {
			return e
		}
	}
	if *driver == "" {
		*driver = filepath.Join(root, "target", "release", "examples", "codec-bench")
	}
	*driver, e = exec.LookPath(*driver)
	if e != nil {
		return e
	}
	*sevenzip, e = exec.LookPath(*sevenzip)
	if e != nil {
		return e
	}
	for _, ref := range []struct {
		path    *string
		backend string
	}{{pure, "pure"}, {c, "c"}} {
		if *ref.path == "" {
			*ref.path = filepath.Join(root, "target", "reference-"+ref.backend, "release", "bzip2-reference-bench")
		}
		*ref.path, e = exec.LookPath(*ref.path)
		if e != nil {
			return e
		}
	}
	pureID, e := identify(*pure)
	if e != nil {
		return e
	}
	cID, e := identify(*c)
	if e != nil {
		return e
	}
	driverID, e := identify(*driver)
	if e != nil {
		return e
	}
	referenceID, e := identify(*sevenzip)
	if e != nil {
		return e
	}
	source, e := sourceDigest(root)
	if e != nil {
		return e
	}
	ctx, cancel := context.WithTimeout(context.Background(), *timeout)
	defer cancel()
	version, e := exec.CommandContext(ctx, *sevenzip, "i").Output()
	if e != nil {
		return e
	}
	revision := "unavailable (source SHA256 recorded)"
	if b, e := exec.Command("git", "-C", root, "rev-parse", "HEAD").Output(); e == nil {
		revision = strings.TrimSpace(string(b))
	}
	r := report{Schema: 5, Settings: *settings, Pure: pureID, C: cID, Platform: runtime.GOOS + "/" + runtime.GOARCH, StartedUTC: time.Now().UTC().Format(time.RFC3339), SourceSHA256: source, Revision: revision, ReferenceVersion: strings.TrimSpace(string(version)), Driver: driverID, Reference: referenceID, Repeats: *repeats, Warmups: *warmups, RSSMethod: rssMethod, OutputSink: os.DevNull, RSSSupported: rssSupported, RSSScope: rssDescription, TimingScope: processTimingDescription, PerformanceCounters: "not collected: architecture/OS hardware counters require a separate diagnostic collector", Skipped: skipped}
	if command == "toolchain" {
		b, e := json.MarshalIndent(r, "", "  ")
		if e == nil {
			fmt.Println(string(b))
		}
		return e
	}
	if *out == "" {
		f, e := os.CreateTemp("", "bzip2-bench-*.json")
		if e != nil {
			return e
		}
		*out = f.Name()
		f.Close()
	}
	if e = outside(*out, root); e != nil {
		return e
	}
	scratch, e := os.MkdirTemp("", "bzip2-bench-")
	if e != nil {
		return e
	}
	defer os.RemoveAll(scratch)
	paths := map[string]string{}
	if command == "run" {
		if *input == "" {
			return errors.New("run requires -input")
		}
		if e = outside(*input, root); e != nil {
			return e
		}
		paths["input"] = *input
		if *fixtureManifest != "" {
			if e = outside(*fixtureManifest, root); e != nil {
				return e
			}
			info, err := readFixtureManifest(*fixtureManifest, *input)
			if err != nil {
				return err
			}
			r.Fixtures = append(r.Fixtures, info)
			for i := range plan {
				plan[i].Corpus = info.Kind
			}
			paths[info.Kind] = *input
		}
	} else {
		for _, name := range corpora {
			path := filepath.Join(scratch, name+corpusExtension(name))
			info, err := createFixtureWithOptions(path, name, *sizeKiB*1024, fixtureOptions{MKVSource: *mkvSource, SourceFile: *mkvSourceFile, Download: *downloadSource})
			if err != nil {
				return err
			}
			r.Fixtures = append(r.Fixtures, info)
			paths[name] = path
		}
	}
	tools := tools{driver: *driver, sevenzip: *sevenzip, pure: *pure, c: *c, timeout: *timeout, settings: *settings}
	for _, s := range plan {
		fmt.Fprintf(os.Stderr, "%s level=%d threads=%d api=%s\n", s.Corpus, s.Level, s.Threads, s.API)
		rows, e := tools.measure(s, paths[s.Corpus], scratch, *repeats, *warmups)
		if e != nil {
			return fmt.Errorf("%s level=%d threads=%d api=%s: %w", s.Corpus, s.Level, s.Threads, s.API, e)
		}
		r.Rows = append(r.Rows, rows...)
	}
	b, e := json.MarshalIndent(r, "", "  ")
	if e != nil {
		return e
	}
	if e = os.WriteFile(*out, append(b, '\n'), 0644); e != nil {
		return e
	}
	fmt.Println(*out)
	return nil
}
