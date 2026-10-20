# pcgtools

A Rust library and command line tool for the data files of the
[PCGen](https://pcgen.org) character generator:

* **PCC** campaign files and **LST** data files — parse, resolve,
  load into a merged dataset, query, lint, edit and reformat;
* **game mode** system files;
* **PCG** character files — read, write, compute derived statistics
  and full character sheets, and edit through an interactive or
  scripted character editor.

Files are read and written losslessly: unmodified bytes (alignment
TABs, comments, line endings, odd encodings) survive a round trip.

[FORMAT.md](FORMAT.md) describes all of these formats.

pcgtools is an independent project, not affiliated with or endorsed
by the PCGen project. It ships no game data.

## Installing

With a Rust toolchain (1.95 or newer):

    cargo install pcgtools

or from a checkout:

    cargo build --release      # binary: target/release/pcgtools

The library is the `pcgtools` crate; its API documentation is on
[docs.rs](https://docs.rs/pcgtools).

## Getting data

pcgtools works on PCGen data you already have: a PCGen installation
(its `data/`, `system/`, `vendordata/` and `homebrewdata/`
directories), a checkout of PCGen's source repository, or your own
homebrew files. Characters need the game mode and campaigns they were
made with. Game data is subject to the licenses of its publishers;
pcgtools does not include or redistribute any.

A first look:

    pcgtools --pcgen-root ~/pcgen campaigns ~/pcgen/data -g 35e
    pcgtools --pcgen-root ~/pcgen pcg stats HERO.pcg
    pcgtools --pcgen-root ~/pcgen pcg sheet HERO.pcg > hero.xml

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
    pcgtools pcg lint HERO.pcg       # check the character against its data and follower files
    pcgtools pcg stats HERO.pcg [-v] [-e SKILL.Stealth -e COMBAT.AC]
    pcgtools pcg sheet HERO.pcg [--full] [--compare EXPECTED.xml]
    pcgtools pcg vars HERO.pcg [-s PC.STAT]

`pcg stats` computes ability scores, base attack bonus, saves, hit
points, armor class and skill totals. `-e CATEGORY.TARGET` explains a
total by listing every bonus that contributes to it.

`pcg sheet` builds the full character sheet as XML (or JSON with
`--json`): abilities, hit points, armor class, initiative, saves,
skills, attacks, weapons with to-hit and damage per mode and range,
protection, equipment and load, feats and special abilities,
proficiencies, languages, class features, domains, companions and
spells. `--full` adds what PCGen's fuller sheet shows beyond that: a
row for each skill situation with a bonus (`Perception (Underwater)`)
and each spell's concentration. `--compare` lists every value that
differs from an expected sheet. `pcg vars` shows the variables the
data declares and their values, by scope.

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

    info | stats [-v] | explain CATEGORY.TARGET | vars [SCOPE]
    ls KIND [GLOB] [COND...] | show KIND KEY | prereq KIND KEY
    set stat ABB SCORE | set xp N | set align ABB | set FIELD VALUE
    set gold N|+N|-N | set debt on|off
    stats buy [METHOD] | stats roll [METHOD] [--seed N] | stats same SCORE
    stats assign SCORE... | stats method
    race KEY | deity KEY|none | domain add KEY
    class add KEY [HP] [--subclass SUB] [--choose CHOICE]... [--stat ABB]...
    class rm | class subclass KEY SUB
    ability add [CATEGORY|]KEY [CHOICE...] | ability rm [CATEGORY|]KEY
    feat add KEY [CHOICE...] | feat rm KEY
    skill KEY RANKS [CLASS]
    lang add KEY | template add KEY | kit apply KEY
    equip add NAME [QTY] | equip rm NAME
    equip wear NAME [LOCATION] [--set SET] | equip unwear NAME [--set SET]
    equip customize NAME +MOD[|CHOICE] | -MOD | size SIZE
    equip buy NAME [QTY] | equip sell NAME [QTY|all]
    eqset ls | eqset show [SET] | eqset use SET
    eqset add NAME | eqset copy SET NAME | eqset rename SET NAME | eqset rm SET
    spell add CLASS SPELL [LEVEL] [BOOK] | spell rm CLASS SPELL [BOOK]
    spell prep CLASS SPELL [LIST] [--meta FEAT[,FEAT]] [--level N] [--domain D]
    spell unprep CLASS SPELL [LIST] [--meta FEAT[,FEAT]]
    spell scribe CLASS SPELL BOOK [--level N] | spell unscribe CLASS SPELL BOOK
    spellbook add NAME | spellbook rm NAME | spellbook ls
    temp ls [GLOB] | temp add [KIND=]SOURCE [pc|ITEM] [--value N]
    temp rm SOURCE [pc|ITEM] | temp on|off SOURCE [pc|ITEM] | temp mods on|off
    follower add NAME TYPE RACE [FILE] | follower rm NAME | follower ls
    undo | save [PATH] | quit

Names are checked against the loaded data. Prerequisites are enforced
for races, classes, abilities, templates and equipment; an ability's
choices must be ones its `CHOOSE` offers; abilities are paid for from
their category's pool (bonus pools, the race's starting feats and the
game mode's feat schedule); skill ranks are capped at the level
maximum and paid for with the class's skill points; a spell is
prepared only if it is known and a slot of its level (raised by any
metamagic feats it is prepared with) is free, and written in a
spellbook item only if the book has the pages left. `info` shows the
pools and skill points left. A class level that offers choices
(`ADD:` tokens granting spellcasting class levels, abilities, class
skills or languages) takes them from `--choose`, in order; a choice
with only as many options as it grants is made without asking; a
class skill chosen with `AUTORANK` also gets its ranks bought when the
points allow. A level's hit points must lie within the hit die's
range as moved by `HD` `MIN` and `MAX` bonuses (the top by default),
and no class goes past its `MAXLEVEL`. Domains, bonus languages (from
those offered) and followers per companion list are limited to the
numbers the rules allow, and `equip wear` refuses an item when its
body slot is full.
Scores are generated by the game mode's point buy (scores bought
within its range and budget), its dice methods, one score for all, or
scores given in stat order; the method is recorded as PCGen records
it. A level that brings a stat increase (`BONUSSTATLEVELSTARTINTERVAL`)
takes the stat to raise from `--stat`; the increase, like any change
to a score after the first level, is recorded with the level, and
`class rm` takes it back. Scores must lie in the stat's `STATRANGE`.
`equip buy` pays the item's price from the character's gold, refusing
to go into debt unless `set debt on`; `equip sell` refunds half the
price (all of it for coins, gems and trade goods), PCGen's default
rates. `equip add` and `equip rm` cost and refund nothing. A kit adds
its funds and pays for its gear as PCGen does: at its `EQUIPBUY`
rate, as much as the funds allow, or once at its `TOTALCOST`; lines
whose prerequisites fail are skipped. Its class levels are gained as
`class add` gains them, and its skill ranks are paid for from the
skill points (unless free).
Temporary bonuses (`TEMPBONUS` on abilities, spells, templates, skills
and worn items) apply to the character or to an item in the equipment
set used that has the types they ask for; one asking for a number
(`TEMPVALUE`) takes it from `--value`, within its range. They are
recorded as PCGen records them, switched off ones included, and
`temp mods` turns all of them on or off for the character and its
equipment set.
`--force` overrides the checks. After
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
| `context`, `rules` | loading a character's data; derived statistics, variables, equipment, spells, pools |
| `sheet` | the full character sheet |
| `repl` | the editing session and command language |

## Testing

    cargo test

runs unit tests and integration tests against the synthetic data set
under `tests/fixtures/mini`. Corpus tests run against real PCGen
checkouts when `PCGEN_ROOT` names a directory containing `pcgen/`
(and optionally `pcgen-newsources/`):

    PCGEN_ROOT=~/src/pcg cargo test --release

They round-trip every data and character file, load every campaign,
check prerequisite, bonus and choice syntax across the data, run
editor sessions on real data, and compare full character sheets with
the expected sheets in PCGen's test suite: every value of every
expected sheet must match, in every game mode. Match counts are
reported per game mode and sheet section; `ORACLE_ONLY=name`,
`ORACLE_SECTION=section` and `ORACLE_VERBOSE=1` narrow the comparison
and print the differences.

Reference tests go further: each scenario in `tests/scenarios.txt`
builds a character with the editor, has PCGen itself export that
character's sheet, and requires every value to match the sheet
pcgtools builds, apart from values a scenario marks as known PCGen
errors (`differs`, or `varies` when PCGen is only sometimes wrong);
PCGen's complaints on loading the character fail the test unless a
`warns` line expects them. They need a PCGen build (`./gradlew jar copyToLibs`
in a PCGen checkout, with the JavaFX jars in `mods/lib`) and a Java
runtime that can run it:

    PCGEN_ROOT=~/src/pcg PCGEN_REF=~/src/pcg/pcgen cargo test --release reference

A scenario marked `full` is also exported with PCGen's fuller sheet
template, and the situational skill rows and spell concentration it
adds must match `pcg sheet --full`; `REF_FULL=1` does this for every
scenario. `REF_ONLY=name` runs one scenario and `REF_VERBOSE=1` prints
every difference; `PCGEN_JAVA` or `JAVA_HOME` selects the Java runtime.
Without their inputs these tests are skipped (with a note on stderr);
`PCGEN_REQUIRE=root,ref` makes a missing `PCGEN_ROOT` or `PCGEN_REF`
a failure instead, as continuous integration does.

Continuous integration runs both: the plain test suite, and the corpus
tests against PCGen data checked out at a pinned revision.

`python3 gen_catalog.py PCGEN_ROOT > src/catalog/tags.tsv` regenerates
the tag catalog. It records facts only: tag names, where they are
valid, and whether they are deprecated.

## Limitations

[BUGS.md](BUGS.md) lists the known gaps and the places where pcgtools
differs from PCGen.

## Contributing

Bug reports and pull requests are welcome at
<https://github.com/jgarzik/pcgtools>. For a wrong statistic or sheet
value, please include the character file and the campaigns it uses.
Before submitting a change, run `cargo fmt`, `cargo clippy
--all-targets -- -D warnings` and `cargo test` (with `PCGEN_ROOT` set,
if you can). Contributions must be your own work: do not copy code
or documentation from other projects.

## License

MIT; see [LICENSE](LICENSE).
