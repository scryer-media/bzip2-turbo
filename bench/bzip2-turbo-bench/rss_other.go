//go:build !darwin && !linux

package main

import "os"

const rssSupported = false
const rssDescription = "unsupported on this platform; peak_rss_bytes omitted"

func peakRSS(*os.ProcessState) *uint64 { return nil }
