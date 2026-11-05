//go:build darwin || linux

package main

import (
	"os"
	"runtime"
	"syscall"
)

const rssSupported = true
const rssDescription = "child process peak RSS from getrusage; normalized to bytes (Darwin bytes, Linux KiB); descendants excluded"

func peakRSS(state *os.ProcessState) *uint64 {
	usage, ok := state.SysUsage().(*syscall.Rusage)
	if !ok || usage.Maxrss <= 0 {
		return nil
	}
	value := uint64(usage.Maxrss)
	if runtime.GOOS == "linux" {
		value *= 1024
	}
	return &value
}
