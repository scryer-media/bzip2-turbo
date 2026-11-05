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

The existing release workflow accepts an existing SSH-signed annotated `vX.Y.Z`
tag. It verifies both the tag and source commit against the allowed signers,
requires stable ancestry and successful push CI for that exact commit on `main`,
and checks that the manifest version matches the tag. Validation packages the
crate; the separate environment-gated publishing job runs only when explicitly
selected and uses trusted publishing. The workflow never creates or moves tags.
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
