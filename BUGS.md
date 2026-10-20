# Known gaps and issues

What pcgtools does not yet do, or does differently from PCGen.  Each
item that is fixed should be removed from this file in the same commit.

## Output

- Only PCGen's base XML sheet (as XML or JSON) is produced, plus the
  fuller sheet's additions with `pcg sheet --full`.  PCGen's other
  output templates (HTML, PDF, text) are not rendered.  Many of them are
  transforms of the base XML, so ours may serve as their input; this is
  untested.
- Export values that no supported sheet uses are not produced (for
  example the numbered temporary-bonus values and their count).
- The fuller sheet's additions are verified only where PCGen can export
  its fuller template; it cannot for Gaslight, Killshot, Spycraft and
  Deadlands.

## Rules

- Game-mode code controls that let typed variables feed the legacy
  system (base and total saves, base and current size) are not modeled.
  No shipped game mode uses them.  The two variable systems are
  otherwise uncoupled by name, as in PCGen.
- PCGen's result caching in formula evaluation is not modeled, so a
  value that depends on it can differ (see the `varies` lines in
  tests/scenarios.txt).
- Formulas naming a variable that nothing defines are read left to
  right as PCGen does; about 200 bonus formulas in the shipped data do
  so (mostly typos in the data).  Only those seen in the reference and
  oracle runs were checked against PCGen.
- `.CLEAR.x` inside class level lines uses the older whole-token
  removal; list tags elsewhere remove only the named item.
- Followers are dropped when their file is missing next to the
  character file or at the written path; PCGen also looks in its
  character directory setting and drops followers whose companion list
  is unknown.  Neither is done.
- Name matching of PCGen's built-in formula terms is case-insensitive
  here, case-sensitive in PCGen.

## Editor

- Character creation:
  - a monster race does not add its monster-class levels automatically;
  - no random name, age, height or weight (new characters have age 0);
  - a stat increase left pending with `--force` can only be spent
    through `set stat`.
- Level-up:
  - an ability choice that needs a further choice is not supported;
  - choices on substitution levels are not offered;
  - a spellcaster choice offers only classes the character has;
  - `class rm` does not undo a level's choices (`undo` does).
- Kits (applied without asking):
  - a SKILL line listing several skills takes the first COUNT in data
    order;
  - level choices that need a pick are skipped, stat increases are left
    in the pool;
  - fractional ranks: PCGen charges points for the dropped fraction,
    pcgtools buys only whole ranks;
  - FREE and COUNT on ABILITY lines are ignored;
  - the monster hit-dice cap on kit levels is not applied;
  - STAT lines are applied in file order, PCGen applies them first;
  - gear is not placed in an equipment set and cannot be chosen by
    type, maximum cost, size or modifier.
- Equipment:
  - items cannot be put inside containers (so container capacity only
    shows when resizing);
  - items worn at the generic "Equipped" location are not moved to
    their natural slot;
  - bought gear is not resized to the character's size;
  - buy and sell rates are fixed at PCGen's defaults.
- Spells: no default spellbook for new known spells; prepared counts are
  set only by repeating the command.
- Temporary bonuses: race, deity, domain, class and equipment-modifier
  sources are not offered; per-set temporary bonus lines (`EQSETBONUS`)
  are kept but never written.
- Companions can be listed, added and removed, but not built as their
  own characters.
- Not attempted: party files, NPC and name generation, preferences.

## File formats

- Version 1 character files are not read.
- `http:` file references are not fetched.
- No counterpart to PCGen's converter for old LST syntax.

## Verification

- Game modes with data but no reference scenario: Darwins_World_2,
  Pathfinder_PFS, Sidewinder.
- Most game modes have one to three scenarios; supplements, monsters,
  prestige classes and high levels are lightly covered.
- PCGen's point-buy budget handling is not exercised by the reference
  test.
- With two worn items carrying spells, PCGen's order of the item
  spellbooks varies between runs; the reference test compares them by
  name.
- In the 3e Dwarf, PCGen itself reports an active bonus loop, so that
  race cannot be a clean reference scenario.

## Documentation

- The editor's `help` for `class add` does not say that a level past
  MAXLEVEL is refused without `--force` (the README does).
- That resizing a container reports its new capacity is not documented.
- The README calls `token` "sub-token" syntax; the crate docs call it
  "choice" syntax.
