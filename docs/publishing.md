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

Run `release` from the Actions tab on `main`, enter the manifest `version`
(for example, `0.1.0`), and leave `dry_run` true to run checks without publishing.
After separate release authorization, running with `dry_run` false publishes
through the protected environment, creates `v<version>` at the checked commit,
and creates the GitHub release. A pre-existing tag is not required for this
manual route. The workflow creates the tag through `gh release --target`, just
as sevenz-turbo does. A tag already present at another commit is rejected.

Manual runs require `ci-complete` success on that exact commit. They check the
manifest version and the matching changelog heading, rejecting an `Unreleased`
heading, then perform API compatibility and package checks. For the first
publication only, a crates.io 404 means there is no published API baseline;
subsequent releases run `cargo semver-checks`. Other registry errors fail that
baseline check. The changelog heading check and notes extraction follow the
sevenz-turbo workflow, including its first-section notes fallback.

The same workflow also accepts pushed `v*` tags, testing them with Nextest and
doctests before the shared checks and publication. That route uses the existing
tag with `--verify-tag` when creating the GitHub release. Manual workflow tags
created using the workflow token do not trigger a second tag-push workflow.

Retries skip crate versions already published and GitHub releases already
created. `dry_run` defaults to true for manual runs; a pushed tag publishes.
Do not push a release tag or dispatch a publishing run without separate release
authorization. Configuration work does not execute either release route.

The downstream sevenz-turbo manifest declares registry dependency
`bzip2-turbo = "0.1.0"`. The operator separately authorized local downstream
version preparation from 0.29.1 to 0.29.2, subject to patch compatibility
validation; no downstream push, tag or publication is authorized. Local
integration verification can use an invocation-only Cargo `[patch.crates-io]`
path override. Do not commit a machine-specific override. Until the standalone
version is published, unpatched downstream packaging and remote CI cannot
resolve it from crates.io.
