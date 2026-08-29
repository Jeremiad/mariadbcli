# mariadbcli

A command-line tool for finding, verifying, downloading and installing MariaDB
releases. It speaks the [MariaDB Downloads REST
API](https://mariadb.org/downloads-rest-api/), so the list of products, releases
and files always comes from MariaDB rather than from anything hard-coded here.

Two things it does:

- **Browse and fetch.** Walk from products down to a single file, filter by OS
  and architecture, read the published checksums and PGP signatures, and stream
  a download to disk.
- **Install.** Set up the MariaDB package repository and install the server on
  Linux, or fetch and run the published MSI on Windows — after showing you
  exactly what it is about to do.

It never connects to a database. Every command is a read of MariaDB's public
download service, except `install`, which changes the local system and says so
first.

## Installing

Download a build for your platform from the
[releases page](https://github.com/Jeremiad/mariadbcli/releases), unpack it, and
put `mariadbcli` somewhere on your `PATH`. Each archive ships with a `.sha256`
next to it:

```
$ sha256sum -c mariadbcli-v0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256
mariadbcli-v0.1.0-x86_64-unknown-linux-gnu.tar.gz: OK
```

Release archives also carry a build provenance attestation, so you can confirm an
archive really came from this repository's release workflow:

```
$ gh attestation verify mariadbcli-v0.1.0-x86_64-unknown-linux-gnu.tar.gz \
    --repo Jeremiad/mariadbcli
```

### Building from source

```
cargo build --release
```

The binary lands in `target/release/mariadbcli`. Rust 1.70 or newer.

## Browsing releases

The commands chain: each one prints the ids the next one needs.

### `list` — what products exist

```
$ mariadbcli list
Products:
+------------------+---------------------------+---------+
| Product id       | Product name              | License |
+------------------+---------------------------+---------+
| documentation    | Documentation             |         |
+------------------+---------------------------+---------+
| mariadb          | MariaDB Server            | GPL     |
+------------------+---------------------------+---------+
| mariadb-galera   | MariaDB Galera Cluster    | GPL     |
+------------------+---------------------------+---------+
| connector-java   | MariaDB Connector/J       | LGPL    |
+------------------+---------------------------+---------+
```

The server's product id is `mariadb`, not `mariadb-server`.

### `releases <product>` — major releases

```
$ mariadbcli releases mariadb
Product releases:
+-------+-------+---------+
| Id    | Name  | Status  |
+-------+-------+---------+
| 13.1  | 13.1  | Preview |
+-------+-------+---------+
| 13.0  | 13.0  | RC      |
+-------+-------+---------+
| 12.3  | 12.3  | Stable  |
+-------+-------+---------+
| 11.4  | 11.4  | Stable  |
+-------+-------+---------+
```

### `point-releases <product> <major>` — versions within a major release

```
$ mariadbcli point-releases mariadb 10.11
Point releases:
+----------+------------------------------+--------------+---------------+-------+
| Id       | Name                         | Release date | Release notes | Files |
+----------+------------------------------+--------------+---------------+-------+
| 10.11.0  | MariaDB Server 10.11.0 Alpha | 2022-09-26   |               | 5     |
+----------+------------------------------+--------------+---------------+-------+
| 10.11.10 | MariaDB Server 10.11.10      | 2024-11-04   |               | 8     |
+----------+------------------------------+--------------+---------------+-------+
```

### `files <product> <point-release>` — the actual downloads

```
$ mariadbcli files mariadb 10.11.6 --os windows --cpu amd64
+---------+------------------------+--------------+
| Id      | Name                   | Release date |
+---------+------------------------+--------------+
| 10.11.6 | MariaDB Server 10.11.6 | 2023-11-14   |
+---------+------------------------+--------------+

Files:
+-------+-----------------------------------------+-------------+---------+--------+
| Id    | Name                                    | Type        | OS      | CPU    |
+-------+-----------------------------------------+-------------+---------+--------+
| 16068 | mariadb-10.11.6-winx64.zip              | ZIP file    | Windows | x86_64 |
+-------+-----------------------------------------+-------------+---------+--------+
| 16066 | mariadb-10.11.6-winx64-debugsymbols.zip | ZIP file    | Windows | x86_64 |
+-------+-----------------------------------------+-------------+---------+--------+
| 16067 | mariadb-10.11.6-winx64.msi              | MSI Package | Windows | x86_64 |
+-------+-----------------------------------------+-------------+---------+--------+
```

The `Id` in the second table is the **file id**, which `checksum`, `signature`
and `download` all take.

### `latest <product> <major>` — skip straight to the newest

```
$ mariadbcli latest mariadb 10.11
```

Same output as `files`, for whichever point release is newest — no need to look
one up first.

## Getting help

Every command takes `-h` for a summary and `--help` for the full detail,
including examples:

```
$ mariadbcli --help              # commands, the chain between them, examples
$ mariadbcli install -h          # options grouped by platform
$ mariadbcli install --help      # the same, with the reasoning behind each
```

## Filtering

`point-releases`, `files` and `latest` accept `--os`, `--cpu` and `--mirror`.
The valid ids come from three commands, because they are not the names you might
guess — the architecture id is `amd64` while its display name is `x86_64`:

```
$ mariadbcli os
Operating systems:
+---------------+---------------+
| Os id         | Name          |
+---------------+---------------+
| deb_package   | DEB Package   |
+---------------+---------------+
| linux_generic | Generic Linux |
+---------------+---------------+
| rpm_package   | RPM Package   |
+---------------+---------------+
| source        | Source Code   |
+---------------+---------------+
| windows       | Windows       |
+---------------+---------------+

$ mariadbcli cpu          # amd64, x86_32, ppc64, ppc64le
$ mariadbcli mirrors      # mirror ids, grouped by country
```

`mirrors --urls` also shows where each mirror serves from:

```
$ mariadbcli mirrors --urls
Resolving 94 mirrors against mariadb-12.3.3-winx64.msi...
Mirrors:
+-----------+-----------------+----------------------------+------------------------------------------+
| Country   | Mirror id       | Mirror name                | Serves from                              |
+-----------+-----------------+----------------------------+------------------------------------------+
| Australia | aarnet_pty_ltd  | AARNet Pty Ltd - Brisbane  | http://mirror.aarnet.edu.au/pub/MariaDB/ |
+-----------+-----------------+----------------------------+------------------------------------------+
| Australia | digital-pacific | Digital Pacific - Sydney   | (unavailable)                            |
+-----------+-----------------+----------------------------+------------------------------------------+
| Austria   | alwyzon         | Alwyzon - Vienna           | https://mirror.alwyzon.net/mariadb/      |
+-----------+-----------------+----------------------------+------------------------------------------+
```

The API publishes no URL for a mirror, only an id, so each one has to be
resolved by following its download redirect — one request per mirror, eight at a
time, a few seconds in total. That is why it is opt-in rather than the default.
Mirrors that do not carry the probe file are shown as `(unavailable)`; at the
time of writing 68 of the 94 resolve.

So, the source tarball for a release:

```
$ mariadbcli files mariadb 10.11.6 --os source
```

For MariaDB Server only `--os source` and `--os windows` actually match. The
Linux packages are published through the apt and rpm repositories rather than as
individual files, so `--os deb_package` and `--os rpm_package` find nothing —
use `install` for those. The API answers a filter that matches nothing with the
same 404 it uses for an unknown release, so the tool adds an explanation:

```
$ mariadbcli files mariadb 11.4.13 --os rpm_package
Error: nothing matched `--os rpm_package`; for MariaDB Server only --os source
and --os windows match, because Linux packages are published through the apt and
rpm repositories rather than as individual files (see `mariadbcli install`)
```

## Verifying and downloading

All three take a file id from `files`.

```
$ mariadbcli checksum 16992
+-----------+------------------------------------------------------------------+
| Algorithm | Checksum                                                         |
+-----------+------------------------------------------------------------------+
| md5sum    | e31c4909ee82e7db5f0e714cea4e90fe                                 |
+-----------+------------------------------------------------------------------+
| sha1sum   | 497dcf674f0a1732b87365d869ad7c9f0af46ca8                         |
+-----------+------------------------------------------------------------------+
| sha256sum | 811a38a862c1c55325b6ba8a757b381923e51dee4fe3cde95887b0ae2490c02d |
+-----------+------------------------------------------------------------------+

$ mariadbcli signature 16992 > mariadb.msi.asc     # detached PGP signature

$ mariadbcli download 16992 --output ./dist
Downloading... 100% (92852224/92852224 bytes)
Saved to ./dist/mariadb-12.3.3-winx64.msi
```

`download` streams to disk rather than buffering, so a multi-hundred-megabyte
file costs no memory. The name comes from the mirror it is redirected to.
`download` does **not** verify what it wrote — compare it against `checksum`
yourself:

```
$ sha256sum ./dist/mariadb-12.3.3-winx64.msi
811a38a862c1c55325b6ba8a757b381923e51dee4fe3cde95887b0ae2490c02d  ...
```

`--mirror <id>` picks a specific download mirror.

## Installing MariaDB

`install` is the one command that changes your machine. It always prints the
complete plan first, asks before applying it, and needs administrative rights.
**`--dry-run` shows the plan and changes nothing** — start there.

`--version` takes a major release (`11.4`) or, on Windows, a point release
(`11.4.13`). Omit it for the newest stable release.

### Linux

The MariaDB package repository is configured for the running distribution, then
the server is installed with the system package manager.

```
$ mariadbcli install --dry-run
Distribution:    Ubuntu 24.04.1 LTS
MariaDB version: 12.3
Package manager: apt-get
Repository base: https://deb.mariadb.org

Would verify:
  https://deb.mariadb.org/12.3/ubuntu/dists/noble/Release

Would install the signing keyring:
  https://supplychain.mariadb.com/mariadb-keyring-2025.gpg -> /etc/apt/keyrings/mariadb-keyring.gpg
  verified against https://supplychain.mariadb.com/mariadb-keyring-2025.gpg.sha256

Would write /etc/apt/sources.list.d/mariadb.sources:
  | Types: deb
  | URIs: https://deb.mariadb.org/12.3/ubuntu
  | Suites: noble
  | Components: main
  | Architectures: amd64
  | Signed-By: /etc/apt/keyrings/mariadb-keyring.gpg

Would remove, if present (apt would otherwise read the repository twice):
  /etc/apt/sources.list.d/mariadb.list

Would run:
  apt-get update
  apt-get install -y mariadb-server

Repository verified.
```

Then apply it:

```
$ sudo mariadbcli install --version 11.4
$ sudo mariadbcli install --version 11.4 --yes     # no confirmation prompt
```

Supported: Debian and Ubuntu, including derivatives such as Linux Mint (mapped
to their upstream suite); RHEL, CentOS, AlmaLinux, Rocky and Fedora; openSUSE
and SLES.

Two Linux-only options:

- `--list-format` writes the one-line `/etc/apt/sources.list.d/mariadb.list`
  instead of the deb822 `mariadb.sources`. Either way the other file is removed
  if present, so apt does not read the repository twice.
- `--repo-base-url` points at an internal mirror. See below.

### Windows

The MSI published for the release is fetched through the downloads API, checked
against the sha256 that API publishes, and handed to `msiexec /qn /norestart`.

```
> mariadbcli install --dry-run
Target:          Windows (x86_64)
MariaDB version: 12.3.3
Package:         mariadb-12.3.3-winx64.msi (file 16992)

Would verify sha256:
  811a38a862c1c55325b6ba8a757b381923e51dee4fe3cde95887b0ae2490c02d

Would download into:
  C:\Users\you\AppData\Local\Temp\mariadbcli

Would run:
  msiexec /i ...\mariadb-12.3.3-winx64.msi SERVICENAME=MariaDB PORT=3306 /qn /norestart
```

From an elevated prompt:

```
> mariadbcli install --version 11.4 --port 3307 --service-name MariaDB114
> mariadbcli install --password "s3cret" --data-dir D:\mariadb\data --yes
> mariadbcli install --msi-property UTF8=1 --msi-property STDCONFIG=1
```

| Option | MSI property | Default |
| --- | --- | --- |
| `--service-name` | `SERVICENAME` | `MariaDB` |
| `--port` | `PORT` | `3306` |
| `--password` | `PASSWORD` | unset |
| `--install-dir` | `INSTALLDIR` | MSI default |
| `--data-dir` | `DATADIR` | MSI default |
| `--msi-property K=V` | anything else | — |

`SERVICENAME` defaults to a real name because the MSI creates **no service** when
it is empty. `/norestart` is always passed, so the installer will not reboot the
machine.

A password passed on the command line is visible to other processes while the
installer runs. It is redacted from the printed plan (`PASSWORD=***`), but if
that matters to you, set the root password after installation instead.

### Installing from a mirror

`--repo-base-url` takes the **full base URL** of a repository mirror and replaces
`https://deb.mariadb.org` or `https://rpm.mariadb.org` wholesale. Pass the host
and any path prefix — no version or distribution segment, no trailing slash
(a trailing slash is ignored):

```
$ sudo mariadbcli install --repo-base-url https://mirror.example.com/mariadb
```

Everything below the base is unchanged, so the mirror must reproduce the upstream
layout. With the base above, MariaDB 11.4 resolves to:

| Distribution | Resulting repository URL |
| --- | --- |
| Ubuntu 24.04 | `https://mirror.example.com/mariadb/11.4/ubuntu` (suite `noble`) |
| Debian 12 | `https://mirror.example.com/mariadb/11.4/debian` (suite `bookworm`) |
| Rocky 9 | `https://mirror.example.com/mariadb/11.4/rockylinux/9/x86_64` |

That is `<BASE>/<version>/<flavour>` for apt and
`<BASE>/<version>/<flavour>/<release>/<arch>` for rpm. The URL must be `http` or
`https`, because the repository is verified with an HTTP request before anything
is written — and that verification runs against your mirror, so a base URL that
does not serve the expected layout fails before the system is touched.

The override does **not** move the signing key: it is still fetched from MariaDB
and checked against its published checksum, so a mirror cannot substitute a key
of its own. Note that the mirror ids from `mariadbcli mirrors` are **not** valid
here — those belong to the file-download service, not to the apt/rpm
repositories.

## Safety

`install` is the only command that writes outside the working directory, so it is
built to be predictable:

- **Nothing happens without a plan.** Every file and command is printed first,
  and `--dry-run` stops there.
- **The repository is verified before anything is written.** An unsupported
  combination — say MariaDB 11.4 on CentOS 7, which MariaDB does not publish —
  fails up front rather than leaving a broken repository behind. `--dry-run`
  performs this check too.
- **Signing material is verified.** The apt keyring is checked against its
  published sha256, and the Windows MSI against the sha256 from the downloads
  API, before either is used.
- **Root or Administrator is checked first**, so you get a clear message rather
  than a half-finished install.
- **Passwords are redacted** from printed plans and echoed commands.

Errors are plain and exit non-zero:

```
$ mariadbcli releases nosuchproduct
Error: https://downloads.mariadb.org/rest-api/nosuchproduct returned 404 Not Found: The product or release you searched for does not exist.
```

## Development

```
cargo build
cargo test                                          # offline; parses JSON fixtures
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all
```

Tests are offline: they parse captured API responses from `tests/fixtures/`, so
they check that the models still match what MariaDB publishes and that install
planning is right for each distribution. CI additionally runs `install --dry-run`
inside real Ubuntu, Debian, Rocky and Windows images.

## Releasing

Releases are cut by tagging. Bump `version` in `Cargo.toml`, commit, then:

```
$ git tag v0.2.0
$ git push origin v0.2.0
```

That triggers `.github/workflows/release.yml`, which:

1. **Verifies** &mdash; refuses to go further unless `cargo fmt --check`, Clippy
   with `-D warnings` and the tests all pass, and the tag matches the version in
   `Cargo.toml`.
2. **Builds** a binary for `x86_64-unknown-linux-gnu` and
   `x86_64-pc-windows-msvc`, smoke-tests each one, packages it with the README
   and licence, and writes a `.sha256` beside it.
3. **Publishes** a GitHub release with generated notes and every archive
   attached.

A tag with a suffix, such as `v0.2.0-rc1`, is published as a prerelease.
Adding another platform is one entry in the workflow's build matrix.

## License

MIT. See [LICENSE](LICENSE).
