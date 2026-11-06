// Historical two-process benchmark protocol, retained for reproducibility.
package main

import (
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

func legacyBlocks() map[string][]byte {
	const size = 1 << 20
	text := []byte("A reproducible compression corpus: records, words, numbers 0123456789.\n")
	random := make([]byte, size)
	state := uint32(0x12345678)
	for i := range random {
		state ^= state << 13
		state ^= state >> 17
		state ^= state << 5
		random[i] = byte(state)
	}
	blocks := map[string][]byte{"text": make([]byte, size), "random": random, "runs": make([]byte, size), "mixed": make([]byte, size)}
	for i := 0; i < size; i++ {
		blocks["text"][i] = text[i%len(text)]
		blocks["runs"][i] = 42
		if i%65 == 64 {
			blocks["runs"][i] = 17
		}
		if i < size/2 {
			blocks["mixed"][i] = random[i]
		} else {
			blocks["mixed"][i] = byte(i - size/2)
		}
	}
	return blocks
}
func legacyCorpus(args []string) error {
	// argparse accepted options before or after the positional destination.
	var options, positional []string
	for i := 0; i < len(args); i++ {
		arg := args[i]
		if arg == "--" {
			positional = append(positional, args[i+1:]...)
			break
		}
		if strings.HasPrefix(arg, "-") {
			options = append(options, arg)
			if arg == "--mib" || arg == "-mib" {
				if i+1 >= len(args) {
					return errors.New("--mib requires a value")
				}
				i++
				options = append(options, args[i])
			}
		} else {
			positional = append(positional, arg)
		}
	}
	fs := flag.NewFlagSet("legacy-corpus", flag.ContinueOnError)
	mib := fs.Int("mib", 32, "MiB per corpus")
	if err := fs.Parse(options); err != nil {
		return err
	}
	if *mib < 1 || len(positional) != 1 {
		return errors.New("legacy-corpus DESTINATION [--mib 32]; size must be positive")
	}
	destination := positional[0]
	root, err := legacyRoot()
	if err != nil {
		return err
	}
	if err = outside(destination, root); err != nil {
		return err
	}
	if err = os.MkdirAll(destination, 0755); err != nil {
		return err
	}
	for name, block := range legacyBlocks() {
		f, err := os.Create(filepath.Join(destination, name+".bin"))
		if err != nil {
			return err
		}
		for i := 0; i < *mib; i++ {
			if _, err = f.Write(block); err != nil {
				f.Close()
				return err
			}
		}
		if err = f.Close(); err != nil {
			return err
		}
	}
	return nil
}
func legacyRoot() (string, error) {
	// Locate the repository from the source file, independent of invocation cwd.
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		return "", errors.New("cannot locate benchmark source")
	}
	return filepath.Clean(filepath.Join(filepath.Dir(file), "../..")), nil
}

var darwinCounters = map[string]string{
	"instructions retired": "instructions_retired", "cycles elapsed": "cycles_elapsed", "maximum resident set size": "maximum_resident_set_size_bytes", "peak memory footprint": "peak_memory_footprint_bytes", "voluntary context switches": "voluntary_context_switches", "involuntary context switches": "involuntary_context_switches", "page reclaims": "page_reclaims", "page faults": "page_faults",
}
var darwinPositive = []string{"instructions_retired", "cycles_elapsed", "maximum_resident_set_size_bytes", "peak_memory_footprint_bytes"}

func parseDarwinResources(raw string) (map[string]uint64, error) {
	parsed := map[string]uint64{}
	for _, line := range strings.Split(raw, "\n") {
		fields := strings.Fields(line)
		if len(fields) < 2 {
			continue
		}
		key, ok := darwinCounters[strings.Join(fields[1:], " ")]
		if !ok {
			continue
		}
		n, err := strconv.ParseUint(fields[0], 10, 64)
		if err != nil {
			continue
		}
		if _, ok = parsed[key]; ok {
			return nil, fmt.Errorf("duplicate Darwin resource counter: %s", key)
		}
		parsed[key] = n
	}
	for _, key := range darwinCounters {
		if _, ok := parsed[key]; !ok {
			return nil, fmt.Errorf("missing Darwin resource counter: %s", key)
		}
	}
	for _, key := range darwinPositive {
		if parsed[key] == 0 {
			return nil, fmt.Errorf("unavailable Darwin resource counter: %s", key)
		}
	}
	return parsed, nil
}
func legacyMedian(values []float64) float64 {
	a := append([]float64(nil), values...)
	sort.Float64s(a)
	if len(a)%2 == 1 {
		return a[len(a)/2]
	}
	return (a[len(a)/2-1] + a[len(a)/2]) / 2
}

