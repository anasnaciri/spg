# spg

An interactive Spring Initializr project generator for the command line.

`spg` wraps [Spring Initializr](https://start.spring.io) with a fast, searchable
TUI. It fetches Spring Initializr metadata, prompts only for the fields you have
not provided on the command line, and unpacks the generated project locally.
Flags skip matching prompts, so the same binary works for interactive use and
for scripted workflows.

## Install

### Homebrew

```sh
brew tap anasnaciri/tap
brew install spg
```

### Cargo

```sh
cargo install spg
```

## Usage

```sh
spg init my-api
spg init my-api --build maven --java-version 21 -d web -d validation
spg deps web
spg config show
spg cache clear
```

Run `spg --help` for the full reference.

### Subcommands

- `spg init [NAME]` — create a Spring Boot project. Any flag you pass skips the
  matching prompt; any flag you omit is asked interactively.
- `spg deps [QUERY]` — browse and search Spring Initializr dependencies.
- `spg config show | reset` — inspect or clear saved defaults.
- `spg cache clear` — drop the cached Spring Initializr metadata.

Saved defaults and the metadata cache live in your OS-specific config and cache
directories (via the [`directories`](https://crates.io/crates/directories)
crate); nothing is written into the current working directory.

## Development

```sh
cargo fmt
cargo test
cargo clippy
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.
