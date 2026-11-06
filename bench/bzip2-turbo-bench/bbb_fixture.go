package main

import (
	"archive/zip"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

const bbbURL = "https://download.blender.org/peach/bigbuckbunny_movies/BigBuckBunny_320x180.mp4.zip"
const bbbArchiveSHA = "109e3ede8790bd633f374ca311d9cc61dce8d7f98f5b0797ca98199c9fbceedf"
const bbbMember = "BigBuckBunny_320x180.mp4"
const bbbMemberBytes int64 = 64657027
const bbbMemberSHA = "f78f39603e6774907f2faafabf26a667f4a6fc31769ec304a8a8f7c62d280508"
const bbbOutputSHA = "a88533696bc7ff1bb9847af06ccf37aafa7c781ea7d94eefbd7c8ff10d34525a"
const bbbAttribution = "Copyright 2008 Blender Foundation / www.bigbuckbunny.org; CC BY 3.0; remuxed excerpt"
const bbbArchiveLimit int64 = 128 << 20

type mediaSource struct {
	Name          string `json:"name"`
	URL           string `json:"url"`
	ArchiveSHA256 string `json:"archive_sha256"`
	Member        string `json:"member"`
	MemberSHA256  string `json:"member_sha256"`
	LicenseURL    string `json:"license_url"`
	Attribution   string `json:"attribution"`
	Changes       string `json:"changes"`
}

func bbbSource(options fixtureOptions, temporary string) (string, error) {
	if options.SourceFile != "" && options.Download {
		return "", errors.New("choose a local source file or explicit source download")
	}
	source := options.SourceFile
	if source == "" {
		if !options.Download {
			return "", errors.New("Big Buck Bunny requires --source-file with the pinned MP4/ZIP, or explicit --download-source")
		}
		source = filepath.Join(temporary, "source.zip")
		client := &http.Client{Timeout: 10 * time.Minute, CheckRedirect: func(*http.Request, []*http.Request) error {
			return errors.New("public source redirects are not permitted")
		}}
		response, e := client.Get(bbbURL)
		if e != nil {
			return "", e
		}
		defer response.Body.Close()
		if response.StatusCode != http.StatusOK {
			return "", fmt.Errorf("public source download returned HTTP %d", response.StatusCode)
		}
		if response.ContentLength > bbbArchiveLimit {
			return "", errors.New("public source archive exceeds size limit")
		}
		file, e := os.OpenFile(source, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
		if e != nil {
			return "", e
		}
		n, e := io.Copy(file, io.LimitReader(response.Body, bbbArchiveLimit+1))
		closeErr := file.Close()
		if e != nil {
			return "", e
		}
		if closeErr != nil {
			return "", closeErr
		}
		if n > bbbArchiveLimit {
			return "", errors.New("public source archive exceeds size limit")
		}
	}
	stat, e := os.Stat(source)
	if e != nil {
		return "", e
	}
	if !stat.Mode().IsRegular() || stat.Size() > bbbArchiveLimit {
		return "", errors.New("invalid public source file extent")
	}
	hash, e := digest(source)
	if e != nil {
		return "", e
	}
	if hash == bbbMemberSHA && stat.Size() == bbbMemberBytes {
		return source, nil
	}
	if hash != bbbArchiveSHA {
		return "", errors.New("public source SHA256 does not match pinned archive or MP4")
	}
	archive, e := zip.OpenReader(source)
	if e != nil {
		return "", e
	}
	defer archive.Close()
	var member *zip.File
	for _, entry := range archive.File {
		if entry.Name == bbbMember {
			if member != nil {
				return "", errors.New("duplicate public source member")
			}
			member = entry
		}
	}
	if member == nil || !member.Mode().IsRegular() || member.UncompressedSize64 != uint64(bbbMemberBytes) {
		return "", errors.New("public source archive has no valid exact allowlisted member")
	}
	input, e := member.Open()
	if e != nil {
		return "", e
	}
	defer input.Close()
	// Never join an archive path: only the exact allowlisted entry is copied.
	target := filepath.Join(temporary, "source.mp4")
	output, e := os.OpenFile(target, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if e != nil {
		return "", e
	}
	n, e := io.Copy(output, io.LimitReader(input, bbbMemberBytes+1))
	closeErr := output.Close()
	if e != nil {
		return "", e
	}
	if closeErr != nil {
		return "", closeErr
	}
	if n != bbbMemberBytes {
		return "", errors.New("public source member extent mismatch")
	}
	hash, e = digest(target)
	if e != nil {
		return "", e
	}
	if hash != bbbMemberSHA {
		return "", errors.New("public source member SHA256 mismatch")
	}
	return target, nil
}
func bbbParameters(source, output string) []string {
	return []string{"-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-ss", "30", "-i", source, "-t", "150", "-map", "0:v:0", "-map", "0:a:0", "-c", "copy", "-fflags", "+bitexact", "-map_metadata", "-1", "-metadata", "title=Big Buck Bunny benchmark excerpt", "-metadata", "comment=" + bbbAttribution, "-f", "matroska", output}
}
func generateBBB(path string, options fixtureOptions) (fixtureInfo, error) {
	info := fixtureInfo{Kind: "mkv", Generator: "FFmpeg Big Buck Bunny H.264/AAC Matroska excerpt"}
	temporary, e := os.MkdirTemp("", "codec-public-media-*")
	if e != nil {
		return info, e
	}
	defer os.RemoveAll(temporary)
	source, e := bbbSource(options, temporary)
	if e != nil {
		return info, e
	}
	info.ToolVersion, e = toolVersion("ffmpeg", "-version")
	if e != nil {
		return info, e
	}
	info.Parameters = bbbParameters("PINNED_SOURCE_MP4", "OUTPUT")
	info.Source = &mediaSource{Name: "Big Buck Bunny (2008)", URL: bbbURL, ArchiveSHA256: bbbArchiveSHA, Member: bbbMember, MemberSHA256: bbbMemberSHA, LicenseURL: "https://creativecommons.org/licenses/by/3.0/", Attribution: bbbAttribution, Changes: "150-second excerpt from 30 seconds; H.264/AAC stream-copy remux to Matroska; metadata replaced with attribution"}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Minute)
	defer cancel()
	command := exec.CommandContext(ctx, "ffmpeg", bbbParameters(source, path)...)
	var stderr bytes.Buffer
	command.Stderr = &stderr
	if e = command.Run(); e != nil {
		return info, fmt.Errorf("public video remux failed: %w: %s", e, stderr.String())
	}
	hash, e := digest(path)
	if e != nil {
		return info, e
	}
	if hash != bbbOutputSHA {
		return info, errors.New("public MKV differs from canonical SHA256; use canonical bytes or matching FFmpeg build")
	}
	media, e := validateBBB(path)
	if e != nil {
		return info, e
	}
	info.Media = &media
	return info, nil
}
func validateBBB(path string) (mediaInfo, error) {
	var result mediaInfo
	version, e := toolVersion("ffprobe", "-version")
	if e != nil {
		return result, e
	}
	result.FFprobeVersion = version
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Minute)
	defer cancel()
	data, e := exec.CommandContext(ctx, "ffprobe", "-v", "error", "-count_frames", "-show_entries", "stream=codec_name,codec_type,pix_fmt,width,height,nb_read_frames:format=format_name,size,duration", "-of", "json", path).Output()
	if e != nil {
		return result, e
	}
	var probe struct {
		Streams []struct {
			Codec       string `json:"codec_name"`
			Type        string `json:"codec_type"`
			PixelFormat string `json:"pix_fmt"`
			Width       int    `json:"width"`
			Height      int    `json:"height"`
			Frames      string `json:"nb_read_frames"`
		} `json:"streams"`
		Format struct {
			Name     string `json:"format_name"`
			Size     string `json:"size"`
			Duration string `json:"duration"`
		} `json:"format"`
	}
	if e = json.Unmarshal(data, &probe); e != nil {
		return result, e
	}
	if !strings.Contains(probe.Format.Name, "matroska") || len(probe.Streams) != 2 {
		return result, errors.New("unexpected public Matroska streams")
	}
	for _, stream := range probe.Streams {
		if stream.Type == "video" && stream.Codec == "h264" {
			result.Codec, result.PixelFormat, result.Width, result.Height = stream.Codec, stream.PixelFormat, stream.Width, stream.Height
			result.Frames, e = strconv.ParseInt(stream.Frames, 10, 64)
			if e != nil {
				return result, e
			}
		} else if stream.Type == "audio" && stream.Codec == "aac" {
			result.AudioCodec = stream.Codec
		} else {
			return result, errors.New("unexpected public video codec")
		}
	}
	result.DurationSeconds, e = strconv.ParseFloat(probe.Format.Duration, 64)
	if e != nil || result.DurationSeconds < 149 || result.DurationSeconds > 151 || result.Frames < 1 || result.Width != 320 || result.Height != 180 || result.AudioCodec != "aac" {
		return result, errors.New("unexpected public video extent")
	}
	stat, e := os.Stat(path)
	if e != nil {
		return result, e
	}
	size, e := strconv.ParseInt(probe.Format.Size, 10, 64)
	if e != nil || size != stat.Size() {
		return result, errors.New("public video byte extent mismatch")
	}
	command := exec.CommandContext(ctx, "ffmpeg", "-hide_banner", "-loglevel", "error", "-xerror", "-i", path, "-map", "0:v:0", "-map", "0:a:0", "-f", "null", "-")
	var stderr bytes.Buffer
	command.Stderr = &stderr
	if e = command.Run(); e != nil {
		return result, fmt.Errorf("public Matroska full decode failed: %w: %s", e, stderr.String())
	}
	return result, nil
}
