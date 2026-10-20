# PCGen Data File Formats

This document describes the on-disk formats used by the PCGen character
generator, as implemented by `pcgtools`. It is an independent
description, written from studying the behavior of PCGen and the data
that ships with it. Where PCGen's behavior is lenient or surprising,
this document says so and describes what `pcgtools` does.

Contents:

1. [Overview](#1-overview)
2. [Text encoding and lines](#2-text-encoding-and-lines)
3. [LST data files](#3-lst-data-files)
4. [PCC campaign files](#4-pcc-campaign-files)
5. [Per-kind structure and loading](#5-per-kind-structure-and-loading)
6. [Token value syntax](#6-token-value-syntax)
7. [Game mode files](#7-game-mode-files)
8. [PCG character files](#8-pcg-character-files)
9. [Derived values](#9-derived-values)
10. [Character sheets](#10-character-sheets)

Appendices: [A. A complete small example](#appendix-a-a-complete-small-example),
[B. Tag catalog](#appendix-b-tag-catalog)

Conventions: `<TAB>` is a horizontal tab (U+0009). Grammar fragments use
EBNF with `|` for alternation, `[ ]` for optional, and `{ }` for
zero-or-more repetition. Literal text is quoted.

---

## 1. Overview

PCGen data is plain text, divided into four families:

| Family | Extension | Purpose |
|---|---|---|
| Campaign | `.pcc` | Describes a source book: metadata, plus the list of data files it contributes |
| Data | `.lst` | Game objects (races, classes, spells, equipment, ...), one object per line |
| Game mode | `.lst` under `system/gameModes/<mode>/` | Rules-system constants: stats, saves, levels, sizes, UI tabs |
| Character | `.pcg` | A saved character |

A *data set* is loaded by choosing one or more campaigns. Each campaign
names data files by kind (for example `SPELL:` names a spell file), may
include further campaigns, and may filter which objects of a file it
wants. PCGen then loads all named files kind-by-kind in a fixed order,
applies modifications, and resolves cross references.

Typical directory layout:

```
<pcgen>/
  data/                 campaign and data files, any depth
  vendordata/           third-party data (same structure)
  homebrewdata/         user data (same structure)
  system/gameModes/<mode>/   game mode files
  characters/           saved characters
```

## 2. Text encoding and lines

### 2.1 Encoding

Files are expected to be UTF-8. A leading UTF-8 byte-order mark
(`EF BB BF`) is tolerated and ignored by PCGen.

Real data does not always comply: a small number of shipped files
contain Latin-1 bytes or truncated UTF-8 sequences. PCGen refuses to
load such a file. `pcgtools` instead decodes each line independently:
lines that are valid UTF-8 are decoded as UTF-8, and any other line is
decoded as Latin-1 (one byte per character) and flagged. Writing the
file back re-encodes flagged lines the same way, so unmodified files
round-trip byte-for-byte. When loading a campaign's objects, though, a
data file with any flagged line is left out entirely, as PCGen does,
and reported as an error.

### 2.2 Line endings

Lines end in LF, CRLF, or a lone CR; files mixing these exist. The final
line may lack a terminator. `pcgtools` records the terminator of each
line individually. Newly inserted lines use the file's most common
terminator.

### 2.3 Comments and blank lines

In every format, a line whose first character is `#` is a comment, and a
line containing only whitespace is ignored. Comments must occupy a whole
line; there is no trailing-comment syntax. (In LST data files the `#`
must be in the very first column; PCC and game mode files trim leading
whitespace first.)

### 2.4 No escaping

LST and PCC files have no escape mechanism. The characters TAB, `|`,
`:`, `,`, `=`, `[`, `]`, `(`, `)` and `.` are structural in various
contexts, which is why object names are conventionally restricted from
using them. PCG character files are the exception: they use named
entities (see chapter 8).

---

## 3. LST data files

### 3.1 Lines and cells

An LST file is a sequence of lines. Every line falls into one of five
classes, decided by its first characters:

| Class | Rule | Meaning |
|---|---|---|
| blank | only whitespace | ignored |
| comment | `#` in column 1 | ignored |
| source header | starts with `SOURCE` | file-wide source defaults (3.4) |
| continuation | starts with TAB (or space) | more tokens for the preceding record (3.5) |
| record | anything else | an object definition or a sub-line |

A record line is split on TAB into *cells*. The first non-empty cell is
the *head*; every further non-empty cell is a *token*. Runs of TABs
(empty cells) are insignificant and are commonly used to align columns
for readability:

```
Rusty Spoon<TAB><TAB>TYPE:Goods.Kitchen<TAB><TAB>COST:1<TAB>WT:0.5
```

Whitespace at the start or end of a token is ignored, but whitespace
*inside* a token is significant. A space used in place of a TAB merges
two tokens into one and is a common data error.

```
record   = head { TAB { TAB } token } ;
token    = tag [ ":" value ] ;
tag      = upper { upper | digit | "_" } ;   (* by convention *)
value    = { any char except TAB } ;
```

The tag ends at the **first** colon; the value may itself contain
colons (`DESC:Ratio 2:1`). A handful of tokens (for example `.CLEAR`
style forms, or bare keywords on some kit lines) have no colon at all.

Tags are case-sensitive and conventionally upper case. Which tags are
legal depends on the file kind (chapter 5); a small set of *global*
tags (such as `KEY`, `TYPE`, `DESC`, `SOURCEPAGE`, `BONUS`, the `PRExxx`
family, `DEFINE`, `CHOOSE`, `ADD`, `AUTO`) is accepted in almost every
object kind.

### 3.2 Heads and object identity

For ordinary object files, the head is the object's **name** (its
display name). The object's **key** — the identity other data uses to
refer to it — is the name, unless a `KEY:` token supplies a different
one. Keys are case-sensitive and must be unique within an object kind
(abilities: within a category).

Some file kinds use a `TAG:value` head instead of a bare name (for
example class files use `CLASS:Fighter`, kit files use
`STARTPACK:Name`, and data control files use `FACTDEF:...`). These are
described per kind in chapter 5.

### 3.3 Modification heads

A head may carry a suffix that turns the line into an operation on an
object defined elsewhere (usually in another source book):

| Head form | Operation |
|---|---|
| `Name` | define a new object |
| `Name.MOD` | add the line's tokens to the existing object `Name` |
| `Name.COPY=NewName` | duplicate `Name` as a new object `NewName`, then apply the line's tokens to the copy |
| `Name.FORGET` | delete the existing object `Name` |

`Name` here is the target's **key**. For abilities the key must be
qualified with the ability category: `CATEGORY=FEAT|Power Attack.MOD`.
Class modification heads keep the `CLASS:` prefix:
`CLASS:Fighter.MOD`. A modification, copy or forget head drops any
leading `TAG:` to find its target, but outside the kinds whose heads
are `TAG:name` (classes, kits and the like) a defining head keeps it as
part of the name: `DEITY:None` in a deity file defines an object keyed
`DEITY:None`.

The suffix is recognized anywhere after the first character of the
head, with `.COPY` checked first, then `.MOD`, then `.FORGET`. (Names
containing these strings therefore cannot be defined directly.)

Operations are not applied in file order. See 5.3 for the deferral
rules.

### 3.4 Source header lines

A line beginning with `SOURCE` carries TAB-separated source tags
(`SOURCELONG`, `SOURCESHORT`, `SOURCEWEB`, `SOURCEDATE`, `SOURCEPAGE`,
`SOURCELINK`). They are not an object. Instead they set default source
information that is applied to every object *defined* after that line
in the same file. Each file conventionally starts with exactly one such
line; the defaults reset at the start of each file.

```
SOURCELONG:Book of Tests<TAB>SOURCESHORT:BoT<TAB>SOURCEDATE:2024-01
```

### 3.5 Continuation lines

Some newer data spreads one record across several physical lines,
starting each additional line with a TAB:

```
Rusty Spoon<TAB>TYPE:Goods.Kitchen
<TAB>COST:1
<TAB>WT:0.5
```

PCGen accepts this only when a user option is enabled; with the option
off, such lines are misread. `pcgtools` always treats a line that
starts with whitespace as a continuation of the preceding record (a
continuation with no preceding record is reported as an error).
Continuations are preserved as separate physical lines when writing.

### 3.6 Lossless editing model

`pcgtools` keeps each line as its exact list of TAB-separated cells,
plus the line's terminator and encoding flag. Editing replaces,
inserts, or removes individual cells, so the bytes of untouched lines
and cells — including alignment TABs and comments — are preserved.

---

## 4. PCC campaign files

A PCC file describes one *campaign* — usually one published book, or a
bundle of books. It has two jobs: carry metadata about the book, and
list the data files the book contributes.

### 4.1 Line syntax

Each non-comment line holds exactly one tag. Lines are **not** split on
TAB; leading and trailing whitespace (including stray TABs) is trimmed.
The tag ends at the first colon:

```
pcc-line = [ "!" ] tag ":" value ;
```

A leading `!` negates prerequisite tags (`!PRECAMPAIGN:...`). Tags may
repeat; how repeats combine depends on the tag (lists accumulate,
single values are replaced).

### 4.2 Metadata tags

| Tag | Value |
|---|---|
| `CAMPAIGN` | display name; also the campaign's key unless `KEY` is given |
| `KEY` | stable identity used by `PRECAMPAIGN` and character files |
| `GAMEMODE` | `|`-separated list of game modes the campaign supports |
| `BOOKTYPE` | `|`-separated list (e.g. core rules, supplement, adventure) |
| `RANK` | integer load priority; default 9 |
| `TYPE` | `.`-separated classification used by the UI |
| `SETTING`, `GENRE`, `STATUS` | free text for filtering in the UI |
| `PUBNAMELONG`, `PUBNAMESHORT`, `PUBNAMEWEB` | publisher |
| `SOURCELONG`, `SOURCESHORT`, `SOURCEWEB`, `SOURCEDATE` | the book itself; `SOURCEDATE` is `YYYY-MM` or `YYYY-MM-DD` |
| `DESC`, `INFOTEXT` | descriptive text (may repeat) |
| `COPYRIGHT`, `LICENSE` | license text, one line per tag |
| `ISOGL`, `ISLICENSED`, `ISMATURE` | `YES`/`NO` flags |
| `SHOWINMENU` | `YES` to list the campaign in quick-load menus |
| `COVER`, `LOGO`, `URL`, `HELP` | images and links (`URL` is `KIND|address|text`) |
| `HIDETYPE` | `KIND|Type|Type...`: types hidden from the UI |
| `COMPANIONLIST` | `Name|Race,Race...`: allowed companion types |
| `FORWARDREF` | `KIND|Key,Key`: objects to be defined later, to silence missing-reference errors |
| `ALLOWDUPES` | object kinds in which duplicate keys are permitted |
| `OPTION` | `name|value` preference applied while the campaign is loaded |
| `PRECAMPAIGN` | campaign-level prerequisite (4.6) |

Any global object tag (such as `BONUS` or `DEFINE`) may also appear; it
applies to the campaign as an object.

### 4.3 File reference tags

Each of the following tags names one data file of the corresponding
kind (see chapter 5). They may appear any number of times.

`DATACONTROL`, `DATATABLE`, `VARIABLE`, `DYNAMIC`, `GLOBALMODIFIER`,
`ABILITYCATEGORY`, `SIZE`, `STAT`, `SAVE`, `ALIGNMENT`, `WEAPONPROF`,
`ARMORPROF`, `SHIELDPROF`, `SKILL`, `LANGUAGE`, `FEAT`, `ABILITY`,
`RACE`, `DOMAIN`, `SPELL`, `DEITY`, `CLASS`, `TEMPLATE`, `EQUIPMOD`,
`EQUIPMENT`, `COMPANIONMOD`, `KIT`, `BIOSET`.

In addition:

| Tag | Meaning |
|---|---|
| `PCC` | include another campaign file (4.5) |
| `LSTEXCLUDE` | `|`-separated files that must not be loaded, by any campaign |

The value of a file reference is a path, optionally followed by
`|`-separated suffixes:

```
fileref  = path { "|" suffix } ;
suffix   = "(INCLUDE:" items ")" | "(EXCLUDE:" items ")"
         | [ "!" ] "PRE" name ":" body ;
items    = item { "|" item } ;
item     = key | "CATEGORY=" category { "," key } ;
```

Separators inside parentheses do not split the value, so keys that
themselves contain `|` or parentheses survive:
`equip.lst|(INCLUDE:Sword (Bastard)|Lantern)`.

* `INCLUDE` loads only the named objects from the file; `EXCLUDE` loads
  all but the named objects. Ability files group keys by category.
  Only one of the two may be used per reference.
* A prerequisite suffix (in practice `PRECAMPAIGN`) makes the reference
  conditional on which other campaigns are selected.
* The kinds `DATACONTROL`, `DATATABLE`, `VARIABLE`, `DYNAMIC`,
  `GLOBALMODIFIER`, `COMPANIONMOD` and `BIOSET` accept no suffixes.

### 4.4 Path resolution

Paths use `/` as separator; `\` is accepted and converted. The first
character selects the base directory:

| Prefix | Resolved against |
|---|---|
| `@` | the data directory |
| `&` | the vendor data directory |
| `$` | the homebrew data directory |
| `*` | the first of homebrew, vendor, data in which the file exists |
| `scheme:` | an absolute URI (`file:`, or `http:` when network loading is enabled) |
| `data/` or `/data/` | legacy form: the data directory |
| anything else | the directory containing the PCC file (`..` allowed) |

A single `/` after a prefix character is optional (`@/x.lst` and
`@x.lst` are the same). File names are case-sensitive on most
filesystems, but data authored on case-insensitive filesystems sometimes gets case
wrong; `pcgtools` falls back to a case-insensitive match.

### 4.5 Campaign includes

`PCC:` names another campaign file. Selecting a campaign implicitly
selects everything it includes, transitively; each campaign is loaded
once even if included several times. Every file list of an included
campaign is appended to the including campaign's lists.

### 4.6 PRECAMPAIGN

```
PRECAMPAIGN:count,item{,item}
```

passes when at least `count` items match. Items:

| Item | Matches when |
|---|---|
| `Key` | a campaign with that key is directly selected |
| `INCLUDES=Key` | that campaign is selected or included by a selected campaign |
| `BOOKTYPE=Type` | a directly selected campaign has that book type |
| `INCLUDESBOOKTYPE=Type` | a selected or included campaign has that book type |
| `[item]` | the item does *not* match |

`!PRECAMPAIGN` inverts the result. On a file reference, a failing
prerequisite drops that file. Other prerequisite kinds on file
references cannot be evaluated without a character and are ignored.

### 4.7 Selection and load order

Given the set of campaigns the user selected:

1. Selected campaigns are ordered by `RANK`, highest value first
   (ties keep selection order).
2. For each selected campaign, its own file references and then those
   of its included campaigns are collected per kind, skipping
   references whose prerequisites fail and exact duplicates.
3. Files named by any `LSTEXCLUDE` are removed.
4. Files are loaded kind by kind, in this fixed order:

   `DATACONTROL`, `DATATABLE`, `VARIABLE`, `DYNAMIC`, `GLOBALMODIFIER`,
   `ABILITYCATEGORY`, `SIZE`, `STAT`, `SAVE`, `ALIGNMENT`, `WEAPONPROF`,
   `ARMORPROF`, `SHIELDPROF`, `SKILL`, `LANGUAGE`, `FEAT`, `ABILITY`,
   `RACE`, `DOMAIN`, `SPELL`, `DEITY`, `CLASS`, `TEMPLATE`, `EQUIPMOD`,
   `EQUIPMENT`, `COMPANIONMOD`, `KIT`, `BIOSET`.

   Within a kind, files load in the collected order.

---

## 5. Per-kind structure and loading

### 5.1 Simple object kinds

Most kinds hold exactly one object per record, named by the head:

`SIZE`, `STAT`, `SAVE`, `ALIGNMENT`, `WEAPONPROF`, `ARMORPROF`,
`SHIELDPROF`, `SKILL`, `LANGUAGE`, `FEAT`, `ABILITY`, `RACE`, `DOMAIN`,
`SPELL`, `DEITY`, `TEMPLATE`, `EQUIPMOD`, `EQUIPMENT`.

Identity rules:

* The key is the name unless a `KEY:` token overrides it.
* `SIZE`, `STAT` and `ALIGNMENT` objects are keyed by their `ABB:`
  abbreviation (`STR`, `M`, `LG`), and data refers to them that way.
* `ABILITY` objects live in an ability category, given by a
  `CATEGORY:` token; keys need only be unique within a category.
  Modification heads must name the category (`CATEGORY=FEAT|Key.MOD`).
* Key comparison is case-insensitive.
* Defining a key that already exists is an error; the later definition
  is ignored, except as described in 5.5. (Listing the same file twice,
  for example with different INCLUDE filters, is not a duplicate.)

### 5.2 Structured kinds

**CLASS.** A class file mixes several line types:

| Head | Meaning |
|---|---|
| `CLASS:Name` | the class itself; further `CLASS:Name` lines in the same file add more tokens to it |
| `N` (an integer) | level line: tokens that apply when the class reaches level N |
| `N:REPEATLEVEL:step[|SKIP=s][|MAX=m]` | level line repeated every *step* levels from N |
| `SUBCLASS:Name` | an alternative variant of the current class (e.g. a specialization) |
| `SUBCLASSLEVEL:N` | level line for the most recent subclass |
| `SUBSTITUTIONCLASS:Name` | a substitution variant of the current class |
| `SUBSTITUTIONLEVEL:N` | level line for the most recent substitution class |
| `CLASS:Name.MOD` | modify a class; following level and subclass lines belong to the modification until the next `CLASS:` line |

**KIT.** A kit starts with `STARTPACK:Name` and owns the lines that
follow it, each of the form `TAG:value<TAB>tokens`, where `TAG` is one
of `ABILITY`, `AGE`, `ALIGN`, `CLASS`, `DEITY`, `FEAT`, `FUNDS`,
`GEAR`, `GENDER`, `KIT`, `LANGBONUS`, `LEVELABILITY`, `NAME`, `PROF`,
`RACE`, `SELECT`, `SKILL`, `SPELLS`, `STAT`, `TABLE`, `TEMPLATE`.
A `REGION:x` line applies to every kit that follows it in the file.

**BIOSET.** `AGESET:n|Name` lines declare age categories; the
`RACENAME:Race` lines that follow give per-race age, height, weight
and appearance data for that age set. Heads repeat across files.

**COMPANIONMOD.** Each line is a modifier applied to companions:
`FOLLOWER:Type=Level` (for companions of a master with that level
variable) or `MASTERBONUSRACE:Race`. Heads repeat by design.

**ABILITYCATEGORY.** `ABILITYCATEGORY:Name` lines define the categories
(pools) abilities belong to.

**DATACONTROL.** One declaration per line:

| Head | Declares |
|---|---|
| `FACTDEF:KIND|Name` | a named single-valued fact on objects of a kind |
| `FACTSETDEF:KIND|Name` | a named multi-valued fact |
| `DEFAULTVARIABLEVALUE:Type|value` | the default for a variable format |
| `FUNCTION:Name` | a data-defined formula function |
| `DYNAMICSCOPE:Name` | a scope for dynamic objects |

**VARIABLE.** One declaration per line: `GLOBAL:[Format=]Name`,
`LOCAL:Scope|[Format=]Name`, or `CHANNEL:Scope|[Format=]Name`.

**DYNAMIC.** One object per line, `Scope:Name<TAB>tokens`, defining
objects of a dynamic scope declared in a data control file.

**GLOBALMODIFIER.** One `MODIFY:` modifier per line, applied globally.

**DATATABLE.** Comma-separated values, not TAB-separated:

```
STARTTABLE:Name
Column1,Column2,...
FORMAT1,FORMAT2,...
value,value,...
...
ENDTABLE:
```

Fields may be double-quoted (with `""` for a literal quote). Rows that
are entirely empty, and lines starting with `#` or `"#`, are ignored.
Cells are trimmed. Inside a table the first row names the columns, the
second gives each column's format, and every further row is data;
the first cell of `STARTTABLE:` and `ENDTABLE` lines is all that is
read. A file may hold several tables. A `STARTTABLE` inside an open
table, an `ENDTABLE` with none open, data outside a table and a
missing `ENDTABLE` are errors (an unterminated table is still kept).
Tables are read by the `lookup` function (6.6).

### 5.3 Modification order

Within one kind, all files are read first. Then deferred operations
from all those files run in three passes:

1. every `.COPY` (copy, then apply the line's tokens to the copy),
2. every `.MOD`,
3. every `.FORGET`.

Each pass processes operations in load order. Consequences:

* A `.MOD` may appear before the definition it modifies, even in an
  earlier file.
* A `.COPY` sees the original object, not any `.MOD` applied to it.
* Operations never cross kinds: a spell file cannot modify a class.
* A `.MOD` whose target does not exist is an error — unless the target
  was dropped by an INCLUDE/EXCLUDE filter.

When a `.MOD` appends a token, the token is added after the object's
existing tokens. Whether that replaces or adds to an earlier value
depends on the tag's cardinality. The same rules apply to tokens of
repeated `CLASS:Name` lines (5.2) and to the tokens a `.COPY` line
applies to its copy.

**Single-valued tags.** A new value replaces every earlier value of
the tag:

`ABB`, `ACCHECK`, `ALTCRITMULT`, `ALTCRITRANGE`, `ALTDAMAGE`,
`BASEITEM`, `BASEQTY`, `CHOOSE`, `CONTAINS`, `COST`, `CR`, `CRITMULT`,
`CRITRANGE`, `DAMAGE`, `EDR`, `EXCLUSIVE`, `FACE`, `HANDS`, `HD`,
`KEYSTAT`, `LEGS`, `LEVELADJUSTMENT`, `MAXDEX`, `MAXLEVEL`, `MEMORIZE`,
`MULT`, `OUTPUTNAME`, `PROFICIENCY`, `RACETYPE`, `RANGE`, `RATEOFFIRE`,
`REACH`, `REACHMULT`, `SELECT`, `SIZE`, `SLOTS`, `SORTKEY`,
`SOURCEDATE`, `SOURCELINK`, `SOURCELONG`, `SOURCEPAGE`, `SOURCESHORT`,
`SOURCEWEB`, `SPELLFAILURE`, `SPELLSTAT`, `STACK`, `STARTSKILLPTS`,
`STATMOD`, `USEUNTRAINED`, `VISIBLE`, `WIELD`, `WT`,
`XTRASKILLPTSPERLVL`.

**List-valued tags.** Every other tag accumulates: each token is kept
in order after the earlier ones.

**Clear forms.** A token whose value has one of these forms removes
earlier tokens before (or instead of) being added:

| Token | Effect |
|---|---|
| `TAG:.CLEAR` | remove every earlier `TAG` token; nothing is added |
| `TAG:.CLEARALL` | the same as `.CLEAR` |
| `TAG:.CLEAR.x` | remove earlier `TAG` tokens matching *x*; nothing is added |
| `TAG:.CLEAR|value` | remove every earlier `TAG` token, then add `TAG:value` |
| `PRE:.CLEAR` | remove every earlier prerequisite token (`PRExxx` and `!PRExxx`) |

For `.CLEAR.x`, an earlier value matches when it equals *x*, when one
of its `|`-separated parts equals *x* (both ignoring case), or when *x*
read as a regular expression matches somewhere in it. So after
`CSKILL:Climb|Swim`, the token `CSKILL:.CLEAR.Swim` removes the whole
`Climb|Swim` token, not only `Swim`.

A `KEY` token changes the object's key; an `ABB` token does so for
sizes, stats and alignments; a `CATEGORY` token changes an ability's
category.

### 5.4 Source information

Each object records the source tags from the file's header line
(`SOURCELONG`, `SOURCESHORT`, `SOURCEWEB`, `SOURCEDATE`) unless the
object's own tokens supply them. `SOURCEPAGE` is normally given per
object.

### 5.5 Duplicate keys and load-plan options

Several campaign tags (4.2) adjust loading for the whole load plan.
They are collected once from every campaign involved, selected or
included.

| Tag | Effect |
|---|---|
| `RANK` | orders selected campaigns, highest first (4.7) |
| `ALLOWDUPES:KIND` | objects of that kind may share a key: every definition is kept, and lookup by key finds the first loaded. One kind per tag. |
| `SOURCEDATE` | the campaign's publication date, used to settle duplicates (below) |
| `FORWARDREF:KIND|Key,Key` | declares objects the data refers to without defining them; a character's reference to one is not reported as missing |

When a kind does not allow duplicates and a second definition of a key
is loaded, the two are compared by *source date*: the object's own
`SOURCEDATE`, else the one in its file's source header (3.4), else
the `SOURCEDATE` of the campaign that listed the file. If both
definitions have a date and the newer one's is later (compared as
text, so `YYYY-MM` and `YYYY-MM-DD` order correctly), the new
definition replaces the old one in place and an informational note is
reported. Otherwise the new definition is ignored with a warning.
Repeated `CLASS:Name` lines within one file are not duplicates (5.2).
Lines of `DATACONTROL`, `VARIABLE`, `GLOBALMODIFIER`, `COMPANIONMOD`
and `BIOSET` files are declarations whose heads repeat by design; they
are never treated as duplicates.

---

## 6. Token value syntax

Token values have their own small languages, built from a few shared
separators.

### 6.1 Separators and grouping

| Separator | Typical role |
|---|---|
| `|` | separates the fields of a token (`ABILITY:Cat|Nature|Key`) |
| `,` | separates list items within a field (`TYPE=A,TYPE=B`) |
| `.` | separates type names (`TYPE:Weapon.Melee.Martial`) |
| `=` | attaches a qualifier or value (`CL=Fighter`, `STR=13`) |
| `[ ]` | groups a nested token, as in `PREMULT` |
| `( )` | groups text that may contain separators (`Sword (Long)`), and formula sub-expressions |

Separators inside `[]` or `()` never split the enclosing value. A
value may not begin or end with a separator, nor contain two in a row
(`a||b`); PCGen rejects such tokens.

### 6.2 Prerequisites

Any object token list may include prerequisites. An object (or a
sub-part such as a BONUS) applies only if all its prerequisites pass.

```
prereq   = [ "!" ] "PRE" kind ":" body ;
body     = arg { "," arg } ;
pre-mult = [ "!" ] "PREMULT:" count "," "[" prereq "]" { "," "[" prereq "]" } ;
```

* `!` negates the result.
* Most kinds are *list* prerequisites: `PREkind:count,item,item...`
  passes when at least *count* items are satisfied. Items may be
  `TYPE=x` (anything of that type), `ANY`, or `key=value` comparisons
  such as `CLASS:1,Fighter=4` (Fighter at level 4 or more).
* `PREMULT` combines other prerequisites: it passes when at least
  *count* of the bracketed prerequisites pass. Nesting is allowed.
* Comparison kinds use a suffix: `PREVARGT`, `PREVARGTEQ`, `PREVAREQ`,
  `PREVARLT`, `PREVARLTEQ`, `PREVARNEQ` take pairs
  `formula,formula{,formula,formula}`. Similar families exist for
  sizes, hands, legs, and others.
* Some kinds take `MIN=`/`MAX=` options instead of a count
  (`PRELEVEL:MIN=5`).
* `PRETEXT:...` is never checked; it documents a requirement that the
  user must judge.

Common kinds: `ABILITY`, `ALIGN`, `ARMORPROF`, `BASEATTACK` (as `PREBAB`),
`CHECK`, `CLASS`, `CSKILL`, `DEITY`, `DOMAIN`, `EQUIP`, `FACT`,
`GENDER`, `HD`, `ITEM`, `LANG`, `LEVEL`, `MOVE`, `PCLEVEL`, `RACE`,
`SIZE`, `SKILL`, `SPELL`, `SPELLCAST`, `STAT`, `TEMPLATE`, `TEXT`,
`TYPE`, `VAR`, `VISION`, `WEAPONPROF`.

### 6.3 BONUS

```
BONUS:category|target{,target}|formula{|option}
option = "TYPE=" name [ ".STACK" | ".REPLACE" ] | prereq ;
```

A bonus adds the value of *formula* to each *target* within
*category*, for example `BONUS:COMBAT|AC|2|TYPE=Armor` or
`BONUS:SKILL|Climb,Swim|4|PRESIZEGTEQ:L`. Common categories:
`ABILITYPOOL`, `CASTERLEVEL`, `CHECKS`/`SAVE`, `COMBAT` (`AC`, `TOHIT`,
`DAMAGE`, `BASEAB`, `INITIATIVE`, ...), `HP`, `MOVEADD`, `MOVEMULT`,
`SIZEMOD`, `SKILL`, `SKILLPOINTS`, `SKILLRANK`, `SPELLCAST`,
`SPELLKNOWN`, `STAT`, `VAR`, `WEAPON`, `WEAPONPROF`.

Typed bonuses do not stack: of several bonuses with the same type to
the same target, only the largest applies, unless the game mode
declares that type stacking. `TYPE=x.STACK` forces stacking for one
bonus; `.REPLACE` makes it replace other bonuses of that type.
Untyped bonuses always stack.

### 6.4 Sub-token tags

Several tags select a sub-token by the first `|`-field of their value:

| Tag | Example | Meaning |
|---|---|---|
| `ADD` | `ADD:FEAT|1|TYPE=Combat` | grant choices when the object is gained |
| `AUTO` | `AUTO:LANG|Common` | grant something automatically |
| `CHOOSE` | `CHOOSE:SKILL|TYPE=Craft` | ask the user to choose; the choice is referred to as `%LIST` or `%CHOICE` |
| `REMOVE` | `REMOVE:FEAT|1|Dodge` | remove choices |
| `TEMPBONUS` | `TEMPBONUS:PC|BONUS:...` | a bonus the user can toggle |

`CHOOSE` may be prefixed with `NUMCHOICES=n|` to limit how many
choices may be made.

`CHOOSE`, `ADD` and `AUTO` values share one structure: a sub-token
naming what is chosen or granted, then `|`-separated selection items.

```
value     = [ "NUMCHOICES=" n "|" ] kind { "|" item }
          | kind "(" text ")" count ;          (* legacy: no "|" at all *)
item      = prereq | selector { "," selector } | name ;
selector  = primary [ "[" item { "|" item } "]" ] ;
primary   = "!" primary
          | "TYPE=" types | "TYPE." types
          | KEYWORD "=" text
          | KEYWORD
          | name ;
KEYWORD   = upper-case letters, digits and "_", at least one letter ;
```

| Item | Example | Selects |
|---|---|---|
| name | `Dodge`, `Charm, Psionic` | the object with that key |
| type | `TYPE=Combat`, `TYPE.Craft` | objects having every `.`-separated type |
| keyword | `ALL`, `ANY`, `PC`, `QUALIFIED`, `NOCHOICE`, `DEITYWEAPON` | a set named by the keyword |
| pair | `CATEGORY=FEAT`, `CLASS=Wizard`, `TITLE=Pick one` | a qualifier or option |
| negation | `!TYPE=Exotic` | everything the item does not select |
| narrowed | `!PC[TYPE=Weapon Focus]` | the head item restricted by the bracketed items |
| conjunction | `!PC,QUALIFIED` | what every comma-joined part selects |
| prerequisite | `PRERACE:1,Human`, `!PREVARGT:X,1` | a condition on the whole token |

Rules:

* `,` joins selectors only when every part is a type, keyword, pair,
  negation or narrowed item; otherwise the whole part is one name, so
  names containing commas survive.
* `|` separates independent items; `,` within one part requires all
  of its parts.
* A pair needs an upper-case key: `a=b` is a name.
* A leading number is a count (`ADD:CLASSSKILLS|2|...`).
* `TITLE=` (and a `NUMCHOICES=` written as an item) is an option, not
  part of the selection.
* The legacy form `ADD:FEAT(TYPE=Occupation)1` keeps its parenthesized
  selection and count as written.
* Values print back exactly as written.

`pcgtools` enumerates a `CHOOSE` only when its items are names, `ALL`,
`ANY` and type items over a kind it can list (skills, languages,
proficiencies, classes, stats, domains, deities, races, templates,
spells, equipment, alignments, sizes, saves); keywords such as `PC`,
narrowed and combined items need the character's state and are
treated as offering anything.

### 6.5 Formulas

Formulas appear in `BONUS`, `DEFINE`, `PREVAR`, and many other places.

```
expr    = or ;
or      = and { "||" and } ;
and     = eq { "&&" eq } ;
eq      = rel { ( "==" | "!=" ) rel } ;
rel     = add { ( "<" | ">" | "<=" | ">=" ) add } ;
add     = mul { ( "+" | "-" ) mul } ;
mul     = unary { ( "*" | "/" | "%" ) unary } ;
unary   = ( "-" | "+" | "!" ) unary | power ;
power   = postfix [ "^" unary ] ;
postfix = atom { "." name | ( "MAX" | "MIN" ) atom } ;
atom    = number | string | term [ "(" [ expr { "," expr } ] ")" ]
        | "(" expr { "," expr } ")" ;
term    = name { "=" text | "[" text "]" } ;
```

* Comparisons yield 1 or 0, so `1+(LVL>=5)+(LVL>=10)` counts thresholds.
* Names are variables: values defined with `DEFINE:Name|initial` and
  raised by `BONUS:VAR|Name|formula`, or built-in terms such as stat
  abbreviations (`STR` is the modifier), `HD`, `TL` (total level),
  `CL` (level in the owning class), `BAB`.
* Legacy terms carry their argument in the name: `CL=Fighter`,
  `COUNT[FEATTYPE=Metamagic]`, `SKILL.Ride.RANK`.
* Built-in functions include `floor`, `ceil`, `round`, `min`, `max`,
  `abs`, `if(cond,then,else)`, `var("name")`, `classlevel("Class")`,
  `count("ABILITIES", filter...)`, and `skillinfo("RANK","Skill")`.
* Very old data uses postfix `.TRUNC`/`.INTVAL` and infix `MAX`/`MIN`
  (`(x)MAX1` meaning the larger of x and 1); `pcgtools` accepts them.
* Strings are double-quoted.

Newer data also writes formulas in `MODIFY` tokens and data control
files. They use the same grammar but a typed evaluation (6.6).

### 6.6 Typed formulas

Formulas in `MODIFY`, `MODIFYOTHER`, global modifier lines and
data-defined `FUNCTION`s evaluate to typed values. Each declared
variable has a *format*:

| Format | Values | Default | Literal |
|---|---|---|---|
| `NUMBER` | integer or decimal | `0` | `3`, `2.5` |
| `BOOLEAN` | true/false | false | `true`/`yes`, `false`/`no` |
| `STRING` | text | empty | as written |
| `ORDEREDPAIR` | two numbers | `0,0` | `5,5` |
| `DICE` | a dice expression, kept as text | `0` | `1d6` |
| `HANDED` | text | `Right` | as written |
| any other name (`STAT`, `ALIGNMENT`, a dynamic scope, ...) | a reference to an object of that kind | none | its key |
| `ARRAY[f]` | a list of *f* | empty | `a,b,c` |
| `OPTIONAL[f]` | an *f* or nothing | nothing | empty or an *f* |

Format names are case-insensitive and nest (`ARRAY[ARRAY[STRING]]`).
The defaults may be overridden per format (9.5).

Evaluation:

* Integer arithmetic stays integral when exact (`6/3` is `2`, `7/2` is
  `3.5`); division or remainder by zero gives 0; `^` is a power.
* `+` of two strings concatenates.
* Comparisons and `&&`, `||`, `!` yield booleans; `==` compares
  numbers numerically and anything else as text ignoring case.
  Booleans count as 1/0 in arithmetic; strings by their numeric
  reading.
* A bare name is a declared variable, looked up from the current scope
  outward (9.5). An undeclared name is 0 and is recorded as unknown.
* A quoted string where an object format is expected is a reference
  to the object with that key.

Built-in functions (names are case-insensitive):

| Function | Result |
|---|---|
| `abs(x)`, `floor(x)`, `ceil(x)` | as usual; `floor`/`ceil` give integers |
| `round(x)` | nearest integer, halves upward (`round(-2.5)` is `-2`) |
| `max(a,b,...)`, `min(a,b,...)` | largest / smallest argument |
| `if(c,t,e)` | *t* when *c* is true, else *e* |
| `value()` | inside a modifier, the variable's value before this modifier |
| `input("Name")` | the value of channel *Name* (9.5) |
| `get("FORMAT","key")` | a reference to an object |
| `key(x)` | the key of an object reference, as text |
| `this()` | the object whose scope instance is evaluating |
| `getOther("SCOPE",key,expr)` | *expr* evaluated in another instance's scope |
| `getFact("KIND",obj,"Fact")` | the object's `FACT:Fact|value` value, as text |
| `lookup("Table",key,"Column")` | a table cell (below) |
| `length(a)`, `isEmpty(a)`, `slice(a,start[,end])` | array length, emptiness, sub-array |
| `isPresent(o)`, `getOptional(o)` | whether an optional holds a value; that value |
| `arg(n)` | inside a data-defined function, its *n*-th argument (from 0) |

`listAll` and `group` are accepted but yield an empty array. Any other
name is a data-defined function, declared in a data control file as
`FUNCTION:Name<TAB>VALUE:formula`; its body sees the caller's scope
and `arg(n)`. An unknown function yields 0 and is recorded.

**Table lookup.** `lookup(table, key, column)` finds the `DATATABLE`
table (5.2) whose name equals *table*, the column whose name equals
*column*, and the first data row whose first cell equals *key* —
ignoring case, or numerically when both read as numbers (`2` matches
`2.0`). The cell is returned as a number when it reads as one, else as
text. A missing table, column or row gives 0. The format row is not
consulted.

```
STARTTABLE:Carry
Strength,Light,Heavy
NUMBER,NUMBER,NUMBER
10,33,100
11,38,115
ENDTABLE:
```

With this table, `lookup("Carry",11,"Heavy")` is 115.

---

## 7. Game mode files

A *game mode* is a rules system (a particular edition of a game, or a
house variant). Each lives in its own directory under
`system/gameModes/`, and holds constants and UI settings that data
files rely on. A directory named `default` supplies any file a mode
does not provide itself.

### 7.1 Line syntax

All game mode files share one line shape:

```
line = head { TAB { TAB } token } ;
head = TAG ":" value ;
```

The head tag says what the line declares; the remaining tokens are its
attributes. Comments and blank lines follow the usual rules (2.3).
Unlike LST files, leading whitespace is ignored.

### 7.2 Identity

| Tag (in `miscinfo.lst`) | Meaning |
|---|---|
| `GAMEMODEKEY` | the mode's identity; defaults to the directory name |
| `ALLOWEDMODES` | `|`-separated names that campaign `GAMEMODE` tags may use for this mode |
| `DISPLAYORDER` | ordering in menus |

A campaign is usable under a mode when one of its `GAMEMODE` values
equals the mode's key or one of its allowed names (case-insensitive).

### 7.3 Files

| File | Purpose | Typical heads |
|---|---|---|
| `miscinfo.lst` | assorted constants and UI setup | `ACNAME`, `BONUSSTACKS`, `BABMAXATT`, `BABATTCYC`, `BABMINVAL`, `SKILLMULTIPLIER`, `SPELLBASEDC`, `CURRENCYUNITABBREV`, `DEFAULTUNITSET`, `UNITSET`, `TAB`, `ROLLMETHOD`, `WIELDCATEGORY`, `ABILITYCATEGORY`, `CLASSTYPE`, `BASEDICE`, `ACTYPE`, `WEAPONTYPE`, ... |
| `statsandchecks.lst` | bonus spells by stat score (older modes also declare stats and saves here) | `BONUSSPELLLEVEL` |
| `level.lst` | experience tables; a `LEVEL:LEVEL` row gives a formula for higher levels; rows before any `XPTABLE` form a table named `Default`; the first table is the default | `XPTABLE`, `LEVEL` with `MINXP`, `CSKILLMAX`, `CCSKILLMAX` |
| `load.lst` | carrying capacity | `SIZEMULT`, `LOADSCORE`, `ENCUMBRANCE` |
| `sizeAdjustment.lst` | size categories (newer modes define sizes in data instead) | |
| `rules.lst` | optional rules the user can toggle | `NAME` with `VAR`, `DEFAULT`, `EXCLUDE`, `DESC` |
| `equipmentslots.lst` | where equipment can be worn | `NUMSLOTS`, `EQSLOT` with `CONTAINS`, `NUMBER` |
| `codeControl.lst` | switches selecting engine behavior (7.4) | `ALIGNMENTFEATURE`, `DOMAINFEATURE`, ... |
| `migration.lst` | renamed object keys, for loading old characters | `RACE:Old` with `NEWKEY`, `MAXVER` |
| `pointbuymethods_system.lst` | point-buy stat generation | `METHOD`, `STAT` |
| `bio/biosettings.lst` | default age/height/weight tables (as BIOSET, 5.2) | `AGESET`, `RACENAME` |
| `bio/traits.lst` | random personality traits | |
| `paperInfo.lst`, `equipIcons.lst` | output page sizes, UI icons | |

`BONUSSTACKS` lists, separated by `.`, the bonus types that stack with
themselves (see 6.3); the line may repeat.

### 7.4 Code controls

Each `codeControl.lst` line is `NAME:value`. Some controls switch a
feature on or off; others name a declared variable (9.5) that takes
over a core value. `pcgtools` reads these:

| Control | Value | Effect |
|---|---|---|
| `ALIGNMENTFEATURE` | `YES`/`NO` | the character's alignment is shown only when `YES` |
| `FACE` | variable name | declares a global `ORDEREDPAIR` variable of that name; its value, when set, is the character's face (width, depth). Without the control, a global variable `Face` the data declares serves instead |
| `PCREACH` | variable name | the character's reach is that global variable's value instead of the race/template value plus `COMBAT.REACH` bonuses |
| `STATMODSAVE` | variable name | the stat part of a save is that variable in the save's scope instead of the bonuses on the save object |
| `STATINPUT` | channel name | each stat's base score is supplied as that channel in the stat's scope |

Control names are case-insensitive. Other controls (such as
`DOMAINFEATURE`) are read but have no effect.

---

## 8. PCG character files

A PCG file saves one character. It records the choices that define the
character — which campaigns were loaded, ability scores, race, each
class level taken, skill ranks bought, abilities chosen, equipment and
how it is worn, biography — plus some cached values. Derived values
(attack bonus, saves, armor class, skill totals) are *not* stored;
they are recomputed from the data when the character is loaded.

### 8.1 Line syntax

The first line identifies the format version, `PCGVERSION:2.0`. (An
older, colon-separated format beginning with `CAMPAIGNS:` exists in a
few very old files; it is not described here.) Lines starting with `#`
are comments; PCGen writes section headings such as `# Character Bio`.

Every other line is a list of elements:

```
line  = elem { "|" elem } [ "|" ] ;
elem  = tag ":" ( text | "[" [ line ] "]" ) ;
```

* The first element names what the line describes:
  `SKILL:Ride|OUTPUTORDER:3|CLASSBOUGHT:[CLASS:Fighter|RANKS:4.0|COST:1|CLASSSKILL:Y]`.
* A value in brackets directly after the colon is a nested element
  list. A nested list may be followed directly by the next element
  without a `|` (`...]CLASSBOUGHT:[...]`).
* Brackets that do not directly follow a colon are ordinary text
  (`EXPRESSION:roll(4,6,[2,3,4])`).
* A trailing `|` is permitted.

Some lines hold one free-text value occupying the whole rest of the
line and are never split: the biography and description fields
(`CHARACTERNAME`, `CHARACTERBIO`, `CITY`, `CATCHPHRASE`, ...), simple
numbers (`AGE`, `HEIGHT`, `WEIGHT`, `EXPERIENCE`, `MONEY`,
`POOLPOINTS`), and a few settings (`GAMEMODE`, `TABNAME`,
`SPELLLIST`, ...).

### 8.2 Escapes

Text values replace structural characters with named entities:

| Character | Entity |
|---|---|
| `&` | `&amp;` |
| `|` | `&pipe;` |
| `:` | `&colon;` |
| `[` | `&lbracket;` |
| `]` | `&rbracket;` |
| newline | `&nl;` |
| carriage return | `&cr;` |
| form feed | `&lf;` |

Free-text whole-line fields escape only `&`, `|` and line breaks.
Unknown `&...;` sequences are left as they are.

### 8.3 Content

| Line | Meaning |
|---|---|
| `CAMPAIGN:key{|CAMPAIGN:key}` | campaigns to load, by campaign key |
| `VERSION:x.y.z` | program version that saved the file (drives key migration, 8.4) |
| `GAMEMODE:name` | game mode |
| `CHARACTERNAME`, `PLAYERNAME`, `AGE`, `GENDER`, `HEIGHT`, `WEIGHT`, `CITY`, ... | biography |
| `STAT:abb|SCORE:n` | base ability score |
| `ALIGN:abb` | alignment |
| `RACE:key` | race |
| `CLASS:key|LEVEL:n|...` | levels held in a class |
| `CLASSABILITIESLEVEL:Class=n|HITPOINTS:h|SKILLSGAINED:s|...` | one level gained, in the order gained, with its hit point roll |
| `EXPERIENCE:n` | experience points |
| `TEMPLATESAPPLIED:[NAME:key]` | applied template |
| `SKILL:key|OUTPUTORDER:n{|CLASSBOUGHT:[CLASS:c|RANKS:r|COST:k|CLASSSKILL:Y/N]}` | ranks bought per class |
| `LANGUAGE:key{|LANGUAGE:key}` | known languages |
| `ABILITY:pool|TYPE:nature|CATEGORY:cat|KEY:key[|APPLIEDTO:a,b]...` | an ability; *nature* is `NORMAL` (chosen), `AUTOMATIC` or `VIRTUAL` |
| `FEAT:key|...` | older form of a chosen feat |
| `USERPOOL:pool|POOLPOINTS:n` | user adjustments to ability pools |
| `WEAPONPROF:[WEAPON:key{|WEAPON:key}]` | weapon proficiencies |
| `MONEY:n` | currency |
| `EQUIPNAME:name|OUTPUTORDER:n|COST:c|WT:w|QUANTITY:q[|CUSTOMIZATION:[BASEITEM:key|DATA:...]]` | an owned item; customized items name their base item |
| `EQUIPSET:location|ID:a.b.c|VALUE:item|QUANTITY:q` | where an item is placed; IDs form a tree rooted at a set such as `0.1` |
| `CALCEQUIPSET:id` | the equipment set used for calculations |
| `DEITY:key|...`, `DOMAIN:key|...` | deity and domains |
| `SPELLBOOK`, `SPELLNAME`, `SPELLLIST` | spells known and prepared |
| `NOTE`, `CHARACTERBIO`, `CHARACTERDESC`, ... | notes and descriptions |

Objects are referred to by key; abilities by category and key.

### 8.4 Key migration

Keys in data change over time. Each game mode's `migration.lst` maps
old keys to new ones, and is applied to characters saved by older
program versions:

```
SOURCE:Old Campaign Key<TAB>NEWKEY:New Key<TAB>MAXVER:6.2.0
RACE:Old Race<TAB>NEWKEY:New Race<TAB>MAXVER:6.08.00
ABILITY:Category|Old Key<TAB>NEWKEY:New Key<TAB>MAXVER:6.07.05
EQUIPMENT:Old Item<TAB>NEWKEY:New Item<TAB>MAXVER:6.00.01
```

A rule applies when the character's `VERSION` is no newer than
`MAXVER` — or, failing that, no newer than `MAXDEVVER` within the same
major.minor series — and no older than `MINVER` if given.

---

## 9. Derived values

The formats above define *data*; the meaning of most of it is the
bonus and variable model described here. It is how a character's
attack bonus, saves, armor class, hit points and skill totals follow
from the objects it has. Sections 9.1–9.4 describe the classic model
used by most shipped data. Declared variables changed by `MODIFY` are
solved separately (9.5); they reach the classic results only through
the code controls of 7.4.

### 9.1 Active objects

An object contributes its tokens while it is *active*:

* every stat and save object;
* skills with ranks, and, once the character has a class, every skill
  usable untrained;
* the character's alignment, race, and size category;
* templates: those recorded in the character, plus those granted by
  `TEMPLATE:` tokens of active objects;
* each class, with its class-line tokens and the level lines for every
  level the character has (a `REPEATLEVEL` line applies repeatedly);
* abilities: those recorded in the character, plus those granted by
  `ABILITY:pool|nature|key...` tokens of active objects whose trailing
  prerequisites pass (a pool names an ability category, which may draw
  on another category);
* equipped items and their equipment modifiers — an item is equipped
  when it sits directly in the calculation equipment set at any
  location other than *Carried* or *Not Carried*;
* deity and domains; companion modifiers granting master bonuses.

An object whose own `PRExxx` tokens fail contributes nothing.

### 9.2 Variables

`DEFINE:Name|formula` declares a variable with an initial value; if
several active objects define it, the largest initial value is used.
The variable's value is that initial value plus every
`BONUS:VAR|Name|formula` that applies. Names not defined this way
fall back to built-in terms: stat abbreviations (the modifier),
`<STAT>SCORE`, `TL`/`HD` (total level), `CL` (level in the owning
class), `CL=Class`, `BAB`, `SIZE`, `ENCUMBERANCE`, `MODEQUIPMAXDEX`,
`MOVE[Walk]`, `SKILL.name.RANK`, `COUNT[...]`, and others.

A formula naming something that is none of these (no active object
defines it and it is no built-in term), calling an unknown function,
or failing to parse is read the way PCGen reads it: the text is
upper-cased, the innermost parenthesized parts are worked out first,
each as a formula of its own, `.IF.a.OP.b.THEN.c.ELSE.d.` (OP one of
`LT`, `LTEQ`, `EQ`, `GT`, `GTEQ`) picks an outcome, and what is left is
worked out strictly left to right over `+ - * /`, every piece that is
not a number or a known name counting as zero. So `X+3+(TL-X)/4` with
`X` undefined and `TL` 3 is `(0+3+3)/4`, 1.5. A function name is left
glued to its argument's value, so `max(X,5)` is zero.

`DEFINE` variables and declared variables (9.5) are separate systems
that are not coupled by name, as in PCGen: a formula in one never sees
a variable of the other, even one with the same name. They meet only
through the code controls of 7.4.

One difference from PCGen is known. Gaslight's level table gives
`CSKILLMAX` as a bare variable (`MaxRank`); PCGen's value for it
depends on when it last cached that variable, so its sheet may show a
lower maximum class skill rank than `pcgtools` does.

Because values depend on each other (an ability granted only above a
variable threshold may itself raise variables), evaluation repeats
until no value changes.

### 9.3 Bonus totals

Every `BONUS:category|target|value` files its (integer-truncated)
value under `category.target` with its type. Totals combine as
follows:

| Bonus | Combines |
|---|---|
| untyped | added |
| a type the game mode lists in `BONUSSTACKS` | added |
| any negative value | added |
| `TYPE=x.STACK` | added, on top of everything else of type x |
| `TYPE=x.REPLACE` | added among themselves; then the larger of the plain and replace totals of type x is used |
| any other typed bonus | only the largest of that type counts |

`%LIST` (or `%CHOICE`) in a target stands for each choice made for the
granting object; an ability taken several times applies its other
bonuses once per time.

### 9.4 The usual results

| Value | Computed as |
|---|---|
| ability score | base score + `STAT.abb` bonuses; the modifier comes from the stat's `STATMOD` formula |
| base attack bonus | `COMBAT.BASEAB` (classes add their part with `TYPE=Base.REPLACE`) |
| save | `SAVE.BASE.name` + `SAVE.name` + `SAVE.ALL` |
| hit points | for each level gained: the roll plus the stat modifier given by stats' `HP.BONUS` bonuses (at least 1), plus `HP.CURRENTMAX` |
| armor class of a type | per the game mode's `ACTYPE`: add the `COMBAT.AC` bonuses of the listed types (`TOTAL` means all), subtract the removed ones |
| skill total | ranks + key stat modifier + `SKILL.name`, `SKILL.STAT.abb`, `SKILL.TYPE.t`, `SKILL.LIST`, `SKILL.ALL` + class/cross-class bonuses + armor check penalty for `ACHECK:YES` skills |
| ranks | bought ranks + `SKILLRANK.name`, capped at the experience table's class or cross-class maximum for the character's level |

### 9.5 Declared variables

**Declarations.** `VARIABLE` files declare variables, one per line:

```
decl   = "GLOBAL:" fname | "LOCAL:" scope "|" fname | "CHANNEL:" scope "|" fname ;
fname  = [ format "=" ] name ;       (* format defaults to NUMBER *)
name   = letter { letter | digit | "_" } ;
```

A head with no tag counts as `GLOBAL`. A channel is stored under the
name `CHANNEL*name`. Redeclaring a variable with the same format is
harmless; with a different format, or with a name already declared in
an enclosing scope, it is an error.

**Scopes.** `PC` is the global scope. Local scopes are named
`PC.X`, and a dotted scope draws on its parent (`PC.EQUIPMENT.PART`
on `PC.EQUIPMENT`, which draws on `PC`). A local variable has one
value per *instance*, that is per object of the scope:

| Scope | Instances |
|---|---|
| `PC.STAT`, `PC.SAVE` | every stat and save in the data |
| `PC.SKILL`, `PC.SIZE` | each active skill (9.1); the character's size |
| `PC.EQUIPMENT` | each equipped item, keyed by the item's name |
| `PC.EQUIPMENT.PART` | each active equipment modifier |
| `PC.<category>` | each dynamic object granted by `GRANT:Category|Key{|Key}` on an active object |

Objects of other kinds are in `PC`. A name is resolved from an
instance by looking in its scope, then each enclosing scope.

**Modifiers.**

```
MODIFY:var|op|value{|PRIORITY=n}
MODIFYOTHER:scope|group|var|op|value{|PRIORITY=n}
```

`MODIFY` on an active object changes *var* as seen from that object's
instance. `MODIFYOTHER` changes it on every instance of *scope*
(written `PC.X` or just `X`) selected by *group*: `ALL`, an object key,
or `GROUP=g` (objects whose `GROUP` token lists *g*); a trailing
`[...]` on the group is ignored. Each line of a `GLOBALMODIFIER` file
is a `MODIFY` applied in the global scope. A modifier naming an
undeclared variable is reported and dropped.

| Op | Result |
|---|---|
| `SET` | the value |
| `MULTIPLY`, `DIVIDE`, `ADD` | previous × value, previous ÷ value, previous + value; `ADD` on an array appends items not already present |
| `MAX`, `MIN` | the larger / smaller of previous and value |

**Solving.** A variable's value starts at its format's default and
passes through its modifiers sorted by `PRIORITY` (ascending, default
0), then by operation in the order `SET`, `MULTIPLY`, `DIVIDE`, `ADD`,
`MAX`, `MIN`, then by load order. For `BOOLEAN`, `STRING`,
`ORDEREDPAIR` and `DICE` variables the value is a literal; otherwise
it is a formula (6.6), evaluated in the carrier's instance for
`MODIFY` and in the target instance for `MODIFYOTHER`, with `value()`
the previous value. A variable that depends on itself is reported and
takes its default.

**Defaults.** `DEFAULTVARIABLEVALUE:FORMAT|value` lines in data control
files set a format's starting value. If the data has none, those of
the game mode's `default/compatibilityDataControl.lst` apply. The
default for the `DEITY` format also names the deity of a character
that has none.

**Channels.** Channel values come from the character file and override
any modifiers:

| Channel | Scope | Source |
|---|---|---|
| (named by `STATINPUT`, 7.4) | each stat | base score |
| `AGE`, `HEIGHT` | `PC` | biography numbers |
| `GOLD` | `PC` | `MONEY` |
| `HAIRCOLOR`, `HAIRSTYLE`, `SKINCOLOR`, `HANDED` | `PC` | biography text |
| `ALIGNMENT`, `DEITY` | `PC` | the chosen object |

**Listing.** `pcgtools pcg vars FILE [--scope S]` prints every declared
variable's value per instance, grouped under `Scope` or `Scope:Key`
headings as `name = value`, optionally limited to one scope; names that
resolved to nothing are listed afterward. With `--json` the same map
is printed as JSON.

### 9.6 Ability pools and skill points

**Ability pools.** Each ability category (from data and from the game
mode, with `FEAT` always present) is a pool. Its size is the
`ABILITYPOOL.<category>` bonus total; the `FEAT` pool adds the level
feats below. Points spent are, over the abilities chosen (`NORMAL`)
from that pool, the ability's `COST` (default 1) times its number of
choices (at least 1). Level feats are:

* the race's `STARTFEATS`;
* for each class with `LEVELSPERFEAT:p` (p > 0), floor(level / p);
  `LEVELSPERFEAT:0` gives none;
* for each game mode `BONUSFEATLEVELSTARTINTERVAL:start|interval`, over
  the levels of classes without `LEVELSPERFEAT`: none below *start*,
  else (levels − start) / interval + 1 (just 1 when *interval* is 0).

**Skill points per level** (for a class level gained at character
level *n*):

1. *sp* = the class's `STARTSKILLPTS` formula + `SKILLPOINTS.NUMBER`.
2. Unless the class has `MODTOSKILLS:NO`, add the stats'
   `MODSKILLPOINTS.NUMBER` bonuses; at least 1.
3. Add the race's `XTRASKILLPTSPERLVL`; at least 1 if step 1 was
   positive, else at least 0.
4. If `MINCLASSSKILLPOINTS.NUMBER` is positive and the class and stat
   part falls below it, raise that part to the minimum.
5. Multiply by the race's `SKILLMULT` at level 1, else by the game
   mode's `SKILLMULTIPLIER` entry for level *n* (default 1).

---

## 10. Character sheets

`pcgtools pcg sheet FILE` computes a character and prints a *sheet*: a
tree of named elements, as XML (tab-indented) or, with `--json`, as a
generic `{tag, attrs, text, children}` tree. `--compare EXPECTED.xml`
checks every leaf of an expected sheet against the computed one, prints
each mismatch as path, wanted and got, and fails on any mismatch.
Leaf paths key a repeated element by its `name` child
(`skill[Climb]`, `[Name#2]` for a repeat) and an unnamed one by
position (`para[0]`).

### 10.1 Sections

The root `character` holds these sections in order:

| Section | Children |
|---|---|
| `basics` | name, biography fields, `alignment`, `bab`, `classes`, `deity` (when set), `experience`, `face`, `favoredlist`, `gender`, `height`, `hitdice`, `languages`, `move`, `race`, `reach`, `size`, `vision`, `weight`, `poolpoints`, `notes`, ... |
| `abilities` | `ability` per stat: name, score, modifier, base, scores without equipment or temporary bonuses |
| `hit_points` | points, alternate, die, damage reduction, `history` of rolls |
| `armor_class` | total, flat, touch, base and per-type parts, max dex, spell failure, check penalty, spell resistance |
| `initiative` | total, dex_mod, misc_mod |
| `skills` | `skillpoints`, maximum ranks, then `skill` rows |
| `saving_throws` | `saving_throw` per save |
| `attack` | `melee`, `ranged`, `grapple` |
| `weapons` | `unarmed`, then `weapon` rows |
| `protection` | `armor`, `shield`, `item` rows |
| `class_features` | turning, rage, lay on hands and similar, when present |
| `equipment` | `item` rows, `total`, `equipmentsets` |
| `weight_allowance` | light, medium, heavy, lift and push limits |
| `special_abilities` | special ability texts, overall, racial and per class |
| `feats` | `feat` rows |
| `ability_objects` | `ability_object` rows of other categories |
| `domains` | `domain` with power (when any) |
| `weapon_proficiencies`, `languages` | comma-joined lists |
| `templates` | visible templates (when any) |
| `misc` | funds, companions and magic items as free text |
| `companions` | companion placeholders by follower type |
| `spells` | `spells_innate`, `known_spells`, `memorized_spells` |
| `prohibited_schools` | when any |

Numbers are written signed where the sheet shows modifiers. Distances,
heights and weights follow the game mode's default unit set
(`DEFAULTUNITSET`/`UNITSET`, default imperial feet and pounds).

### 10.2 Basics, abilities and hit points

| Value | Rule |
|---|---|
| `bab` | `COMBAT.BASEAB` |
| `alignment` | blank unless `ALIGNMENTFEATURE` is `YES` (7.4) |
| `experience/next_level` | minimum experience for the next level |
| `face` | the last race/template `FACE` (default 5), or the `FACE` control variable (or a data-declared `Face`); squares by the game mode `SQUARESIZE` (default 5) |
| `race` | the race's output name; without a race, that of the race whose `GROUP` is `Unselected` |
| `deity` | the chosen deity, else the data's default (`DEFAULTVARIABLEVALUE:DEITY`); a default naming no loaded deity shows that name with every other field blank |
| `reach` | the last race/template `REACH` (default 5) + `COMBAT.REACH`, or the `PCREACH` control variable |
| `move` | movement by type, walk first |
| `vision` | per type, the longest distance plus `VISION` bonuses |
| `hitdice` | `(LdHD)` per class, plus the hit point stat bonus and `HP.CURRENTMAX` |
| ability `modifier` | the stat's `STATMOD` (default `floor(SCORE/2)-5`); a non-ability stat shows `*` |
| `hit_points/points` | as 9.4 |
| `hit_points/history` | per level: roll, stat bonus, their sum |

### 10.3 Defense, initiative and saves

| Value | Rule |
|---|---|
| armor class parts | each a game mode `ACTYPE` (9.4): Total, Flatfooted, Touch, Base, Armor, Shield, Ability, Size, NaturalArmor, ClassDefense, Dodge, Misc |
| `max_dex` | the lowest of the load cap (medium 3, heavy 1, overloaded 0) and each equipped item's `MAXDEX` + `EQMARMOR.MAXDEX`, plus `MISC.MAXDEX`, within 0–100; blank at 100 |
| `spell_failure` | items' `SPELLFAILURE` + `EQMARMOR.SPELLFAILURE` + `MISC.SPELLFAILURE` |
| `check_penalty` | the lower of the items' (non-positive) `ACCHECK` sum and the load penalty (medium −3, heavy −6), + `MISC.ACCHECK` |
| `initiative/total` | `COMBAT.INITIATIVE` + variable `INITCOMP` |
| save `total` | as 9.4, split into base, ability, feat (ability sources of category `FEAT`), magic (equipment), race, epic and misc parts; the ability part is the `STATMODSAVE` variable when that control is set |

### 10.4 Skills

* Rows: skills with ranks, plus, once the character has a class,
  skills usable untrained that are not exclusive (or are class or
  cross-class skills), visible, and whose prerequisites pass. Rows sort
  by name ignoring spaces.
* `ranks` and totals as 9.4; `misc_mod` is the total less ranks and
  key stat modifier. An `ACHECK` of `DOUBLE` doubles the armor check
  penalty; `WEIGHT` subtracts one per 5 units carried.
* `skillpoints/used` is ranks times cost per class: game mode
  `SKILLCOST_CLASS` (default 1) for class skills, `SKILLCOST_EXCLUSIVE`
  (default 0) for exclusive skills, else `SKILLCOST_CROSSCLASS`
  (default 2). `unused` is the sum of the character file's remaining
  skill points per class (adjusted per 9.6 under the `RETROSKILL`
  rule); `total` is their sum.

### 10.5 Attacks and weapons

**Attack lines.** Each of melee, ranged and grapple totals
`TOHIT.TOHIT`, `TOHIT.TYPE.x`, `COMBAT.TOHIT` and `COMBAT.TOHIT.x`
over the base attack bonus, written as the iterative sequence: attacks
every `BABATTCYC` (default 5) while the remainder is at least 1, at
most `BABMAXATT` (default 4), joined with `/`. A class `ATTACKCYCLE`
applies where it yields more attacks.

**Weapon rows.** Weapons come from the calculation equipment set,
merged by name. An item typed both melee and ranged is split into a
melee copy and a thrown copy (`Name (Thrown)`), shown together in one
`weapon`: `melee` rows from the first, `ranges` from the second. The
thrown copy's range is `RANGE` + `EQMWEAPON.RANGEADD`. Double, natural
and non-standard weapons get a single `simple` row instead. `type`
lists damage type abbreviations; a double weapon adds those of its
second head (`ALTTYPE`) after a `/`.

| Value | Rule |
|---|---|
| hand | from the equipment set location: primary, off-hand, both, two weapons, carried, equipped |
| to-hit | attack bonus + type bonuses for the weapon's types (melee bonuses for melee use, ranged and thrown for ranged) + finesse bonus for light or finesseable melee weapons + the item's `WEAPON.TOHIT` + `WEAPONPROF=` bonuses |
| non-proficient | `WEAPONNONPROFPENALTY` (default −4); unproficient worn armor and shields add their check penalty |
| two-weapon | primary −6, off-hand −10, off-hand alone −4; `OFFHANDLIGHTBONUS` for a light off-hand weapon; natural secondary −5 |
| damage dice | `DAMAGE`, stepped for size difference and `EQMWEAPON.DAMAGESIZE` along the game mode `BASEDICE` table |
| damage bonus | `WEAPON.DAMAGE` + type damage bonuses; for melee use, a positive stat part is multiplied by the `COMBAT.DAMAGEMULT:h` total (plus proficiency and item `DAMAGEMULT:h`), where *h* is 1 one-handed, 0 off-hand, 2 two-handed (light weapons at most 1) |
| critical | threat `21 − CRITRANGE × (doublings + 1) − additions`, shown `low-20`; multiplier `CRITMULT` + `CRITMULTADD` |
| `num_attacks` | length of the attack sequence + natural extra attacks + `WEAPON.ATTACKS` |
| `feat/hit`, `feat/damage` | weapon bonuses from feats; for a weapon with no proficiency object, also feat bonuses to `WEAPONPROF=` its name |
| wield category | `WIELD` adjusted for size, then stepped along the `WIELDCATEGORY` `UP`/`DOWN` lists by `WEAPONPROF=key\|WIELDCATEGORY`, the most negative `WEAPONPROF=TYPE.x\|WIELDCATEGORY` over the item's types, and the item's `WEAPON\|WIELDCATEGORY` |
| two-handed weapon not held in two hands | `N/A` |
| game mode with no `Light` or `OneHanded` wield category | to-hit and damage empty for the unarmed strike and weapons with a proficiency (PCGen cannot tell how they are wielded) |

**Ranges.** A ranged weapon lists five range rows at multiples of its
increment *r*; a 30-foot row is inserted before the first multiple
above 30 unless 30 is itself a multiple (increment 20 gives 20, 30,
40, 60, 80). Each row's to-hit adds the range penalty
(`RANGEPENALTY`, default −2, plus `COMBAT.RANGEPENALTY`) per full
increment beyond the first, and short-range bonuses within
`SHORTRANGE` (default 30). A ranged weapon that holds ammunition
(items placed inside it in the equipment set) repeats `ranges` once
per distinct ammunition, naming it and adding its `WEAPON.TOHIT`.

### 10.6 Protection and equipment

| Value | Rule |
|---|---|
| `protection` | armor with an AC bonus, every shield, other items with an AC bonus: AC with and without modifiers, check penalty, max dex (armor), spell failure |
| `equipment/item` | items of the calculation set; identical items in one place merge; `OUTPUTORDER:-1` hides; sorted by output order, then name |
| `total/weight` | carried top-level items × quantity; containers add contents, scaled by a `CONTAINS` percentage or ignored with `*`; the first worn clothing item is free unless `CLOTHINGENCUMBRANCE` |
| `total/value` | cost × quantity over the character's items, plus twice cost × quantity for each `AUTO:EQUIP` item whose prerequisites pass; with `CURRENCYUNITABBREV` (default `gp`) |
| `total/load` | Light, Medium, Heavy or Overload by weight against `ENCUMBRANCE` fractions of the maximum load |
| `weight_allowance` | maximum load × each `ENCUMBRANCE` fraction; the maximum load is the game mode `load.lst` `LOAD` row at the `LOADSCORE` variable (else Strength score), × `LOADMULT` (default 4) per 10 points above the table, × the size's `SIZEMULT` + `LOADMULT.TYPE=SIZE` bonuses |

### 10.7 Abilities, proficiencies and languages

* `feats` lists abilities of category `FEAT`; `ability_objects` the
  categories Special Ability, Salient Divine Ability, Mutation, Talent
  and Internal. Visible entries (`VISIBLE` `YES`/`EXPORT`) come first,
  each group by nature (`NORMAL`, `AUTOMATIC`, `VIRTUAL`).
* A row is `Name (choices)` with sorted choices; repeated choices
  collapse unless `STACK:YES`. A chosen ability also granted
  automatically with the same choices, from a pool that cannot be
  edited, appears only as granted; a non-stacking ability's chosen
  entry omits choices already granted.
* `special_abilities` gathers `SAB` texts with `%` filled from
  variables; texts whose variables are all zero are dropped and
  duplicates shown as `text (n)`.
* `weapon_proficiencies`: saved proficiencies, `AUTO:WEAPONPROF`
  grants (names, `TYPE=`, `%LIST`, deity weapons; `CHANGEPROF` applied),
  and natural weapons, by output name.
* `languages`: the character's languages by name.

### 10.8 Spells

Casters are the race (when it has `SPELLS` tokens) and each class with
a spell type. Spells come from the character file, `KNOWNSPELLS`
automatic spells, domain lists and `SPELLKNOWN` grants.

| Value | Rule |
|---|---|
| caster level | a fixed `CASTERLEVEL=` wins; else class level (unless a `CASTERLEVEL.<class>` bonus exists) + `PCLEVEL.<class>` + `PCLEVEL.TYPE.<type>`, plus positive `CASTERLEVEL` bonuses for the class, `ALLSPELLS`, spell type, spell, school, subschool, descriptor and domain; at least 1 |
| cast per day | the class's `CAST` entry for the spell level at its caster class level (class level + `PCLEVEL` bonuses), + `SPELLCAST` bonuses for class or any class, level or all levels, and type, + bonus spells from the casting stat (`BONUSSPELLLEVEL`) |
| known per day | the `KNOWN` entry likewise, + `SPELLKNOWN` bonuses |
| stat limit | when the casting stat (`MAXLEVELSTAT`) is too low for a level, only bonuses count |
| specialty / domain slot | `cast` shows `+N` for specialty spells, `+1` for a divine caster's domain slot |
| DC | the game mode `SPELLBASEDC` (default `10+SPELLLEVEL+BASESPELLSTAT`) + `DC` bonuses (all spells, spell, class, domain, type, school, subschool, descriptor); blank when the spell allows no save |
| range | the game mode `SPELLRANGE` formula at the caster level, as `Name (distance)` |

Spell books are numbered in order: known spells, innate, the books of
spell-like abilities, the character file's lists, then spellbook items.
With a casting race, the prepared lists leave out the book whose number
is the position of a caster (race first) with known spells, as PCGen's
sheet does.

---

## Appendix A. A complete small example

A tiny data set, invented for illustration (TABs shown as `<TAB>`).

`data/clockwork/clockwork.pcc`:

```
CAMPAIGN:Clockwork Companions
KEY:Clockwork
GAMEMODE:Testmode
BOOKTYPE:Supplement
RANK:3
SOURCELONG:Clockwork Companions
SOURCESHORT:CC
STAT:cc_stats.lst
CLASS:cc_classes.lst
ABILITY:cc_feats.lst
EQUIPMENT:cc_equip.lst|(EXCLUDE:Broken Gear)
PCC:@/common/common.pcc
```

`cc_stats.lst`:

```
SOURCELONG:Clockwork Companions<TAB>SOURCESHORT:CC
Might<TAB>ABB:MGT<TAB>STATMOD:floor(SCORE/2)-5<TAB>BONUS:HP|BONUS|MGT
Grace<TAB>ABB:GRC<TAB>STATMOD:floor(SCORE/2)-5<TAB>BONUS:COMBAT|AC|10|TYPE=Base<TAB>BONUS:COMBAT|AC|GRC|TYPE=Ability
```

`cc_classes.lst`:

```
SOURCELONG:Clockwork Companions<TAB>SOURCESHORT:CC
CLASS:Tinker<TAB>HD:6<TAB>TYPE:Base.PC<TAB>MAXLEVEL:20<TAB>BONUS:COMBAT|BASEAB|classlevel()*3/4|TYPE=Base.REPLACE
CLASS:Tinker<TAB>CSKILL:Tinkering|Climbing
1<TAB>ABILITY:FEAT|AUTOMATIC|Steady Hands
3<TAB>ADD:FEAT|1
```

`cc_feats.lst`:

```
SOURCELONG:Clockwork Companions<TAB>SOURCESHORT:CC
Steady Hands<TAB>CATEGORY:FEAT<TAB>TYPE:General<TAB>BONUS:SKILL|Tinkering|2
Quick Step<TAB>CATEGORY:FEAT<TAB>KEY:QuickStep<TAB>PREABILITY:1,CATEGORY=FEAT,Steady Hands
CATEGORY=FEAT|Steady Hands.MOD<TAB>DESC:Your hands never shake.
```

`cc_equip.lst`:

```
SOURCELONG:Clockwork Companions<TAB>SOURCESHORT:CC
Tin Shield<TAB>TYPE:Shield<TAB>COST:5<TAB>WT:5<TAB>ACCHECK:-1<TAB>BONUS:COMBAT|AC|1|TYPE=Shield
Tin Shield.COPY=Brass Shield<TAB>COST:12
Broken Gear<TAB>TYPE:Goods
```

A character using it (`tess.pcg`):

```
PCGVERSION:2.0
CAMPAIGN:Clockwork
VERSION:6.09.00
GAMEMODE:Testmode
CHARACTERNAME:Tess Tinkerton
CITY:Gear&colon;town
STAT:MGT|SCORE:14
STAT:GRC|SCORE:12
CLASS:Tinker|LEVEL:2|SKILLPOOL:0
CLASSABILITIESLEVEL:Tinker=1|HITPOINTS:6|SKILLSGAINED:4|SKILLSREMAINING:0
CLASSABILITIESLEVEL:Tinker=2|HITPOINTS:4|SKILLSGAINED:4|SKILLSREMAINING:0
SKILL:Tinkering|OUTPUTORDER:1|CLASSBOUGHT:[CLASS:Tinker|RANKS:2.0|COST:1|CLASSSKILL:Y]
ABILITY:FEAT|TYPE:NORMAL|CATEGORY:FEAT|KEY:QuickStep
EQUIPNAME:Tin Shield|OUTPUTORDER:1|WT:5.0|QUANTITY:1.0
EQUIPSET:Default Set|ID:0.1
EQUIPSET:Shield|ID:0.1.1|VALUE:Tin Shield|QUANTITY:1.0
CALCEQUIPSET:0.1
```

What a loader derives:

* `Steady Hands` carries the `DESC` added by the `.MOD`; `Brass Shield`
  exists as a copy of `Tin Shield` with its own cost; `Broken Gear` is
  excluded by the campaign's filter.
* Tess has two Tinker levels: base attack bonus 1 (1.5 truncated); hit
  points (6+2)+(4+2) = 14 with Might 14 (+2); armor class
  10 + 1 (Grace) + 1 (shield) = 12; she has `Steady Hands` automatically
  from her first level, which satisfies `Quick Step`'s prerequisite.

## Appendix B. Tag catalog

`pcgtools tags [SCOPE]` lists the tags PCGen recognizes, by scope: a
file kind (`SPELL`, `CLASS`, ...), a line type (`CLASSLEVEL`,
`SUBCLASS`), a token family (`PRE`, `BONUS`, `CHOOSE`, `ADD`, `AUTO`),
`GLOBAL` (valid on every object), `PCC`, or a game mode file
(`GAMEMODE.MISCINFO`, ...). Deprecated tags are marked. The catalog
is generated from PCGen's sources and lists names and validity only.
