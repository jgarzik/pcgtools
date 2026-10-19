# Changelog

All notable changes to this project are recorded here. Versions follow
[Semantic Versioning](https://semver.org); before 1.0, minor versions
may change the library API.

## 0.5.0 — first release

### Data files

* Parse PCC campaign files, LST data files and game mode system files
  losslessly: unmodified bytes (alignment, comments, line endings,
  encodings) survive a round trip.
* Resolve `@/`, `&/` and `$/` paths, campaign includes and
  `PRECAMPAIGN`; plan load order by campaign rank.
* Load a selection of campaigns into merged objects with `.COPY`,
  `.MOD` and `.FORGET`, single-valued tag replacement, the `.CLEAR`
  forms, `INCLUDE`/`EXCLUDE`, `ALLOWDUPES`, `SOURCEDATE` and
  `FORWARDREF`.
* Structured parsing of prerequisites, bonuses, formulas and
  `CHOOSE`/`ADD`/`AUTO` choices.
* Commands: `campaigns`, `deps`, `load`, `show`, `query`, `dump`,
  `gamemode`, `tags`, `parse`, `lint`, `edit`, `fmt`, `roundtrip`.

### Characters

* Read and write PCG character files (version 2), losslessly, with a
  typed view of the character and key migration.
* Derived statistics: ability scores, attack bonuses, saves, hit
  points, armor class, initiative, skills, movement, vision and
  challenge rating, following the bonus model and the declared
  variable (`MODIFY`) system with code controls.
* Full character sheets (`pcg sheet`, XML or JSON): weapons with
  to-hit, damage and ranges, equipment and load, abilities and
  special abilities, proficiencies, languages, class features,
  domains, companions and spells.
* `pcg stats`, `pcg vars`, `pcg lint`, `pcg show`.

### Character editor

* `repl` (interactive or scripted) and `char` (one command):
  stats, race, alignment, deity and domains, class levels and
  subclasses, abilities with choice checks and pool costs, skills
  with skill point budgets, languages, templates, kits, equipment
  with customization, spells, and followers; prerequisites are
  enforced, with `--force` to override; undo.

### Testing

* Unit and integration tests against a synthetic data set.
* Corpus tests against real data: every campaign loads, every data
  and character file round-trips, and every value of every expected
  character sheet in PCGen's test suite matches, in all its game
  modes.