type legacyMeasurement struct {
	Mode            string             `json:"mode"`
	Implementation  string             `json:"implementation"`
	Threads         int                `json:"threads"`
	Seconds         []float64          `json:"seconds"`
	Median          float64            `json:"median_seconds"`
	Throughput      float64            `json:"mib_per_second"`
	Resources       []map[string]any   `json:"darwin_resource_runs,omitempty"`
	ResourceMedians map[string]float64 `json:"darwin_resource_medians,omitempty"`
	MaxRSS          uint64             `json:"maximum_rss_bytes,omitempty"`
	MaxFootprint    uint64             `json:"maximum_peak_memory_footprint_bytes,omitempty"`
}

func legacyParity(items []legacyMeasurement) (map[string]any, error) {
	type key struct {
		mode    string
		threads int
	}
	pairs := map[key]map[string]float64{}
	var order []key
	for _, item := range items {
		k := key{item.Mode, item.Threads}
		if pairs[k] == nil {
			pairs[k] = map[string]float64{}
			order = append(order, k)
		}
		if _, ok := pairs[k][item.Implementation]; ok {
			return nil, errors.New("duplicate parity measurement")
		}
		pairs[k][item.Implementation] = item.Median
	}
	failures := []map[string]any{}
	for _, k := range order {
		p := pairs[k]
		reference, r := p["reference"]
		candidate, c := p["candidate"]
		if len(p) != 2 || !r || !c {
			return nil, errors.New("incomplete parity measurements")
		}
		if candidate > reference {
			failures = append(failures, map[string]any{"mode": k.mode, "threads": k.threads, "reference_median_seconds": reference, "candidate_median_seconds": candidate})
		}
	}
	return map[string]any{"passed": len(failures) == 0, "comparisons": len(pairs), "criterion": "candidate median_seconds <= reference median_seconds, using unrounded values and no tolerance", "host_quality": "not assessed: the script cannot establish host idleness or measurement stability", "failures": failures}, nil
}
func legacyExecute(args []string, input, output string) (float64, error) {
	in, err := os.Open(input)
	if err != nil {
		return 0, err
	}
	defer in.Close()
	out, err := os.Create(output)
	if err != nil {
		return 0, err
	}
	defer out.Close()
	cmd := exec.Command(args[0], args[1:]...)
	cmd.Stdin = in
	cmd.Stdout = out
	cmd.Stderr = os.Stderr
	start := time.Now()
	err = cmd.Run()
	return time.Since(start).Seconds(), err
}
func legacyEmit(value any) error { return json.NewEncoder(os.Stdout).Encode(value) }
func legacyCompare(args []string) error {
	fs := flag.NewFlagSet("legacy-compare", flag.ContinueOnError)
	codec := fs.String("codec", "", "candidate binary")
	oracle := fs.String("oracle", "", "7zz binary")
	input := fs.String("input", "", "input file")
	results := fs.String("results", "", "JSON outside checkout")
	level := fs.Int("level", 5, "level 1..9")
	runs := fs.Int("runs", 3, "at least three rounds")
	warmups := fs.Int("warmups", 1, "at least one warmup")
	block := fs.Int("block-size", 0, "optional block bytes")
	passes := fs.Int("passes", 0, "optional passes")
	hardware := fs.String("hardware", runtime.GOARCH, "hardware identity")
	revision := fs.String("revision", "unspecified", "source identity")
	profile := fs.String("profile", "release", "build profile")
	resources := fs.Bool("darwin-resources", false, "Darwin time -l counters")
	required := fs.Bool("require-parity", false, "strict unrounded median gate")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if fs.NArg() != 0 || *codec == "" || *oracle == "" || *input == "" || *results == "" {
		return errors.New("--codec, --oracle, --input and --results are required")
	}
	if *runs < 3 || *warmups < 1 || *level < 1 || *level > 9 {
		return errors.New("require runs >=3, warmups >=1 and level 1..9")
	}
	if *resources && runtime.GOOS != "darwin" {
		return errors.New("--darwin-resources requires Darwin /usr/bin/time -l")
	}
	root, err := legacyRoot()
	if err != nil {
		return err
	}
	if err = outside(*results, root); err != nil {
		return err
	}
	report := map[string]any{"platform": runtime.GOOS, "machine": runtime.GOARCH, "hardware": *hardware, "logical_cpus": runtime.NumCPU(), "revision": *revision, "profile": *profile, "warmups": *warmups, "order": "alternating per repetition", "level": *level, "runs": *runs, "block_size": nil, "passes": nil, "measurements": []legacyMeasurement{}, "ratios": []map[string]any{}}
	options := []string{"-", "-"}
	fs.Visit(func(f *flag.Flag) {
		if f.Name == "block-size" {
			report["block_size"] = *block
			options[0] = strconv.Itoa(*block)
		}
		if f.Name == "passes" {
			report["passes"] = *passes
			options[1] = strconv.Itoa(*passes)
		}
	})
	for key, path := range map[string]string{"codec_sha256": *codec, "oracle_sha256": *oracle, "input_sha256": *input} {
		hash, err := digest(path)
		if err != nil {
			return err
		}
		report[key] = hash
	}
	stat, err := os.Stat(*input)
	if err != nil {
		return err
	}
	report["input_bytes"] = stat.Size()
	if *resources {
		report["darwin_resources"] = map[string]any{"collector": "/usr/bin/time -l -o FILE", "wall_measurement": "perf_counter around /usr/bin/time and its child; includes wrapper startup and counter reporting", "memory_unit": "bytes", "counter_scope": "command process as reported by Darwin time; warmups excluded from recorded runs"}
	}
	temporary, err := os.MkdirTemp("", "bzip2-parity-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(temporary)
	packed, candidate, decoded := filepath.Join(temporary, "oracle.bz2"), filepath.Join(temporary, "candidate.bz2"), filepath.Join(temporary, "decoded")
	measurements := []legacyMeasurement{}
	ratios := []map[string]any{}
	for _, threads := range []int{1, 2, 4, 8} {
		thread := strconv.Itoa(threads)
		levelText := strconv.Itoa(*level)
		encode := []string{*oracle, "a", "-tbzip2", "-mx=" + levelText, "-mmt=" + thread, "-si", "-so", "stream.bz2"}
		if options[0] != "-" {
			encode = append(encode, "-md="+options[0]+"b")
		}
		if options[1] != "-" {
			encode = append(encode, "-mpass="+options[1])
		}
		decode := []string{*oracle, "x", "-tbzip2", "-mmt=" + thread, "-si", "-so"}
		if _, err = legacyExecute(encode, *input, packed); err != nil {
			return err
		}
		cmd := exec.Command(*codec, append([]string{"encode", levelText, thread, *input, candidate}, options...)...)
		cmd.Stderr = os.Stderr
		if err = cmd.Run(); err != nil {
			return err
		}
		if err = equal(candidate, packed); err != nil {
			return fmt.Errorf("encoded bytes differ at %d threads: %w", threads, err)
		}
		cmd = exec.Command(*codec, "decode", levelText, thread, packed, decoded)
		cmd.Stderr = os.Stderr
		if err = cmd.Run(); err != nil {
			return err
		}
		if err = equal(decoded, *input); err != nil {
			return fmt.Errorf("decoded bytes differ at %d threads: %w", threads, err)
		}
		for _, mode := range []string{"encode", "decode"} {
			if mode == "decode" && threads > nativeDecodeThreadCap {
				// The native decoder runs at most two threads; the bytes were
				// still checked above at this thread count.
				continue
			}
			source := *input
			reference := encode
			if mode == "decode" {
				source = packed
				reference = decode
			}
			commands := map[string][]string{"reference": reference, "candidate": append([]string{*codec, mode, levelText, thread, source, "-"}, options...)}
			timings := map[string][]float64{"reference": {}, "candidate": {}}
			resourcesBy := map[string][]map[string]any{"reference": {}, "candidate": {}}
			for repetition := 0; repetition < *warmups+*runs; repetition++ {
				order := []string{"reference", "candidate"}
				if repetition%2 == 1 {
					order = []string{"candidate", "reference"}
				}
				for _, implementation := range order {
					command := commands[implementation]
					resourcePath := filepath.Join(temporary, fmt.Sprintf("resources-%d-%s-%s-%d.txt", threads, mode, implementation, repetition))
					if *resources {
						command = append([]string{"/usr/bin/time", "-l", "-o", resourcePath}, command...)
					}
					elapsed, err := legacyExecute(command, source, os.DevNull)
					if err != nil {
						return err
					}
					var resource map[string]any
					if *resources {
						raw, err := os.ReadFile(resourcePath)
						if err != nil {
							return err
						}
						parsed, err := parseDarwinResources(string(raw))
						if err != nil {
							return err
						}
						resource = map[string]any{"raw": string(raw), "parsed": parsed}
					}
					if repetition >= *warmups {
						timings[implementation] = append(timings[implementation], elapsed)
						if *resources {
							resourcesBy[implementation] = append(resourcesBy[implementation], resource)
						}
					}
				}
			}
			medians := map[string]map[string]float64{}
			for _, implementation := range []string{"reference", "candidate"} {
				m := legacyMedian(timings[implementation])
				item := legacyMeasurement{Mode: mode, Implementation: implementation, Threads: threads, Seconds: timings[implementation], Median: m, Throughput: float64(stat.Size()) / m / (1 << 20)}
				if *resources {
					item.Resources = resourcesBy[implementation]
					item.ResourceMedians = map[string]float64{}
					for _, key := range darwinCounters {
						values := []float64{}
						for _, r := range item.Resources {
							values = append(values, float64(r["parsed"].(map[string]uint64)[key]))
						}
						item.ResourceMedians[key] = legacyMedian(values)
					}
					for _, r := range item.Resources {
						p := r["parsed"].(map[string]uint64)
						item.MaxRSS = max(item.MaxRSS, p["maximum_resident_set_size_bytes"])
						item.MaxFootprint = max(item.MaxFootprint, p["peak_memory_footprint_bytes"])
					}
					medians[implementation] = item.ResourceMedians
				}
				measurements = append(measurements, item)
				if err = legacyEmit(item); err != nil {
					return err
				}
			}
			referenceMedian, candidateMedian := legacyMedian(timings["reference"]), legacyMedian(timings["candidate"])
			ratio := map[string]any{"mode": mode, "threads": threads, "candidate_over_reference_seconds": candidateMedian / referenceMedian, "candidate_over_reference_throughput": referenceMedian / candidateMedian}
			if *resources {
				values := map[string]float64{}
				for _, key := range darwinPositive {
					values[key] = medians["candidate"][key] / medians["reference"][key]
				}
				ratio["candidate_over_reference_resource_medians"] = values
			}
			ratios = append(ratios, ratio)
			if err = legacyEmit(ratio); err != nil {
				return err
			}
		}
	}
	parity, err := legacyParity(measurements)
	if err != nil {
		return err
	}
	parity["required"] = *required
	report["measurements"] = measurements
	report["ratios"] = ratios
	report["measured_parity"] = parity
	output, err := json.MarshalIndent(report, "", "  ")
	if err != nil {
		return err
	}
	if err = os.WriteFile(*results, append(output, '\n'), 0644); err != nil {
		return err
	}
	if err = legacyEmit(map[string]any{"measured_parity": parity}); err != nil {
		return err
	}
	if *required && !parity["passed"].(bool) {
		return errors.New("measured parity failed")
	}
	return nil
}
