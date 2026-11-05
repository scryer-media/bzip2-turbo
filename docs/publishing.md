# Operator-owned publication prerequisites

The public repository is
[bzip2-turbo](https://github.com/scryer-media/bzip2-turbo). The operator pushes
reviewed, signed source history before remote validation and publication.
Implementation and validation work do not authorize publication, tags or pushes.

Before an explicitly authorized publication, the operator must configure the
repository variable `RELEASE_ALLOWED_SIGNERS` with SSH allowed-signers entries,
protect the `crates-io` GitHub environment with the desired approval policy,
and register the crates.io trusted publisher for this repository's
`.github/workflows/release.yml` workflow and `crates-io` environment. First-crate
ownership and registration must be established with crates.io before relying on
that workflow. Do not place private signing material in the repository.

Run the `release` workflow manually from the Actions tab on `main`. Enter
`version` exactly as in `Cargo.toml` (for example, `0.1.0`). `dry_run` defaults
to true and performs validation without uploading or creating a GitHub release.
After separate release authorization, set `dry_run` to false to publish through
the protected `crates-io` environment and create the GitHub release.

An existing, remotely pushed SSH-signed annotated `vX.Y.Z` tag is required even
for validation. The workflow has no private signing key and never creates or
moves tags; the operator must prepare that signed tag separately after release
authorization. Both tag and source commit are verified against the allowed
signers. The exact source commit must be on `main` and have a successful push
run of `ci.yml` on `main`. All later jobs check out that immutable commit.

The manifest must match the requested version. `CHANGELOG.md` must contain
exactly one heading `## X.Y.Z - YYYY-MM-DD` with a valid calendar date and
nonempty release notes. The initial `## 0.1.0` heading is deliberately undated
and must be dated separately during authorized release preparation. Validation
runs `cargo publish --locked --dry-run`; platform tests are covered by the
required exact-commit CI run. There is no published semver baseline for the
first release, so this workflow does not assume one exists.

Publication retries skip an already uploaded crates.io version. Any registry
response other than success or not-found stops publication. A subsequent GitHub
release step checks that the remote tag object still matches the verified tag,
then uses `--verify-tag`; it never asks GitHub to create an unsigned tag. Release
notes come only from the matching dated changelog section. Retrying updates
notes on an existing release. Runs are serialized, including across versions.
These controls do not bootstrap a first crates.io publication: crate ownership
and trusted publisher registration must already be established separately.
Do not bypass signature verification when any prerequisite fails.

The operator's next steps are to review the implementation and validation,
push reviewed signed source history, obtain successful remote CI, configure
the publication prerequisites, and separately authorize the release. These are
pending actions, not completed work. Release validation and publication must
remain separate from the implementation handoff.

The downstream sevenz-turbo manifest declares registry dependency
`bzip2-turbo = "0.1.0"`. The operator separately authorized local downstream
version preparation from 0.29.1 to 0.29.2, subject to patch compatibility
validation; no downstream push, tag or publication is authorized. Local
integration verification can use an invocation-only Cargo `[patch.crates-io]`
path override. Do not commit a machine-specific override. Until the standalone
version is published, unpatched downstream packaging and remote CI cannot
resolve it from crates.io.
