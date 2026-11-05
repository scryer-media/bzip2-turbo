package main

import (
	"bytes"
	"encoding/json"
	"testing"
)

type chunkedJSONWriter struct {
	bytes.Buffer
	chunk int
}

func (w *chunkedJSONWriter) Write(p []byte) (int, error) {
	total := len(p)
	for len(p) > 0 {
		n := min(w.chunk, len(p))
		w.Buffer.Write(p[:n])
		p = p[n:]
	}
	return total, nil
}
func TestJSONTextIsValidExactSizedAndChunkIndependent(t *testing.T) {
	for _, size := range []int64{256, 257, 1023, 1024, 65535, 65536, 65537, 1 << 20} {
		var first bytes.Buffer
		second := chunkedJSONWriter{chunk: 17}
		if err := generateJSON(&first, size); err != nil {
			t.Fatal(err)
		}
		if err := generateJSON(&second, size); err != nil {
			t.Fatal(err)
		}
		if int64(first.Len()) != size || !json.Valid(first.Bytes()) || !bytes.Equal(first.Bytes(), second.Bytes()) {
			t.Fatalf("invalid JSON/extent/chunk dependency at %d", size)
		}
		var payload struct {
			Records []jsonRecord `json:"records"`
			Padding string       `json:"padding"`
		}
		if err := json.Unmarshal(first.Bytes(), &payload); err != nil {
			t.Fatal(err)
		}
		if len(payload.Padding) > 1200 {
			t.Fatal("large padding", len(payload.Padding))
		}
		if size >= 65535 {
			seen := map[string]bool{}
			for i, record := range payload.Records {
				if record.ID != uint64(i) || seen[record.Description] || len(record.Tags) == 0 || record.Attributes.Label == "" {
					t.Fatal("repeated/malformed record", i)
				}
				seen[record.Description] = true
			}
			if len(seen) < 50 {
				t.Fatal("too few varied records")
			}
		}
	}
	for _, size := range []int64{0, 1, 2, 255} {
		if err := generateJSON(&bytes.Buffer{}, size); err == nil {
			t.Fatal("accepted too-small JSON", size)
		}
	}
}
