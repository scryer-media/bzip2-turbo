//go:build linux

package main

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"syscall"
	"time"
)

const rssMethod = "gnu-time-child-v1"

const rssSupported = true
const rssDescription = "codec command peak RSS from GNU /usr/bin/time own child report; Linux KiB normalized to bytes; Go runner pre-exec high-water excluded"
const processTimingDescription = "monotonic wall seconds include GNU time wrapper startup, command startup and I/O; command CPU seconds from GNU time own child report (hundredth-second precision); host idleness not assessed"

func measuredCommand(ctx context.Context, args []string) (*exec.Cmd, func(*os.ProcessState) (processUsage, error), func(), error) {
	return linuxMeasuredCommand(ctx, args, "/usr/bin/time")
}
func linuxMeasuredCommand(ctx context.Context, args []string, reporter string) (*exec.Cmd, func(*os.ProcessState) (processUsage, error), func(), error) {
	if len(args) == 0 {
		return nil, nil, nil, errors.New("empty process command")
	}
	if _, err := exec.LookPath(args[0]); err != nil {
		return nil, nil, nil, err
	}
	if _, err := exec.LookPath(reporter); err != nil {
		return nil, nil, nil, fmt.Errorf("GNU /usr/bin/time is required for valid Linux RSS: %w", err)
	}
	directory, err := os.MkdirTemp("", "bzip2-rss-*")
	if err != nil {
		return nil, nil, nil, err
	}
	cleanup := func() { _ = os.RemoveAll(directory) }
	report := filepath.Join(directory, "usage")
	file, err := os.OpenFile(report, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		cleanup()
		return nil, nil, nil, err
	}
	if err = file.Close(); err != nil {
		cleanup()
		return nil, nil, nil, err
	}
	command := exec.CommandContext(ctx, reporter, append([]string{"-q", "-f", gnuTimeFormat, "-o", report, "--"}, args...)...)
	command.Env = fixedEnv(map[string]string{"LC_ALL": "C"})
	// The reporter and codec inherit a new task-owned group, never group zero.
	command.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	command.Cancel = func() error {
		if command.Process == nil || command.Process.Pid <= 0 {
			return os.ErrProcessDone
		}
		err := syscall.Kill(-command.Process.Pid, syscall.SIGKILL)
		if errors.Is(err, syscall.ESRCH) {
			return os.ErrProcessDone
		}
		return err
	}
	command.WaitDelay = 2 * time.Second
	collect := func(*os.ProcessState) (processUsage, error) {
		file, err := os.Open(report)
		if err != nil {
			return processUsage{}, err
		}
		defer file.Close()
		data, err := io.ReadAll(io.LimitReader(file, 257))
		if err != nil {
			return processUsage{}, err
		}
		return parseGNUTimeReport(data)
	}
	return command, collect, cleanup, nil
}
