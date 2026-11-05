// Repository maintenance commands use only the Go standard library.
package main

import (
	"archive/tar"
	"bytes"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime"
	"sort"
	"strings"
)

func output(args ...string) ([]byte, error) { return exec.Command("git", args...).Output() }
func run(name string, args ...string) error {
	c := exec.Command(name, args...)
	c.Stdin = os.Stdin
	c.Stdout = os.Stdout
	c.Stderr = os.Stderr
	return c.Run()
}

var homePattern = regexp.MustCompile(`/(?:U` + `sers|ho` + `me)/[^/\\\s]+|[A-Za-z]:[\\/][Uu]sers[\\/][^\\/\s]+`)
var emailPattern = regexp.MustCompile("[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@[A-Za-z0-9.-]+\\.[A-Za-z]{2,}")

func fallback(keys ...string) string {
	for _, k := range keys {
		if v := os.Getenv(k); v != "" {
			return v
		}
	}
	return ""
}
func reasons(data []byte, legal bool) ([]string, error) {
	found := map[string]bool{}
	add := func(s string) { found[s] = true }
	text := string(data)
	fold := strings.ToLower(text)
	if bytes.ContainsRune(data, 0) {
		add("generated binary content")
	}
	if os.Getenv("GITHUB_ACTIONS") != "true" {
		for _, item := range []struct{ value, label string }{{fallback("USER", "USERNAME"), "local account name"}, {fallback("HOME", "USERPROFILE"), "local home directory"}} {
			if item.value != "" && strings.Contains(fold, strings.ToLower(item.value)) {
				add(item.label)
			}
		}
	}
	if homePattern.MatchString(text) {
		add("home path")
	}
	for _, email := range emailPattern.FindAllString(text, -1) {
		if !legal && !strings.HasSuffix(strings.ToLower(email), "@users.noreply.github.com") {
			add("contact address")
		}
	}
	config := os.Getenv("XDG_CONFIG_HOME")
	if config == "" {
		h, e := os.UserHomeDir()
		if e != nil {
			return nil, e
		}
		config = filepath.Join(h, ".config")
	}
	blocked := fallback("GIT_PRIVACY_BLOCKLIST", "GIT_PRIVACY_DENYLIST")
	if blocked == "" {
		blocked = filepath.Join(config, "git-privacy", "denylist")
	}
	raw, e := os.ReadFile(blocked)
	if e != nil && !errors.Is(e, os.ErrNotExist) {
		return nil, errors.New("cannot read machine-local privacy configuration")
	}
	for _, line := range strings.Split(string(raw), "\n") {
		term := strings.TrimSpace(strings.SplitN(line, "#", 2)[0])
		if term != "" && strings.Contains(fold, strings.ToLower(term)) {
			add("machine-local blocked term")
		}
	}
	result := []string{}
	for label := range found {
		result = append(result, label)
	}
	sort.Strings(result)
	return result, nil
}
func privacy(args []string) error {
	staged := false
	message := ""
	for i := 0; i < len(args); i++ {
		switch args[i] {
		case "--staged":
			staged = true
		case "--message":
			i++
			if i == len(args) {
				return errors.New("message file required")
			}
			message = args[i]
		default:
			return errors.New("unknown privacy argument")
		}
	}
	if staged && message != "" {
		return errors.New("choose staged files or message")
	}
	failed := false
	check := func(data []byte, legal bool) error {
		r, e := reasons(data, legal)
		if e != nil {
			return e
		}
		if len(r) > 0 {
			fmt.Println("privacy check blocked an input: " + strings.Join(r, ", "))
			failed = true
		}
		return nil
	}
	if message != "" {
		b, e := os.ReadFile(message)
		if e != nil {
			return errors.New("cannot read commit message")
		}
		if e = check(b, false); e != nil {
			return e
		}
	} else {
		names, e := output("ls-files", "--cached", "-z")
		if e != nil {
			return errors.New("cannot enumerate Git index")
		}
		for _, name := range bytes.Split(names, []byte{0}) {
			if len(name) == 0 {
				continue
			}
			p := string(name)
			var b []byte
			if staged {
				b, e = output("show", ":"+p)
			} else {
				b, e = os.ReadFile(p)
			}
			if e != nil {
				return errors.New("cannot read tracked input")
			}
			if e = check(append(append([]byte{}, name...), append([]byte{10}, b...)...), p == "COPYING" || p == "LICENSE-7ZIP"); e != nil {
				return e
			}
		}
	}
	if failed {
		return errors.New("privacy check failed")
	}
	return nil
}
func aggregateIndex(dst io.Writer) error {
	names, e := output("ls-files", "--cached", "-z")
	if e != nil {
		return errors.New("cannot enumerate Git index")
	}
	seen := map[string]bool{}
	for _, name := range bytes.Split(names, []byte{0}) {
		if len(name) == 0 || seen[string(name)] {
			continue
		}
		seen[string(name)] = true
		if _, e = fmt.Fprintf(dst, "\n%s\n", name); e != nil {
			return e
		}
		c := exec.Command("git", "show", ":"+string(name))
		c.Stdout = dst
		if e = c.Run(); e != nil {
			return errors.New("cannot read indexed blob")
		}
	}
	return nil
}
func secrets(args []string) error {
	docker := len(args) == 1 && args[0] == "--docker"
	if len(args) > 0 && !docker {
		return errors.New("unknown scanner argument")
	}
	raw, e := output("rev-parse", "--show-toplevel")
	if e != nil {
		return e
	}
	root := strings.TrimSpace(string(raw))
	f, e := os.CreateTemp("", "index-secrets-*")
	if e != nil {
		return e
	}
	defer os.Remove(f.Name())
	defer f.Close()
	if e = aggregateIndex(f); e != nil {
		return e
	}
	if _, e = f.Seek(0, 0); e != nil {
		return e
	}
	name := "gitleaks"
	command := []string{"stdin", "--redact", "--config=" + filepath.Join(root, ".gitleaks.toml")}
	if docker {
		name = "docker"
		command = []string{"run", "--rm", "-i", "-v", root + ":/repo:ro", "zricethezav/gitleaks:v8.30.1", "stdin", "--redact", "--config=/repo/.gitleaks.toml"}
	}
	c := exec.Command(name, command...)
	c.Stdin = f
	c.Stdout = os.Stdout
	c.Stderr = os.Stderr
	return c.Run()
}

