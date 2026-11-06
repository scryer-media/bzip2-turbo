//go:build !darwin && !linux

package main

import (
	"context"
	"os"
	"os/exec"
)

const rssMethod = "unsupported"

const rssSupported = false
const rssDescription = "unsupported on this platform; peak_rss_bytes omitted"
const processTimingDescription = "monotonic wall seconds including process startup and I/O; CPU seconds from ProcessState; host idleness not assessed"

func measuredCommand(ctx context.Context, args []string) (*exec.Cmd, func(*os.ProcessState) (processUsage, error), func(), error) {
	command := exec.CommandContext(ctx, args[0], args[1:]...)
	collect := func(state *os.ProcessState) (processUsage, error) {
		return processUsage{state.UserTime().Seconds(), state.SystemTime().Seconds(), nil}, nil
	}
	return command, collect, func() {}, nil
}
