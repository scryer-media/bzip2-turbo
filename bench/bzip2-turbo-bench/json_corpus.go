package main

import (
	"bufio"
	"encoding/json"
	"errors"
	"flag"
	"io"
	"strings"
)

// xorshift64* fixes the generated stream independently of writer chunk sizes.
type jsonRandom uint64

func (r *jsonRandom) next() uint64 {
	x := uint64(*r)
	x ^= x >> 12
	x ^= x << 25
	x ^= x >> 27
	*r = jsonRandom(x)
	return x * 2685821657736338717
}

var jsonVocabulary = strings.Fields("amber anchor archive autumn basket beacon birch bridge canyon cedar circle cloud cobalt copper coral delta ember field forest garden glass granite harbor horizon island ivory jasmine journey lantern lattice lemon linen maple meadow midnight mineral mosaic mountain ocean olive orchard paper pebble pine planet prism quartz rain raven river saffron silver slate solar sparrow spring stone stream summit sunset timber trail valley velvet violet water willow wind winter woven acorn almond atlas bamboo barley basil berry blossom branch bronze canvas cinnamon citrus clay clover crystal dawn dune echo fern flax frost ginger grove hazel honey indigo juniper lagoon leaf lily lunar marine mint mist moss nectar oak opal orbit oyster pearl pepper petal plum pollen reef ribbon ridge root sage sand seed shadow shell shore silk snow spruce star steel storm tea terrace thyme tide topaz walnut wave wheat wildwood")

func (r *jsonRandom) words(n int) string {
	var text strings.Builder
	for i := 0; i < n; i++ {
		if i != 0 {
			text.WriteByte(' ')
		}
		text.WriteString(jsonVocabulary[r.next()%uint64(len(jsonVocabulary))])
	}
	return text.String()
}

type jsonRecord struct {
	ID          uint64   `json:"id"`
	Title       string   `json:"title"`
	Description string   `json:"description"`
	Category    string   `json:"category"`
	Active      bool     `json:"active"`
	Tags        []string `json:"tags"`
	Metrics     struct {
		Count  uint64 `json:"count"`
		Score  uint64 `json:"score"`
		Weight uint64 `json:"weight"`
	} `json:"metrics"`
	Attributes struct {
		Region   string `json:"region"`
		Material string `json:"material"`
		Label    string `json:"label"`
	} `json:"attributes"`
}

// generateJSON retains one record and a fixed output buffer, never the payload.
// The final short string consumes less space than the next complete record.
func generateJSON(output io.Writer, size int64) error {
	if size < 256 {
		return errors.New("JSON text corpus requires at least 256 bytes")
	}
	writer := bufio.NewWriterSize(output, 65536)
	prefix := `{"records":[`
	tail := `],"padding":"`
	suffix := `"}`
	if _, err := io.WriteString(writer, prefix); err != nil {
		return err
	}
	remaining := size - int64(len(prefix)+len(tail)+len(suffix))
	random := jsonRandom(0x6a09e667f3bcc909)
	first := true
	for id := uint64(0); ; id++ {
		record := jsonRecord{ID: id, Title: random.words(3 + int(random.next()%7)), Description: random.words(18 + int(random.next()%45)), Category: random.words(1), Active: random.next()%2 == 0}
		for n := int(random.next()%5) + 1; n > 0; n-- {
			record.Tags = append(record.Tags, random.words(1))
		}
		record.Metrics.Count = random.next() % 1000000
		record.Metrics.Score = random.next() % 10000
		record.Metrics.Weight = random.next() % 100000
		record.Attributes.Region = random.words(2)
		record.Attributes.Material = random.words(1)
		record.Attributes.Label = random.words(3)
		encoded, err := json.Marshal(record)
		if err != nil {
			return err
		}
		needed := int64(len(encoded))
		if !first {
			needed++
		}
		if needed > remaining {
			break
		}
		if !first {
			if err = writer.WriteByte(','); err != nil {
				return err
			}
		}
		if _, err = writer.Write(encoded); err != nil {
			return err
		}
		remaining -= needed
		first = false
	}
	if _, err := io.WriteString(writer, tail); err != nil {
		return err
	}
	const alphabet = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
	for ; remaining > 0; remaining-- {
		if err := writer.WriteByte(alphabet[random.next()%uint64(len(alphabet))]); err != nil {
			return err
		}
	}
	if _, err := io.WriteString(writer, suffix); err != nil {
		return err
	}
	return writer.Flush()
}
func jsonCorpus(args []string) error {
	fs := flag.NewFlagSet("json-corpus", flag.ContinueOnError)
	output := fs.String("output", "", "JSON file outside checkout")
	size := fs.Int64("bytes", 32<<20, "exact payload length, at least 256 bytes")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if *output == "" || fs.NArg() != 0 {
		return errors.New("json-corpus --output PATH [--bytes N]")
	}
	root, err := legacyRoot()
	if err != nil {
		return err
	}
	if err = outside(*output, root); err != nil {
		return err
	}
	return generate(*output, "text", *size)
}
