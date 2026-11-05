package main

import (
	"archive/tar"
	"bytes"
	"crypto/rand"
	"encoding/hex"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func TestPrivacyRules(t *testing.T) {
	t.Setenv("GITHUB_ACTIONS", "true")
	t.Setenv("GIT_PRIVACY_BLOCKLIST", filepath.Join(t.TempDir(), "missing"))
	t.Setenv("GIT_PRIVACY_DENYLIST", "")
	cases := []struct {
		data  string
		legal bool
		want  string
	}{{"safe documentation", false, ""}, {"role@" + "example.invalid", false, "contact address"}, {"role@" + "users.noreply.github.com", false, ""}, {"role@" + "example.invalid", true, ""}, {"/" + "home/role/file", true, "home path"}, {"C:" + `\Users\role\file`, false, "home path"}, {"binary\x00", false, "generated binary content"}}
	for _, c := range cases {
		got, e := reasons([]byte(c.data), c.legal)
		if e != nil {
			t.Fatal(e)
		}
		if strings.Join(got, ",") != c.want {
			t.Fatalf("reason mismatch: %v != %s", got, c.want)
		}
	}
}
func TestMachineRulesAndFallback(t *testing.T) {
	p := filepath.Join(t.TempDir(), "terms")
	term := "generated-private-" + "token"
	if e := os.WriteFile(p, []byte("# comment\n"+term+" # note\n"), 0600); e != nil {
		t.Fatal(e)
	}
	t.Setenv("GITHUB_ACTIONS", "")
	t.Setenv("USER", "local-"+"role")
	t.Setenv("HOME", "/sandbox/"+"role")
	t.Setenv("GIT_PRIVACY_BLOCKLIST", "")
	t.Setenv("GIT_PRIVACY_DENYLIST", p)
	got, e := reasons([]byte(strings.ToUpper(term)+" local-role /sandbox/role"), false)
	if e != nil {
		t.Fatal(e)
	}
	if len(got) != 3 {
		t.Fatalf("expected three private categories, got %v", got)
	}
}
func fixtureRepo(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	prior, e := os.Getwd()
	if e != nil {
		t.Fatal(e)
	}
	if e = os.Chdir(dir); e != nil {
		t.Fatal(e)
	}
	t.Cleanup(func() { _ = os.Chdir(prior) })
	cmd := exec.Command("git", "init", "--quiet")
	if e = cmd.Run(); e != nil {
		t.Fatal(e)
	}
	return dir
}
func TestIndexIncludesUnbornAndUnchangedBlobs(t *testing.T) {
	fixtureRepo(t)
	name := "tracked.txt"
	if e := os.WriteFile(name, []byte("indexed-content"), 0600); e != nil {
		t.Fatal(e)
	}
	if e := exec.Command("git", "add", name).Run(); e != nil {
		t.Fatal(e)
	}
	if e := os.WriteFile(name, []byte("working-content"), 0600); e != nil {
		t.Fatal(e)
	}
	var got bytes.Buffer
	if e := aggregateIndex(&got); e != nil {
		t.Fatal(e)
	}
	if !strings.Contains(got.String(), "indexed-content") || strings.Contains(got.String(), "working-content") {
		t.Fatal("scanner did not use complete index")
	}
	// A tree object does not require a commit or a configured signing identity.
	if _, e := output("write-tree"); e != nil {
		t.Fatal(e)
	}
	got.Reset()
	if e := aggregateIndex(&got); e != nil {
		t.Fatal(e)
	}
	if !strings.Contains(got.String(), name) {
		t.Fatal("indexed filename absent")
	}
}
func TestStagedPrivacyScansFilenamesAndRedacts(t *testing.T) {
	fixtureRepo(t)
	t.Setenv("GITHUB_ACTIONS", "true")
	t.Setenv("GIT_PRIVACY_BLOCKLIST", filepath.Join(t.TempDir(), "missing"))
	name := "role@" + "example.invalid"
	if e := os.WriteFile(name, []byte("safe"), 0600); e != nil {
		t.Fatal(e)
	}
	if e := exec.Command("git", "add", name).Run(); e != nil {
		t.Fatal(e)
	}
	old := os.Stdout
	r, w, e := os.Pipe()
	if e != nil {
		t.Fatal(e)
	}
	os.Stdout = w
	e = privacy([]string{"--staged"})
	_ = w.Close()
	os.Stdout = old
	var out bytes.Buffer
	_, _ = out.ReadFrom(r)
	_ = r.Close()
	if e == nil || strings.Contains(out.String(), name) || !strings.Contains(out.String(), "contact address") {
		t.Fatal("privacy filename rejection or redaction failed")
	}
}
func TestArchiveRejectsTraversalAndLinks(t *testing.T) {
	for _, h := range []*tar.Header{{Name: "../escape", Mode: 0600, Typeflag: tar.TypeReg}, {Name: "link", Linkname: "target", Typeflag: tar.TypeSymlink}} {
		var b bytes.Buffer
		tw := tar.NewWriter(&b)
		if e := tw.WriteHeader(h); e != nil {
			t.Fatal(e)
		}
		_ = tw.Close()
		if e := extractArchive(&b, t.TempDir()); e == nil {
			t.Fatal("unsafe archive accepted")
		}
	}
}
func TestSecretScannerReadsFullUnbornIndex(t *testing.T) {
	if _, e := exec.LookPath("gitleaks"); e != nil {
		t.Skip("native gitleaks unavailable; CI separately scans with pinned Docker image")
	}
	fixtureRepo(t)
	if e := os.WriteFile(".gitleaks.toml", []byte("[extend]\nuseDefault = true\n"), 0600); e != nil {
		t.Fatal(e)
	}
	// Generate a detector fixture at runtime; no credential is stored in source.
	random := make([]byte, 18)
	if _, e := rand.Read(random); e != nil {
		t.Fatal(e)
	}
	secret := "github_token = ghp_" + hex.EncodeToString(random)
	if e := os.WriteFile("fixture", []byte(secret), 0600); e != nil {
		t.Fatal(e)
	}
	if e := exec.Command("git", "add", ".").Run(); e != nil {
		t.Fatal(e)
	}
	if e := os.WriteFile("fixture", []byte("safe working tree"), 0600); e != nil {
		t.Fatal(e)
	}
	if e := secrets(nil); e == nil {
		t.Fatal("indexed secret was not rejected")
	}
	if e := os.WriteFile("fixture", []byte("safe indexed tree"), 0600); e != nil {
		t.Fatal(e)
	}
	if e := exec.Command("git", "add", ".").Run(); e != nil {
		t.Fatal(e)
	}
	if e := secrets(nil); e != nil {
		t.Fatal(e)
	}
}