const revision = "9128b80e3a471108678e2b3ec3c985861c8d7b0f"

func extractArchive(r io.Reader, root string) error {
	tr := tar.NewReader(r)
	for {
		h, e := tr.Next()
		if e == io.EOF {
			return nil
		}
		if e != nil {
			return e
		}
		name := filepath.FromSlash(h.Name)
		if filepath.IsAbs(name) || name == ".." || strings.HasPrefix(name, ".."+string(os.PathSeparator)) {
			return errors.New("unsafe archive path")
		}
		p := filepath.Join(root, name)
		switch h.Typeflag {
		case tar.TypeXGlobalHeader:
			// git archive stores its source commit in a global PAX metadata header.
			continue
		case tar.TypeDir:
			if e = os.MkdirAll(p, 0755); e != nil {
				return e
			}
		case tar.TypeReg, tar.TypeRegA:
			if e = os.MkdirAll(filepath.Dir(p), 0755); e != nil {
				return e
			}
			f, e := os.OpenFile(p, os.O_CREATE|os.O_EXCL|os.O_WRONLY, os.FileMode(h.Mode)&0777)
			if e != nil {
				return e
			}
			_, e = io.Copy(f, tr)
			closeErr := f.Close()
			if e != nil {
				return e
			}
			if closeErr != nil {
				return closeErr
			}
		default:
			return errors.New("unsupported reference archive entry")
		}
	}
}
func oracle() error {
	root := os.Getenv("BZIP2_ORACLE_BUILD_DIR")
	if root == "" {
		root = filepath.Join("target", "bzip2-oracle")
	}
	root, e := filepath.Abs(root)
	if e != nil {
		return e
	}
	if e = os.MkdirAll(root, 0755); e != nil {
		return e
	}
	source := filepath.Join(root, "source")
	marker := ".bzip2-oracle-revision"
	if info, e := os.Lstat(source); e == nil {
		if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			return errors.New("refusing unverified reference source")
		}
		b, e := os.ReadFile(filepath.Join(source, marker))
		if e != nil || strings.TrimSpace(string(b)) != revision {
			return errors.New("refusing unverified or mismatched reference source")
		}
	} else if !errors.Is(e, os.ErrNotExist) {
		return e
	} else {
		staging, e := os.MkdirTemp(root, "source.staging.")
		if e != nil {
			return e
		}
		defer os.RemoveAll(staging)
		gitdir := filepath.Join(root, "git")
		if e = run("git", "init", "--bare", "--quiet", gitdir); e != nil {
			return e
		}
		if e = run("git", "-C", gitdir, "fetch", "--quiet", "--depth", "1", "https://github.com/ip7z/7zip.git", revision); e != nil {
			return e
		}
		b, e := output("-C", gitdir, "rev-parse", "FETCH_HEAD^{commit}")
		if e != nil || strings.TrimSpace(string(b)) != revision {
			return errors.New("reference revision verification failed")
		}
		c := exec.Command("git", "-C", gitdir, "archive", revision)
		pipe, e := c.StdoutPipe()
		if e != nil {
			return e
		}
		if e = c.Start(); e != nil {
			return e
		}
		e = extractArchive(pipe, staging)
		if e != nil {
			_ = c.Process.Kill()
		}
		waitErr := c.Wait()
		if e != nil {
			return e
		}
		if waitErr != nil {
			return waitErr
		}
		if e = os.WriteFile(filepath.Join(staging, marker), []byte(revision+"\n"), 0644); e != nil {
			return e
		}
		if _, e = os.Lstat(source); !errors.Is(e, os.ErrNotExist) {
			return errors.New("reference source appeared during preparation")
		}
		if e = os.Rename(staging, source); e != nil {
			return e
		}
	}
	bundle := filepath.Join(source, "CPP", "7zip", "Bundles", "Alone2")
	makefile := "makefile.gcc"
	binary := filepath.Join(bundle, "_o", "7zz")
	if runtime.GOOS == "darwin" {
		arch := map[string]string{"arm64": "arm64", "amd64": "x64"}[runtime.GOARCH]
		if arch == "" {
			return errors.New("unsupported macOS reference architecture")
		}
		makefile = "../../cmpl_mac_" + arch + ".mak"
		binary = filepath.Join(bundle, "b", "m_"+arch, "7zz")
	}
	if e = run("make", "-C", bundle, "-f", makefile, "-j2"); e != nil {
		return e
	}
	info, e := os.Stat(binary)
	if e != nil || info.Mode()&0111 == 0 {
		return errors.New("reference executable unavailable")
	}
	if env := os.Getenv("GITHUB_ENV"); env != "" {
		f, e := os.OpenFile(env, os.O_APPEND|os.O_WRONLY|os.O_CREATE, 0600)
		if e != nil {
			return e
		}
		_, e = fmt.Fprintln(f, "BZIP2_TURBO_7ZZ="+binary)
		_ = f.Close()
		if e != nil {
			return e
		}
	}
	fmt.Println("BZip2 reference: " + binary)
	return nil
}
func dispatch(args []string) error {
	if len(args) == 0 {
		return errors.New("command required: privacy, scan-secrets, oracle")
	}
	switch args[0] {
	case "privacy":
		return privacy(args[1:])
	case "scan-secrets":
		return secrets(args[1:])
	case "oracle":
		return oracle()
	}
	return errors.New("unknown maintenance command")
}
func main() {
	if e := dispatch(os.Args[1:]); e != nil {
		fmt.Fprintln(os.Stderr, "maintenance command failed:", e)
		os.Exit(1)
	}
}
