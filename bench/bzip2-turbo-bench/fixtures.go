package main

import (
	"bufio"
	"bytes"
	"context"
	"crypto/sha256"
	"debug/elf"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

type fixtureInfo struct {
	Kind           string       `json:"kind"`
	Input          identity     `json:"input"`
	Bytes          int64        `json:"bytes"`
	RequestedBytes int64        `json:"requested_bytes,omitempty"`
	Generator      string       `json:"generator"`
	ToolVersion    string       `json:"tool_version,omitempty"`
	Parameters     []string     `json:"parameters,omitempty"`
	Seed           string       `json:"seed,omitempty"`
	RawSHA256      string       `json:"raw_sha256,omitempty"`
	SourceSHA256   string       `json:"source_sha256,omitempty"`
	Media          *mediaInfo   `json:"media,omitempty"`
	Source         *mediaSource `json:"source,omitempty"`
}
type mediaInfo struct {
	Codec           string  `json:"codec"`
	AudioCodec      string  `json:"audio_codec,omitempty"`
	DurationSeconds float64 `json:"duration_seconds,omitempty"`
	PixelFormat     string  `json:"pixel_format"`
	Width           int     `json:"width"`
	Height          int     `json:"height"`
	Frames          int64   `json:"frames"`
	FFprobeVersion  string  `json:"ffprobe_version"`
}

func validCorpus(kind string) bool {
	switch kind {
	case "text", "json", "repeated-text", "logs", "mkv", "elf", "random", "periodic", "runs":
		return true
	}
	return false
}
func corpusExtension(kind string) string {
	switch kind {
	case "json":
		return ".json"
	case "repeated-text":
		return ".txt"
	case "logs":
		return ".log"
	case "mkv":
		return ".mkv"
	case "elf":
		return ".elf"
	}
	return ".bin"
}

const repeatedSentence = "A reproducible compression corpus: records, words, numbers 0123456789.\n"

func generateRepeatedText(output io.Writer, size int64) error {
	if size < 1 {
		return errors.New("repeated text size must be positive")
	}
	block := bytes.Repeat([]byte(repeatedSentence), 1024)
	for size > 0 {
		n := min(size, int64(len(block)))
		if _, err := output.Write(block[:n]); err != nil {
			return err
		}
		size -= n
	}
	return nil
}

func generateLogs(output io.Writer, size int64) error {
	if size < 1 {
		return errors.New("log corpus size must be positive")
	}
	writer := bufio.NewWriterSize(output, 65536)
	random := jsonRandom(0x510e527fade682d1)
	timestamp := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	services := []string{"api", "worker", "cache", "scheduler", "indexer"}
	events := []string{"request_completed", "job_completed", "cache_refresh", "retry_scheduled", "connection_opened", "batch_committed", "queue_sample", "resource_check"}
	messages := []string{"request completed", "background task finished", "cached entry refreshed", "retry scheduled after transient failure", "connection established", "batch committed successfully", "queue depth sampled", "resource check completed"}
	methods := []string{"GET", "GET", "GET", "POST", "PUT", "DELETE"}
	statuses := []int{200, 200, 200, 200, 201, 204, 304, 400, 404, 429, 500, 503}
	var written int64
	for id := uint64(0); written < size; id++ {
		timestamp = timestamp.Add(time.Duration(100+random.next()%250000) * time.Microsecond)
		level := "INFO"
		switch n := random.next() % 100; {
		case n < 20:
			level = "DEBUG"
		case n >= 98:
			level = "ERROR"
		case n >= 90:
			level = "WARN"
		}
		event := int(random.next() % uint64(len(events)))
		line := fmt.Sprintf("%s level=%s service=%s node=node-%02d event=%s request_id=%016x method=%s path=%q status=%d duration_ms=%d bytes=%d queue=%d remote=192.0.2.%d message=%q\n", timestamp.Format(time.RFC3339Nano), level, services[random.next()%uint64(len(services))], random.next()%16, events[event], id, methods[random.next()%uint64(len(methods))], fmt.Sprintf("/api/v1/%s/%d", jsonVocabulary[random.next()%uint64(len(jsonVocabulary))], random.next()%100000), statuses[random.next()%uint64(len(statuses))], random.next()%2000, random.next()%1048576, random.next()%4096, 1+random.next()%254, messages[event])
		n, e := io.WriteString(writer, line)
		if e != nil {
			return e
		}
		written += int64(n)
	}
	return writer.Flush()
}
func toolVersion(name string, args ...string) (string, error) {
	b, e := exec.Command(name, args...).Output()
	if e != nil {
		return "", e
	}
	return strings.SplitN(strings.TrimSpace(string(b)), "\n", 2)[0], nil
}
func fixture(args []string) error {
	fs := flag.NewFlagSet("fixture", flag.ContinueOnError)
	kind := fs.String("kind", "logs", "json, repeated-text, logs, mkv or elf")
	output := fs.String("output", "", "fixture file outside checkout")
	source := fs.String("source", "big-buck-bunny", "MKV: big-buck-bunny or noise")
	sourceFile := fs.String("source-file", "", "verified Big Buck Bunny MP4 or ZIP outside checkout")
	download := fs.Bool("download-source", false, "explicitly download the pinned public source archive")
	size := fs.Int64("bytes", 32<<20, "JSON/repeated-text bytes, minimum log bytes or raw MKV noise bytes; ignored for public-film MKV and ELF")
	if e := fs.Parse(args); e != nil {
		return e
	}
	if fs.NArg() != 0 || *output == "" || (*kind != "json" && *kind != "repeated-text" && *kind != "logs" && *kind != "mkv" && *kind != "elf") {
		return errors.New("fixture --kind json|repeated-text|logs|mkv|elf --output PATH [--bytes N]")
	}
	root, e := legacyRoot()
	if e != nil {
		return e
	}
	if e = outside(*output, root); e != nil {
		return e
	}
	if *sourceFile != "" {
		if e = outside(*sourceFile, root); e != nil {
			return e
		}
	}
	info, e := createFixtureWithOptions(*output, *kind, *size, fixtureOptions{MKVSource: *source, SourceFile: *sourceFile, Download: *download})
	if e != nil {
		return e
	}
	return json.NewEncoder(os.Stdout).Encode(info)
}

type fixtureOptions struct {
	MKVSource  string
	SourceFile string
	Download   bool
}

func createFixture(path, kind string, size int64) (fixtureInfo, error) {
	return createFixtureWithOptions(path, kind, size, fixtureOptions{MKVSource: "big-buck-bunny"})
}
func createFixtureWithOptions(path, kind string, size int64, options fixtureOptions) (info fixtureInfo, err error) {
	path, err = filepath.Abs(path)
	if err != nil {
		return info, err
	}
	info = fixtureInfo{Kind: kind, RequestedBytes: size, Generator: "Go standard library"}
	if !validCorpus(kind) || size < 1 {
		return info, errors.New("invalid fixture kind or size")
	}
	temporary, e := os.CreateTemp(filepath.Dir(path), ".codec-fixture-*")
	if e != nil {
		return info, e
	}
	name := temporary.Name()
	_ = temporary.Close()
	defer os.Remove(name)
	switch kind {
	case "json", "repeated-text", "logs":
		info.Seed = "xorshift64*:510e527fade682d1"
		if kind == "json" {
			info.Seed = "xorshift64*:6a09e667f3bcc909"
		}
		if kind == "repeated-text" {
			info.Seed = "continuous fixed 71-byte sentence"
		}
		f, e := os.Create(name)
		if e != nil {
			return info, e
		}
		if kind == "json" {
			e = generateJSON(f, size)
		} else if kind == "repeated-text" {
			e = generateRepeatedText(f, size)
		} else {
			e = generateLogs(f, size)
		}
		closeErr := f.Close()
		if e != nil {
			return info, e
		}
		if closeErr != nil {
			return info, closeErr
		}
	case "mkv":
		if options.MKVSource == "noise" {
			info, e = generateNoiseMKV(name, size)
		} else if options.MKVSource == "big-buck-bunny" {
			info, e = generateBBB(name, options)
		} else {
			return info, errors.New("unknown MKV source")
		}
		if e != nil {
			return info, e
		}
	case "elf":
		info, e = generateELF(name)
		if e != nil {
			return info, e
		}
	default:
		if e = generate(name, kind, size); e != nil {
			return info, e
		}
	}
	stat, e := os.Stat(name)
	if e != nil {
		return info, e
	}
	info.Bytes = stat.Size()
	info.Input, e = identify(name)
	if e != nil {
		return info, e
	}
	info.Input.Name = filepath.Base(path)
	if e = os.Rename(name, path); e != nil {
		return info, e
	}
	return info, nil
}

const frameWidth = 256
const frameHeight = 256
const frameBytes = frameWidth * frameHeight

func noise(output io.Writer, frames int64) (string, error) {
	random := jsonRandom(0x1f83d9abfb41bd6b)
	buffer := make([]byte, frameBytes)
	hash := sha256.New()
	writer := io.MultiWriter(output, hash)
	for frame := int64(0); frame < frames; frame++ {
		for i := 0; i < len(buffer); i += 8 {
			binary.LittleEndian.PutUint64(buffer[i:], random.next())
		}
		if _, e := writer.Write(buffer); e != nil {
			return "", e
		}
	}
	return hex.EncodeToString(hash.Sum(nil)), nil
}
func mkvParameters() []string {
	return []string{"-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-cpuflags", "0", "-f", "rawvideo", "-pixel_format", "gray", "-video_size", "256x256", "-framerate", "30", "-i", "pipe:0", "-an", "-c:v", "ffv1", "-level", "3", "-coder", "1", "-context", "1", "-g", "1", "-slices", "4", "-slicecrc", "1", "-threads", "1", "-fflags", "+bitexact", "-flags:v", "+bitexact", "-map_metadata", "-1", "-map_chapters", "-1", "-metadata", "creation_time=", "-metadata", "encoder=", "-metadata:s:v", "encoder=", "-f", "matroska"}
}
func generateNoiseMKV(path string, size int64) (fixtureInfo, error) {
	info := fixtureInfo{Kind: "mkv", RequestedBytes: size, Generator: "FFmpeg FFV1 lossless seeded gray noise", Seed: "xorshift64*:1f83d9abfb41bd6b"}
	if size < 1 || size > 1<<40 {
		return info, errors.New("MKV raw extent must be 1..2^40 bytes")
	}
	version, e := toolVersion("ffmpeg", "-version")
	if e != nil {
		return info, fmt.Errorf("FFmpeg is required: %w", e)
	}
	info.ToolVersion = version
	frames := size / frameBytes
	if size%frameBytes != 0 {
		frames++
	}
	info.Parameters = append(mkvParameters(), "OUTPUT")
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Minute)
	defer cancel()
	cmd := exec.CommandContext(ctx, "ffmpeg", append(mkvParameters(), path)...)
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	input, e := cmd.StdinPipe()
	if e != nil {
		return info, e
	}
	if e = cmd.Start(); e != nil {
		return info, e
	}
	raw, e := noise(input, frames)
	closeErr := input.Close()
	waitErr := cmd.Wait()
	if e != nil {
		return info, e
	}
	if closeErr != nil {
		return info, closeErr
	}
	if waitErr != nil {
		return info, fmt.Errorf("FFmpeg generation failed: %w: %s", waitErr, stderr.String())
	}
	info.RawSHA256 = raw
	media, e := validateMKV(path, frames, raw)
	if e != nil {
		return info, e
	}
	info.Media = &media
	return info, nil
}
func validateMKV(path string, frames int64, raw string) (mediaInfo, error) {
	var result mediaInfo
	version, e := toolVersion("ffprobe", "-version")
	if e != nil {
		return result, fmt.Errorf("ffprobe is required: %w", e)
	}
	result.FFprobeVersion = version
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Minute)
	defer cancel()
	b, e := exec.CommandContext(ctx, "ffprobe", "-v", "error", "-count_frames", "-select_streams", "v:0", "-show_entries", "stream=codec_name,pix_fmt,width,height,nb_read_frames:format=format_name,size", "-of", "json", path).Output()
	if e != nil {
		return result, fmt.Errorf("ffprobe validation failed: %w", e)
	}
	var probe struct {
		Streams []struct {
			Codec       string `json:"codec_name"`
			PixelFormat string `json:"pix_fmt"`
			Width       int    `json:"width"`
			Height      int    `json:"height"`
			Frames      string `json:"nb_read_frames"`
		} `json:"streams"`
		Format struct {
			Name string `json:"format_name"`
			Size string `json:"size"`
		} `json:"format"`
	}
	if e = json.Unmarshal(b, &probe); e != nil {
		return result, e
	}
	if len(probe.Streams) != 1 || !strings.Contains(probe.Format.Name, "matroska") {
		return result, errors.New("fixture is not a one-stream Matroska video")
	}
	stream := probe.Streams[0]
	count, e := strconv.ParseInt(stream.Frames, 10, 64)
	if e != nil {
		return result, e
	}
	if stream.Codec != "ffv1" || stream.PixelFormat != "gray" || stream.Width != frameWidth || stream.Height != frameHeight || count != frames {
		return result, errors.New("unexpected Matroska stream extent or format")
	}
	stat, e := os.Stat(path)
	if e != nil {
		return result, e
	}
	size, e := strconv.ParseInt(probe.Format.Size, 10, 64)
	if e != nil || size != stat.Size() {
		return result, errors.New("Matroska byte extent mismatch")
	}
	hash := sha256.New()
	counter := &countWriter{writer: hash}
	cmd := exec.CommandContext(ctx, "ffmpeg", "-hide_banner", "-loglevel", "error", "-xerror", "-cpuflags", "0", "-i", path, "-map", "0:v:0", "-threads", "1", "-pix_fmt", "gray", "-f", "rawvideo", "pipe:1")
	cmd.Stdout = counter
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	if e = cmd.Run(); e != nil {
		return result, fmt.Errorf("Matroska full decode failed: %w: %s", e, stderr.String())
	}
	if counter.bytes != frames*frameBytes || hex.EncodeToString(hash.Sum(nil)) != raw {
		return result, errors.New("Matroska decoded noise differs from seeded source")
	}
	result.Codec, result.PixelFormat, result.Width, result.Height, result.Frames = stream.Codec, stream.PixelFormat, stream.Width, stream.Height, frames
	return result, nil
}

