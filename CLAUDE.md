# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`mariadbcli` (WIP) â€” a Rust CLI that wraps the [MariaDB Downloads REST API](https://mariadb.org/downloads-rest-api/) to list and (eventually) download MariaDB products. There is no local database involvement; the tool only does HTTP GETs against `https://downloads.mariadb.org/rest-api/` and prints tables.

## Commands

```bash
cargo build
cargo test                  # offline: parses tests/fixtures/*.json, no network
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all

cargo run -- list                         # products
cargo run -- releases mariadb             # major releases
cargo run -- files mariadb 10.11.6 --os windows --cpu amd64
cargo run -- download 16068 --output ./dist
cargo run -- install --os-release tests/fixtures/os-release/ubuntu-24.04 --dry-run
```

The `--os-release` flag makes `install` planning testable from any platform,
including Windows, where `/etc/os-release` does not exist.

CI (`.github/workflows/rust.yml`) gates on fmt, clippy `-D warnings`, build, and test. A second job, `install-dry-run`, builds inside `ubuntu:24.04`, `debian:12` and `quay.io/rockylinux/rockylinux:9` containers and runs `install --dry-run` there **without** `--os-release`, so it exercises real `/etc/os-release` detection and confirms the URL that produces exists upstream — the part `tests/install.rs` deliberately stubs. A third workflow uploads clippy SARIF to code scanning and is advisory only.

Run a single test with `cargo test --test models parses_release_files`.

## Architecture

Everything lives in the library (`src/lib.rs`); `src/main.rs` is a shim that parses args and calls `Cli::run()`. This split exists so `tests/` can reach the code — integration tests cannot see a binary's private modules.

- `src/api/` — the REST layer.
  - `models.rs` mirrors the response shapes one-to-one, each with a doc link to the documented endpoint. `Release`/`File` are shared across responses. **Both** `PointReleasesRoot.releases` and `ReleaseFilesRoot.release_data` are `HashMap<String, Release>` keyed by version string — not lists, and not a single object. The API returns version-keyed objects in both cases.
  - `requests.rs` exposes a `Client` holding one `reqwest::Client` (connection reuse) plus a base URL. Every JSON endpoint is a thin method over the private generic `get::<T>(path, query)`, which carries the response body into the error on non-2xx (the API reports unknown product/release/file ids that way) and adds context on parse failure. Add endpoints the same way, and keep errors flowing through `anyhow::Result` — nothing here should panic on a network or shape failure.
  - Two endpoints break the JSON pattern deliberately: `signature()` returns **plain text** (a PGP block) via `get_text`, and `download()` streams `resp.chunk()` to disk rather than buffering a multi-hundred-MB body in memory.
  - `Filters { mirror, os, cpu }` carries the documented query parameters; valid ids come from the `mirrors`/`os`/`cpu` endpoints, and note the `--cpu` id (`amd64`) differs from its display name (`x86_64`).
