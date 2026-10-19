# PCGen Data File Formats

This document describes the on-disk formats used by the PCGen character
generator, as implemented by `pcgtools`. It is an independent
description, written from studying the behavior of PCGen and the data
that ships with it. Where PCGen's behavior is lenient or surprising,
this document says so and describes what `pcgtools` does.

Contents:

1. [Overview](#1-overview)
2. [Text encoding and lines](#2-text-encoding-and-lines)
3. LST data files (TBD)
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
