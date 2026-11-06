# Security

Report suspected vulnerabilities through the repository's private GitHub
security advisory form. Include the version, feature set, target, reproduction
steps and a minimal synthetic input. Do not include credentials or private data.

Malformed input must return an error without out-of-bounds access. Decoder
storage is bounded by the validated block-size header. Total decompressed output
can still be large: consumers should enforce application-specific output limits.
Native worker queues are bounded and workers are joined during teardown.

Repository hooks and CI run gitleaks and privacy checks on content and commit
messages. Required upstream legal notices are retained. The
[validation record](docs/validation.md) states the scope of unsafe code and
which checks do not establish OS or SIMD safety.
