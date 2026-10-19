# pcgtools

Tools and a Rust library for reading, querying, editing and writing the
data files of the [PCGen](http://pcgen.org) character generator: PCC
campaign files, LST data files, game mode system files, and PCG
character files.

See [docs/FORMAT.md](docs/FORMAT.md) for a description of the formats.

## Testing

`cargo test` runs the unit tests and the synthetic fixtures under
`tests/fixtures/`. Corpus tests run against a real PCGen checkout when
`PCGEN_ROOT` points at a directory containing `pcgen/` (and optionally
`pcgen-newsources/`):

    PCGEN_ROOT=~/tmp/repo/pcg cargo test