type countWriter struct {
	writer io.Writer
	bytes  int64
}

func (w *countWriter) Write(b []byte) (int, error) {
	n, e := w.writer.Write(b)
	w.bytes += int64(n)
	return n, e
}

const elfSource = `package main
import("bufio";"encoding/json";"fmt";"os";"sort")
type event struct {Level string; Service string; Bytes int64}
func main(){counts:=map[string]int64{};var total int64;scanner:=bufio.NewScanner(os.Stdin);for scanner.Scan(){var item event;if json.Unmarshal(scanner.Bytes(),&item)==nil{counts[item.Service+":"+item.Level]++;total+=item.Bytes}};if scanner.Err()!=nil{fmt.Fprintln(os.Stderr,"input failed");os.Exit(1)};keys:=make([]string,0,len(counts));for key:=range counts{keys=append(keys,key)};sort.Strings(keys);for _,key:=range keys{fmt.Printf("%s %d\n",key,counts[key])};fmt.Printf("bytes %d\n",total)}
`

func fixedEnv(values map[string]string) []string {
	var env []string
	for _, entry := range os.Environ() {
		key := strings.SplitN(entry, "=", 2)[0]
		if _, replace := values[key]; !replace {
			env = append(env, entry)
		}
	}
	for key, value := range values {
		env = append(env, key+"="+value)
	}
	return env
}
func generateELF(path string) (fixtureInfo, error) {
	info := fixtureInfo{Kind: "elf", Generator: "Go standard-library JSON event summarizer"}
	versionCommand := exec.Command("go", "env", "GOVERSION")
	versionCommand.Env = fixedEnv(map[string]string{"GOTOOLCHAIN": "local", "GOENV": "off", "GOWORK": "off"})
	versionBytes, e := versionCommand.Output()
	version := strings.TrimSpace(string(versionBytes))
	if e != nil {
		return info, fmt.Errorf("Go is required: %w", e)
	}
	info.ToolVersion = version
	hash := sha256.Sum256([]byte(elfSource))
	info.SourceSHA256 = hex.EncodeToString(hash[:])
	source, e := os.MkdirTemp("", "codec-elf-source-*")
	if e != nil {
		return info, e
	}
	defer os.RemoveAll(source)
	if e = os.WriteFile(filepath.Join(source, "go.mod"), []byte("module example.invalid/fixture/metrics\n\ngo 1.23\n"), 0600); e != nil {
		return info, e
	}
	if e = os.WriteFile(filepath.Join(source, "main.go"), []byte(elfSource), 0600); e != nil {
		return info, e
	}
	info.Parameters = []string{"CGO_ENABLED=0", "GOOS=linux", "GOARCH=amd64", "GOAMD64=v1", "GOTOOLCHAIN=local", "GOFLAGS=", "GOWORK=off", "GOENV=off", "GOPROXY=off", "GOSUMDB=off", "go", "build", "-trimpath", "-buildvcs=false", "-ldflags=-buildid=", "-o", "OUTPUT", "."}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Minute)
	defer cancel()
	cmd := exec.CommandContext(ctx, "go", "build", "-trimpath", "-buildvcs=false", "-ldflags=-buildid=", "-o", path, ".")
	cmd.Dir = source
	cmd.Env = fixedEnv(map[string]string{"CGO_ENABLED": "0", "GOOS": "linux", "GOARCH": "amd64", "GOAMD64": "v1", "GOTOOLCHAIN": "local", "GOFLAGS": "", "GOWORK": "off", "GOENV": "off", "GOPROXY": "off", "GOSUMDB": "off", "GOEXPERIMENT": ""})
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	if e = cmd.Run(); e != nil {
		return info, fmt.Errorf("ELF build failed: %w: %s", e, stderr.String())
	}
	if e = validateELF(path); e != nil {
		return info, e
	}
	return info, nil
}
func validateELF(path string) error {
	file, e := elf.Open(path)
	if e != nil {
		return e
	}
	defer file.Close()
	if file.Class != elf.ELFCLASS64 || file.Data != elf.ELFDATA2LSB || file.Machine != elf.EM_X86_64 || (file.Type != elf.ET_EXEC && file.Type != elf.ET_DYN) {
		return errors.New("unexpected ELF architecture or type")
	}
	text := file.Section(".text")
	if text == nil || text.Size == 0 {
		return errors.New("ELF has no executable text")
	}
	for _, program := range file.Progs {
		if program.Type == elf.PT_LOAD && program.Filesz > 0 {
			return nil
		}
	}
	return errors.New("ELF has no loadable segment")
}

