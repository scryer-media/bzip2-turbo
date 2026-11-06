//go:build linux

package main

import (
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"testing"
	"time"
)

var resident []byte

func touchResident(size int) []byte {
	b := make([]byte, size)
	for i := 0; i < len(b); i += 4096 {
		b[i] = byte((i/4096)%251 + 1)
	}
	return b
}
func rssHelper(t *testing.T, mode string, more ...string) []string {
	t.Helper()
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	return append([]string{exe, "-test.run=^TestLinuxRSSHelper$", "--", mode}, more...)
}
func TestLinuxRSSHelper(t *testing.T) {
	index := -1
	for i, arg := range os.Args {
		if arg == "--" {
			index = i
			break
		}
	}
	if index < 0 {
		return
	}
	args := os.Args[index+1:]
	switch args[0] {
	case "resident":
		n, err := strconv.Atoi(args[1])
		if err != nil {
			os.Exit(2)
		}
		resident = touchResident(n)
		time.Sleep(40 * time.Millisecond)
		fmt.Println(resident[len(resident)-4096])
		runtime.KeepAlive(resident)
		os.Exit(0)
	case "sleep":
		time.Sleep(30 * time.Second)
		os.Exit(0)
	case "descendant":
		exe, err := os.Executable()
		if err != nil {
			os.Exit(2)
		}
		child := exec.Command(exe, "-test.run=^TestLinuxRSSHelper$", "--", "sleep")
		if err = child.Start(); err != nil {
			os.Exit(3)
		}
		if err = os.WriteFile(args[1], []byte(strconv.Itoa(child.Process.Pid)), 0600); err != nil {
			_ = child.Process.Kill()
			os.Exit(4)
		}
		_ = child.Wait()
		os.Exit(0)
	}
}
func TestLinuxRSSDoesNotInheritInflatedGoParent(t *testing.T) {
	input := filepath.Join(t.TempDir(), "input")
	if err := os.WriteFile(input, nil, 0600); err != nil {
		t.Fatal(err)
	}
	defer func() { resident = nil }()
	var peaks []uint64
	for _, mib := range []int{64, 128} {
		resident = touchResident(mib << 20)
		args := rssHelper(t, "resident", strconv.Itoa(4<<20))
		direct := exec.Command(args[0], args[1:]...)
		if err := direct.Run(); err != nil {
			t.Fatal(err)
		}
		legacy := uint64(direct.ProcessState.SysUsage().(*syscall.Rusage).Maxrss) * 1024
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		value, err := process(ctx, args, input, os.DevNull)
		cancel()
		runtime.KeepAlive(resident)
		if err != nil {
			t.Fatal(err)
		}
		if value.RSSBytes == nil || *value.RSSBytes >= 32<<20 {
			t.Fatal("RSS follows inflated parent", mib, value.RSSBytes)
		}
		peaks = append(peaks, *value.RSSBytes)
		t.Logf("parent=%dMiB legacy_wait4=%dB corrected_child=%dB", mib, legacy, *value.RSSBytes)
	}
	difference := int64(peaks[0]) - int64(peaks[1])
	if difference < 0 {
		difference = -difference
	}
	if difference > 16<<20 {
		t.Fatal("child RSS changed with parent heap", peaks)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	value, err := process(ctx, rssHelper(t, "resident", strconv.Itoa(80<<20)), input, os.DevNull)
	if err != nil || value.RSSBytes == nil || *value.RSSBytes < 72<<20 {
		t.Fatal("collector suppressed a genuinely resident child", value, err)
	}
	t.Logf("deliberate80MiBchild=%dB", *value.RSSBytes)
}
func TestLinuxReporterPrivateLifecycleAndMissingReports(t *testing.T) {
	args := rssHelper(t, "resident", strconv.Itoa(4<<20))
	if _, _, _, err := linuxMeasuredCommand(context.Background(), args, filepath.Join(t.TempDir(), "missing-time")); err == nil {
		t.Fatal("missing reporter accepted")
	}
	cmd, collect, cleanup, err := linuxMeasuredCommand(context.Background(), args, "/usr/bin/time")
	if err != nil {
		t.Fatal(err)
	}
	defer cleanup()
	report := ""
	for i, arg := range cmd.Args {
		if arg == "-o" {
			report = cmd.Args[i+1]
		}
	}
	if report == "" {
		t.Fatal("no private output report")
	}
	file, err := os.Stat(report)
	if err != nil || file.Mode().Perm() != 0600 {
		t.Fatal("report permissions")
	}
	directory, err := os.Stat(filepath.Dir(report))
	if err != nil || directory.Mode().Perm() != 0700 {
		t.Fatal("report directory permissions")
	}
	if _, err = collect(nil); err == nil {
		t.Fatal("empty report accepted")
	}
	if err = os.WriteFile(report, reportText("5120", "0", "0"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = collect(nil); err != nil {
		t.Fatal(err)
	}
	if err = os.Remove(report); err != nil {
		t.Fatal(err)
	}
	if _, err = collect(nil); err == nil {
		t.Fatal("missing report accepted")
	}
	if !errors.Is(cmd.Cancel(), os.ErrProcessDone) {
		t.Fatal("unstarted command cancellation was not refused")
	}
	cleanup()
	if _, err = os.Stat(filepath.Dir(report)); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("private report directory retained")
	}
}
func TestLinuxProcessFailureAndTimeoutKillOwnedGroup(t *testing.T) {
	input := filepath.Join(t.TempDir(), "input")
	if err := os.WriteFile(input, nil, 0600); err != nil {
		t.Fatal(err)
	}
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	_, err = process(context.Background(), []string{exe, "-test.run=^TestProcessHelper$", "--", "fail"}, input, os.DevNull)
	var exit *exec.ExitError
	if !errors.As(err, &exit) || exit.ExitCode() != 7 {
		t.Fatal("child exit status was not preserved", err)
	}
	pidfile := filepath.Join(t.TempDir(), "pid")
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	done := make(chan error, 1)
	args := rssHelper(t, "descendant", pidfile)
	go func() { _, err := process(ctx, args, input, os.DevNull); done <- err }()
	var pid int
	deadline := time.Now().Add(10 * time.Second)
	for time.Now().Before(deadline) {
		if data, e := os.ReadFile(pidfile); e == nil {
			pid, _ = strconv.Atoi(string(data))
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	if pid <= 0 {
		cancel()
		t.Fatal("descendant did not start")
	}
	select {
	case err = <-done:
		if !errors.Is(err, context.DeadlineExceeded) {
			t.Fatal("timeout/cancel status lost", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("process group deadline did not return")
	}
	deadline = time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		data, e := os.ReadFile(fmt.Sprintf("/proc/%d/stat", pid))
		if errors.Is(e, os.ErrNotExist) {
			return
		}
		if e == nil {
			tail := string(data[strings.LastIndex(string(data), ")")+1:])
			if strings.HasPrefix(strings.TrimSpace(tail), "Z") {
				return
			}
		}
		time.Sleep(10 * time.Millisecond)
	}
	// Kill only the recorded helper if its command line still proves ownership.
	if commandLine, e := os.ReadFile(fmt.Sprintf("/proc/%d/cmdline", pid)); e == nil && strings.Contains(string(commandLine), "-test.run=^TestLinuxRSSHelper$") {
		_ = syscall.Kill(pid, syscall.SIGKILL)
	}
	t.Fatal("task-owned descendant survived process-group cancellation")
}
