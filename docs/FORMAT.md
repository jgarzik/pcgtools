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
8. PCG character files (TBD)

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
round-trip byte-for-byte.

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
`CLASS:Fighter.MOD`.

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
  is ignored. (Listing the same file twice, for example with different
  INCLUDE filters, is not a duplicate.)

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
depends on the tag: single-valued tags (such as `HD` or `COST`) take
the last value, list-valued tags (such as `BONUS` or `ABILITY`)
accumulate. The special value `.CLEAR` (as in `TYPE:.CLEAR`) removes
all earlier values of that tag; several tags support further
`.CLEAR.` variants inside their own value syntax (chapter 6).

### 5.4 Source information

Each object records the source tags from the file's header line
(`SOURCELONG`, `SOURCESHORT`, `SOURCEWEB`, `SOURCEDATE`) unless the
object's own tokens supply them. `SOURCEPAGE` is normally given per
object.

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

Newer data also uses a separate typed formula language in `MODIFY`
tokens and data control files: `MODIFY:var|operation|formula` with
operations such as `SET`, `ADD`, `MULTIPLY`, `MAX`, `MIN`, and a
`PRIORITY=n` option. Its expressions use the same operator grammar.

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
| `level.lst` | experience tables; a `LEVEL:LEVEL` row gives a formula for higher levels | `XPTABLE`, `LEVEL` with `MINXP`, `CSKILLMAX`, `CCSKILLMAX` |
| `load.lst` | carrying capacity | `SIZEMULT`, `LOADSCORE`, `ENCUMBRANCE` |
| `sizeAdjustment.lst` | size categories (newer modes define sizes in data instead) | |
| `rules.lst` | optional rules the user can toggle | `NAME` with `VAR`, `DEFAULT`, `EXCLUDE`, `DESC` |
| `equipmentslots.lst` | where equipment can be worn | `NUMSLOTS`, `EQSLOT` with `CONTAINS`, `NUMBER` |
| `codeControl.lst` | switches selecting engine behavior | `ALIGNMENTFEATURE`, `DOMAINFEATURE`, ... |
| `migration.lst` | renamed object keys, for loading old characters | `RACE:Old` with `NEWKEY`, `MAXVER` |
| `pointbuymethods_system.lst` | point-buy stat generation | `METHOD`, `STAT` |
| `bio/biosettings.lst` | default age/height/weight tables (as BIOSET, 5.2) | `AGESET`, `RACENAME` |
| `bio/traits.lst` | random personality traits | |
| `paperInfo.lst`, `equipIcons.lst` | output page sizes, UI icons | |

`BONUSSTACKS` lists, separated by `.`, the bonus types that stack with
themselves (see 6.3); the line may repeat.
