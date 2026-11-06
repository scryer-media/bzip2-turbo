package main

import (
	"fmt"
	"strings"
	"testing"
)

func reportText(rss, user, system string) []byte {
	return []byte(fmt.Sprintf("BZIP2_RSS_KIB=%s\nBZIP2_USER_SECONDS=%s\nBZIP2_SYSTEM_SECONDS=%s\n", rss, user, system))
}
func TestGNUTimeReportUsesOwnChildFieldsAndRejectsMalformedData(t *testing.T) {
	usage, err := parseGNUTimeReport(reportText("5120", "0.12", "0.00"))
	if err != nil || usage.RSSBytes == nil || *usage.RSSBytes != 5242880 || usage.UserSeconds != 0.12 || usage.SystemSeconds != 0 {
		t.Fatal("wrong GNU time units/scope", usage, err)
	}
	if _, err = parseGNUTimeReport(reportText("5120", "0.00", "0.00")); err != nil {
		t.Fatal("zero short-command CPU must be valid", err)
	}
	bad := [][]byte{nil, []byte(""), []byte("BZIP2_RSS_KIB=12\n"), reportText("0", "0", "0"), reportText("-1", "0", "0"), reportText("12KiB", "0", "0"), reportText("18446744073709551615", "0", "0"), reportText("5120", "NaN", "0"), reportText("5120", "+Inf", "0"), reportText("5120", "-0.01", "0"), reportText("5120", "0", "1e9999"), []byte(strings.Repeat("x", 257)), []byte("BZIP2_RSS_KIB=12\nBZIP2_RSS_KIB=12\nBZIP2_SYSTEM_SECONDS=0\n"), append(reportText("12", "0", "0"), []byte("unexpected\n")...)}
	for i, data := range bad {
		if _, err = parseGNUTimeReport(data); err == nil {
			t.Fatalf("accepted malformed report %d", i)
		}
	}
}
