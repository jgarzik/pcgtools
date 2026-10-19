# pcgtools

A Rust library and command line tool for the data files of the
[PCGen](http://pcgen.org) character generator:

* **PCC** campaign files and **LST** data files — parse, resolve,
  load into a merged dataset, query, lint, edit and reformat;
* **game mode** system files;
* **PCG** character files — read, write, compute derived statistics,
  and edit through an interactive or scripted character editor.

Files are read and written losslessly: unmodified bytes (alignment
TABs, comments, line endings, odd encodings) survive a round trip.

[docs/FORMAT.md](docs/FORMAT.md) describes all of these formats.

## Building

    cargo build --release

The binary is `target/release/pcgtools`.

## Data locations

Most commands need to know where data lives:

| Option | Meaning |
|---|---|
| `--pcgen-root DIR` | a PCGen installation (`data/`, `vendordata/`, `homebrewdata/`, `system/`); also `PCGEN_INSTALL` |
| `--data-dir DIR` | data directory (target of `@/` paths) |
| `--vendor-dir DIR`, `--homebrew-dir DIR` | targets of `&/` and `$/` paths |
| `--system-dir DIR` | directory holding `gameModes/` |

When a PCC file lives under a directory named `data`, the data
directory is guessed from it. `--json` switches most output to JSON.

## Commands

### Exploring data

    pcgtools campaigns DATA_DIR [-g GAMEMODE]   # list campaigns
    pcgtools deps BOOK.pcc                      # campaigns and files a selection loads
    pcgtools load BOOK.pcc [MORE.pcc...]        # load; count objects; report problems
    pcgtools show -p BOOK.pcc spell "Magic Missile"
    pcgtools query -p BOOK.pcc spell 'CLASSES~Wizard=9' -f SCHOOL
    pcgtools query -p BOOK.pcc feat -n 'Improved*' TYPE=Combat -c
    pcgtools dump BOOK.pcc > dataset.json
    pcgtools gamemode [NAME] [-f level.lst]
    pcgtools tags [SCOPE]                       # known tags (SPELL, GLOBAL, PRE, ...)
    pcgtools parse FILE                         # syntax tree of a .pcc/.lst/.pcg file

`show` prints the merged object with the file and line of its
definition and of every `.MOD` applied to it. Query conditions are
`TAG`, `!TAG`, `TAG=value`, `TAG~regex` and `TYPE=type`.

### Checking and changing data files

    pcgtools lint BOOK.pcc [-s] [--min warning]
    pcgtools lint -k spell my_spells.lst
    pcgtools edit my_spells.lst "Magic Missile" --set "DESC:Darts of force." --remove TEMPDESC
    pcgtools edit my_equip.lst Rope --rename "Silk Rope" -i
    pcgtools fmt --check data/homebrew
    pcgtools fmt -w my_spells.lst
    pcgtools roundtrip DATA_DIR

`lint` reports unknown or deprecated tags (from a built-in tag
catalog), malformed prerequisites and bonuses, unparseable formulas,
separator mistakes, tokens joined by spaces, encoding problems,
modifications of missing objects, duplicates, and unknown game modes.
`edit` changes one record and leaves every other byte untouched;
`fmt` rewrites files into a canonical layout.

### Characters

    pcgtools pcg show HERO.pcg
    pcgtools pcg lint HERO.pcg       # check the character against its data
    pcgtools pcg stats HERO.pcg [-v] [-e SKILL.Stealth -e COMBAT.AC]

`pcg stats` computes ability scores, base attack bonus, saves, hit
points, armor class and skill totals. `-e CATEGORY.TARGET` explains a
total by listing every bonus that contributes to it.

### Character editor

    pcgtools repl HERO.pcg
    pcgtools repl --new --gamemode MODE --campaign "Campaign Key" --name Brakka NEW.pcg
    pcgtools char HERO.pcg class add Fighter
    pcgtools char HERO.pcg -o COPY.pcg feat add "Power Attack"

`repl` opens an interactive session with line editing and history.
When standard input is not a terminal it reads commands from it,
one per line, so editing can be scripted. `char` runs a single
command and saves the file if it changed.

Commands (`help` in the editor lists them):

    info | stats [-v] | explain CATEGORY.TARGET
    ls KIND [GLOB] [COND...] | show KIND KEY | prereq KIND KEY
    set stat ABB SCORE | set xp N | set align ABB | set FIELD VALUE
    race KEY | deity KEY|none | domain add KEY
    class add KEY [HP] | class rm
    ability add [CATEGORY|]KEY [CHOICE...] | ability rm [CATEGORY|]KEY
    feat add KEY | feat rm KEY
    skill KEY RANKS [CLASS]
    lang add KEY | template add KEY
    equip add NAME [QTY] | equip rm NAME | equip wear NAME [LOCATION] | equip unwear NAME
    undo | save [PATH] | quit

Names are checked against the loaded data. Prerequisites are enforced
for races, classes, abilities, templates and equipment, and skill
ranks are capped at the level maximum; `--force` overrides. After
each change the editor shows the updated level, attack bonus, hit
points, armor class and saves.

Example session:

    $ pcgtools --pcgen-root ~/pcgen repl --new --gamemode MODE --campaign KEY --name Brakka
    > set stat STR 16
    > race Human
    > class add Fighter
    Fighter is now level 1
    level 1  BAB +1  HP 12  AC 12  Fortitude +4 Reflex +2 Will +0
    > feat add "Power Attack"
    > prereq feat "Great Cleave"
    does not qualify for Great Cleave: PRETOTALAB:4
    > save brakka.pcg

## Library

The crate exposes the same functionality:

| Module | Purpose |
|---|---|
| `text` | lossless text decoding (encodings, line endings) |
| `lst` | LST lines, record heads, per-kind grammar, edits, formatting |
| `pcc` | PCC files, file references, path resolution |
| `campaign` | campaign registry and load planning |
| `dataset` | loading a selection into merged objects |
| `token` | prerequisite, bonus, sub-token and formula syntax |
| `gamemode` | game mode files and key migration |
| `catalog`, `lint`, `query` | tag catalog, checks, object filters |
| `pcg` | PCG files and the typed `Character` view |
| `context`, `rules` | loading a character's data; derived statistics |
| `repl` | the editing session and command language |

## Testing

    cargo test

runs unit tests and integration tests against the synthetic data set
under `tests/fixtures/mini`. Corpus tests run against real PCGen
checkouts when `PCGEN_ROOT` names a directory containing `pcgen/`
(and optionally `pcgen-newsources/`):

    PCGEN_ROOT=~/src/pcg cargo test --release

They round-trip every data and character file, load every campaign,
check prerequisite and bonus syntax across the data, and compare
derived statistics with the expected character sheets in PCGen's test
suite. In every game mode, the core values (attack bonus, saves, hit
points, armor class, skills) must match; match counts for every other
section of the sheet are reported per game mode.

Continuous integration runs both: the plain test suite, and the corpus
tests against PCGen data checked out at a pinned revision.

`tools/gen_catalog.py PCGEN_ROOT > src/catalog/tags.tsv` regenerates
the tag catalog. It records facts only: tag names, where they are
valid, and whether they are deprecated.

## Limitations

* Derived statistics follow the classic bonus model. Data that relies
  on the newer `MODIFY`/declared-variable system for core values is
  only partly supported.
* Spells, attacks, weapon damage and companion statistics are not
  computed.
* Version 1 character files (from very old releases) are not read.
* Network (`http:`) file references are not fetched.
