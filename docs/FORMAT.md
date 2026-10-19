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
4. PCC campaign files (TBD)
5. Per-file-type structure (TBD)
6. Token value syntax (TBD)
7. Game mode files (TBD)
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

Operations are not applied in file order. See 5.x for the deferral
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
