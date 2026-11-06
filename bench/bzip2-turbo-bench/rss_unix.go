//go:build darwin

package main

import (
	"context"
	"os"
	"os/exec"
	"syscall"
)

const rssMethod = "darwin-native-rusage-v1"

const rssSupported = true
const rssDescription = "child process peak RSS from getrusage on Darwin; native byte units"
const processTimingDescription = "monotonic wall seconds including process startup and I/O; CPU seconds from ProcessState; host idleness not assessed"

func peakRSS(state *os.ProcessState) *uint64 {
	usage, ok := state.SysUsage().(*syscall.Rusage)
	if !ok || usage.Maxrss <= 0 {
		return nil
	}
	value := uint64(usage.Maxrss)
	return &value
}
func measuredCommand(ctx context.Context, args []string) (*exec.Cmd, func(*os.ProcessState) (processUsage, error), func(), error) {
	command := exec.CommandContext(ctx, args[0], args[1:]...)
	collect := func(state *os.ProcessState) (processUsage, error) {
		return processUsage{state.UserTime().Seconds(), state.SystemTime().Seconds(), peakRSS(state)}, nil
	}
	return command, collect, func() {}, nil
}
