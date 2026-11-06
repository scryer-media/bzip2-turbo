package main

import (
	"errors"
	"math"
	"strconv"
	"strings"
)

const gnuTimeFormat = "BZIP2_RSS_KIB=%M\nBZIP2_USER_SECONDS=%U\nBZIP2_SYSTEM_SECONDS=%S"

func parseGNUTimeReport(data []byte) (processUsage, error) {
	var usage processUsage
	if len(data) > 256 {
		return usage, errors.New("GNU time report exceeds expected extent")
	}
	lines := strings.Split(strings.TrimSpace(string(data)), "\n")
	if len(lines) != 3 {
		return usage, errors.New("GNU time report is missing or malformed")
	}
	fields := []string{"BZIP2_RSS_KIB=", "BZIP2_USER_SECONDS=", "BZIP2_SYSTEM_SECONDS="}
	values := make([]string, 3)
	for i, line := range lines {
		if !strings.HasPrefix(line, fields[i]) {
			return usage, errors.New("GNU time report has missing, duplicate or unexpected fields")
		}
		values[i] = strings.TrimPrefix(line, fields[i])
		if values[i] == "" {
			return usage, errors.New("GNU time report has an empty field")
		}
	}
	for _, c := range values[0] {
		if c < 48 || c > 57 {
			return usage, errors.New("GNU time RSS is not an unsigned decimal integer")
		}
	}
	kib, err := strconv.ParseUint(values[0], 10, 64)
	if err != nil || kib == 0 || kib > math.MaxUint64/1024 {
		return usage, errors.New("GNU time RSS is zero, invalid or overflows bytes")
	}
	rss := kib * 1024
	usage.RSSBytes = &rss
	for i, target := range []*float64{&usage.UserSeconds, &usage.SystemSeconds} {
		value, err := strconv.ParseFloat(values[i+1], 64)
		if err != nil || math.IsNaN(value) || math.IsInf(value, 0) || value < 0 || strings.HasPrefix(values[i+1], "-") {
			return processUsage{}, errors.New("GNU time CPU seconds are invalid")
		}
		*target = value
	}
	return usage, nil
}
