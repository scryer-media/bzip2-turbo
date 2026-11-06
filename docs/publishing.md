# Operator-owned publication prerequisites

The public repository is
[bzip2-turbo](https://github.com/scryer-media/bzip2-turbo). The operator pushes
reviewed, signed source history before remote validation and publication.
Implementation and validation work do not authorize publication, tags or pushes.

Before publication, protect the `crates-io` GitHub environment with the desired
approval policy and register the crates.io trusted publisher for this
repository's `.github/workflows/release.yml` workflow and `crates-io` environment.
The initial crate publication, ownership and registration must be established
separately before relying on trusted publishing. The workflow does not bootstrap
these prerequisites. No signing key or `RELEASE_ALLOWED_SIGNERS` is required by
the release workflow.

Run `release` from the Actions tab on `main` and leave the only input,
`dry_run`, true to run checks without publishing. The workflow reads the version
from the checked-out `Cargo.toml`; there is no version prompt.
After separate release authorization, running with `dry_run` false publishes
through the protected environment, creates `v<version>` at the checked commit,
and creates the GitHub release. A pre-existing tag is not required for this
manual route. The workflow creates the tag through `gh release --target`. A
tag already present at another commit is rejected.

Manual runs require `ci-complete` success on that exact commit. They check the
manifest version and the matching changelog heading, rejecting an `Unreleased`
heading, then perform API compatibility and package checks. For the first
publication only, a crates.io 404 means there is no published API baseline;
subsequent releases run `cargo semver-checks`. Other registry errors fail that
baseline check. The release notes are the version's changelog section, or the
file's first section when no heading names the version.

The same workflow also accepts pushed `v*` tags, testing them with Nextest and
doctests before the shared checks and publication. That route uses the existing
tag with `--verify-tag` when creating the GitHub release. Manual workflow tags
created using the workflow token do not trigger a second tag-push workflow.

Retries skip crate versions already published and GitHub releases already
created. `dry_run` defaults to true for manual runs; a pushed tag publishes.
Do not push a release tag or dispatch a publishing run without separate release
authorization. Configuration work does not execute either release route.