- `src/install/` — installation, the only part of the crate that writes outside the working directory. `mod.rs` holds what both platforms share (version resolution, `require_privileges`, `current_arch`) and re-exports both halves, so `install::Plan` and `install::MsiPlan` are reachable directly. `cli.rs` picks the half at runtime with `cfg!(target_os = "windows")`, which keeps both compiling everywhere.
- `src/install/msi.rs` — the Windows half. There is no repository: `select_installer` picks the one `.msi` out of a release (which also ships a winx64 zip and a debugsymbols msi, so os/cpu matching alone is not enough), the sha256 comes from the same API payload rather than a second request, and `msiexec` runs with `/qn /norestart` — never reboot a machine out from under the caller. `describe` and the executed command both go through `redact`, so a `--password` never reaches the terminal or a log.
- `src/install/repository.rs` — the Linux half: distribution repository setup.
  - Split so everything decidable without touching the system is pure: `Distro::from_os_release` parses, `Plan::build` resolves the exact files and commands, `Plan::describe` renders them, and only `Plan::execute` writes. This is why `tests/install.rs` can pin the whole matrix on Windows.
  - Three mapping rules are easy to get wrong and are covered by tests: derivatives use `UBUNTU_CODENAME` over `VERSION_CODENAME` (Mint's `virginia` has no upstream suite, its `jammy` base does); RHEL-likes publish under the **major** version (`9`, not `9.4`) while Fedora and the SUSE family use the full `VERSION_ID` (`15.6`); and the rpm repositories capitalise the package (`MariaDB-server`) where the deb ones do not (`mariadb-server`). Rocky's os-release id is `rocky` but its repository directory is `rockylinux`.
  - `Plan::verify_repository` is a read-only HEAD run before anything is written — and on `--dry-run` too — so an unpublished combination (e.g. MariaDB 11.4 on el7) fails before the system is touched.
  - The two package families need **different encodings of the same key**, which is easy to get backwards: apt's `Signed-By` needs the *binary* keyring from `supplychain.mariadb.com` (checked against its published `.sha256`), while dnf/zypper import the *ASCII-armored* key straight from the `gpgkey =` URL. Writing armored bytes to the apt keyring path breaks `apt-get update`.
  - Files are written through `write_readable`, which chmods 0644: apt fetches as the unprivileged `_apt` user, so a root umask of 077 would otherwise leave the keyring unreadable.
  - `PlanOptions.repo_base_url` replaces the `deb.mariadb.org` / `rpm.mariadb.org` host wholesale for an internal mirror. It is the **full base URL** with no version or distribution segment: the layout beneath it (`<BASE>/<version>/<flavour>[/<release>/<arch>]`) is unchanged, so a mirror must mirror the upstream paths. `normalize_base_url` trims a trailing slash and rejects non-http(s) schemes, because `verify_repository` is an HTTP request and a scheme it cannot make would silently skip that check. The override deliberately does not move the signing key or the rpm `gpgkey =` URL, so a mirror cannot substitute its own key. The ids from the `mirrors` command belong to the download service and are not valid here.
  - `AptFormat` selects deb822 `.sources` (default) or the one-line `.list` (`--list-format`). Whichever is written, the other is listed in `Plan::remove_files` and deleted — apt reads every file in `sources.list.d`, so leaving both configures the repository twice.
- `src/cli.rs` — clap 4 derive (`Cli` / `Command`) plus the `print_*` functions.
  - The `run()` match is exhaustive over `Command`, so a new subcommand cannot be silently unhandled. Keep it that way rather than reintroducing string-keyed dispatch.
  - `print_*` take a parsed model and render with `prettytable-rs` (`use prettytable::{row, Table}` — no `#[macro_use]`). `FilterArgs` is a `#[command(flatten)]` struct converted into `api::Filters` via `From`.
  - Download progress printing stays in `cli.rs`: `Client::download` takes an `on_progress(written, total)` callback rather than printing itself.

Boundary to respect: `requests.rs` returns parsed models (or bytes) and never prints; `cli.rs` owns all rendering. Where the API returns a version-keyed map, printers sort by `release_id` first, since `HashMap` iteration order is otherwise nondeterministic.

## Testing

`tests/models.rs` deserializes `tests/fixtures/*.json` — captured from the live API — into the models. These are the guard against the third-party response shape drifting away from the hand-written structs, which is exactly how `ReleaseFilesRoot.release_data` came to be wrong. When adding or changing an endpoint, capture a real response into a fixture rather than testing against the network.

## Releasing

Tagging `v*` runs `.github/workflows/release.yml`: verify (the normal gate, plus a check that the tag matches `Cargo.toml`'s version) then a build matrix then `gh release create`. Notes worth keeping in mind when editing it:

- The build uses `--locked`, so a stale `Cargo.lock` fails the release rather than silently resolving different dependencies. Commit the lockfile with any dependency change.
- Permissions are granted per job, not at workflow level: only the publishing job gets `contents: write`. The build job needs `id-token`/`attestations: write` for `actions/attest-build-provenance`, which lets anyone run `gh attestation verify <archive> --repo <repo>`.
- Only first-party actions plus the preinstalled `gh` CLI are used, so a release does not depend on third-party action code.

## Filters 404 rather than returning nothing

The `os`/`cpu` endpoints advertise ids the release-file endpoints will not match: for MariaDB Server only `source` and `windows` return 200, because deb and rpm packages are distributed through the repositories rather than as individual files. A filter matching nothing yields the *same* 404 as an unknown release, so `explain_filtered` in `cli.rs` adds context whenever filters were set. `describe_body` in `requests.rs` keeps the API's plain-sentence error bodies but drops its HTML 404 pages, which are pure noise in a terminal.

## Mirrors have no URL

The `/mirrors` endpoint publishes only `mirror_id` and `mirror_name` for all 94 mirrors. `mirrors --urls` resolves each one by following the `?mirror=<id>` download redirect for a probe file, eight concurrently, then trims the release and package directories off the result (`mirror_base`, unit-tested in `cli.rs` because the string surgery is easy to get wrong). The probe must be a *current* release: mirrors do not carry archived files and the API returns 404 for every mirror if you ask for one. Around a quarter of mirrors legitimately 404 and are shown as `(unavailable)` rather than failing the listing.

## API coverage

Every endpoint documented at <https://mariadb.org/downloads-rest-api/> is implemented, with two deliberate omissions: the checksum, signature and download endpoints also have **by-name** forms (`{product}/{point_release}/{file_name}/checksum`) alongside the short **by-id** forms used here. Only the by-id forms are wired, since `files` prints the ids. Add the by-name variants only if something actually needs them, rather than leaving them as dead code.

`install` covers the distribution repositories rather than the REST API. It has not been executed end-to-end: the planning is fully tested and every generated URL was checked against the live repositories, but writing to `/etc` and running apt/dnf/zypper has not been exercised, since development is on Windows. Run it in a container before trusting it.

`download` writes the file and does not verify it — `checksum` is a separate command, and wiring verification into `download` would mean pulling in a hashing crate.