// This bounded format guard catches mislabeled input, not hostile attestation.
// The generator performs full media validation; canonical BBB also has a pin.
func validateMatroskaHeader(path string) error {
	file, err := os.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()
	prefix, err := io.ReadAll(io.LimitReader(file, 4096))
	if err != nil {
		return err
	}
	docType := append([]byte{0x42, 0x82, 0x88}, []byte("matroska")...)
	if len(prefix) < 4 || !bytes.Equal(prefix[:4], []byte{0x1a, 0x45, 0xdf, 0xa3}) || !bytes.Contains(prefix, docType) {
		return errors.New("input has no Matroska EBML header and document type")
	}
	return nil
}

func readFixtureManifest(path, input string) (fixtureInfo, error) {
	var info fixtureInfo
	data, err := os.ReadFile(path)
	if err != nil {
		return info, err
	}
	if err = json.Unmarshal(data, &info); err != nil {
		return info, err
	}
	if !validCorpus(info.Kind) {
		return info, errors.New("unknown canonical fixture kind")
	}
	stat, err := os.Stat(input)
	if err != nil {
		return info, err
	}
	hash, err := digest(input)
	if err != nil {
		return info, err
	}
	if info.Bytes != stat.Size() || info.Input.SHA256 != hash {
		return info, errors.New("canonical fixture receipt does not match input SHA256 and actual bytes")
	}
	switch info.Kind {
	case "elf":
		if err = validateELF(input); err != nil {
			return info, fmt.Errorf("canonical ELF kind validation: %w", err)
		}
	case "mkv":
		if err = validateMatroskaHeader(input); err != nil {
			return info, err
		}
		if info.Source != nil {
			source := info.Source
			if hash != bbbOutputSHA || source.URL != bbbURL || source.ArchiveSHA256 != bbbArchiveSHA || source.Member != bbbMember || source.MemberSHA256 != bbbMemberSHA || source.Attribution != bbbAttribution || source.LicenseURL != "https://creativecommons.org/licenses/by/3.0/" || source.Changes == "" || info.Media == nil || info.Media.Codec != "h264" || info.Media.AudioCodec != "aac" {
				return info, errors.New("canonical public Matroska receipt does not match pinned media and attribution")
			}
		} else {
			if info.Media == nil || info.Media.Codec != "ffv1" || info.Media.Frames < 1 || info.RawSHA256 == "" {
				return info, errors.New("canonical Matroska receipt has no validated media identity")
			}
			if info.Media.Width != frameWidth || info.Media.Height != frameHeight || info.Media.PixelFormat != "gray" {
				return info, errors.New("canonical noise media metadata differs from recipe")
			}

		}
	}
	return info, nil
}
