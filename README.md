# mkdb

List the targets and variables in a GNU make project, and where each is defined.
`mkdb` reads make's internal database (`make -p`) rather than the makefiles, so
includes, `eval`, and conditionals are already resolved.

## Build

```sh
cargo build --release   # binary at target/release/mkdb
```

## Usage

```sh
mkdb [-vsptfa]
make -pRrq [MAKE-ARGS] 2>/dev/null | mkdb [-vsptfa]
mkdb [-vsptfa] < DUMP
mkdb [-pfa] --tree=TARGET [< DUMP]
```

When stdin is a terminal, `mkdb` runs `${MAKE:-make} -pRrq` in the current
directory, passing make's stderr through. Otherwise it reads a database from
stdin. Pipe one in to pass make arguments such as `-C DIR`, `-f FILE`, or
`VAR=value`, or when stdin is not a terminal (scripts, CI). Either way, make
reads the makefiles, which has side effects (see [Limitations](#limitations)).

Flags select which kinds of rows to show (see [Output](#output)); `-a` shows
all of them. With none of `-v`, `-s`, `-p`, `-t`, or `-f`, `mkdb` shows global
variables and phony targets, as if given `-vp`. `--tree` prints a dependency
tree instead (see [Dependency tree](#dependency-tree)). `-h` prints a summary;
`--help` prints more detail.

## Output

Each row has three columns: kind, name, and location. Given this Makefile:

```make
CC = gcc
.PHONY: all clean
all: prog
prog: CFLAGS += -g
prog: main.o
	$(CC) -o $@ $^
%.o: %.c
	$(CC) $(CFLAGS) -c $<
clean:
	rm -f prog *.o
```

`mkdb` prints the following (`grep` drops built-in and environment variables;
`column -t` re-aligns what's left):

```
$ make -pRrq 2>/dev/null | mkdb -a | grep -v '<' | column -t
pattern   %.o            Makefile:8
phony     all            -
phony     clean          Makefile:10
scoped    prog:CFLAGS    Makefile:4
target    .PHONY         -
target    prog           Makefile:6
variable  CC             Makefile:1
variable  MAKEFILE_LIST  Makefile:1
```

| Kind       | Flag | Meaning                                              |
|------------|------|------------------------------------------------------|
| `variable` | `-v` | Global variable                                      |
| `scoped`   | `-s` | Target- or pattern-specific variable, as `SCOPE:VAR` |
| `phony`    | `-p` | Target listed as a prerequisite of `.PHONY`          |
| `pattern`  | `-t` | Pattern rule, e.g. `%.o: %.c`                        |
| `target`   | `-f` | Any other target (a file target)                     |

The location is `FILE:LINE` when make records it. Otherwise it is the
variable's origin in angle brackets (`<environment>`, `<default>`,
`<automatic>`, `<command line>`, `<makefile>`, `<override>`), or `-` for a
target without a recipe. Paths under the directory make ran in (`CURDIR`) are
shown relative to it.

## Dependency tree

`--tree=TARGET` prints TARGET's prerequisites recursively, drawn like `tree`.
TARGET may be given relative to `CURDIR`. Given this Makefile:

```make
.PHONY: all
all: prog docs
prog: main.o util.o | outdir
	cc -o $@ $^
main.o: main.c common.h
util.o: util.c common.h
%.o: %.c
	cc -c $<
outdir:
	mkdir -p $@
docs:: README
	echo a
docs:: prog
	echo b
```

`mkdb` prints:

```
$ mkdb -a --tree=all
all
├── prog
│   ├── main.o
│   │   ├── main.c
│   │   └── common.h
│   ├── util.o
│   │   ├── util.c
│   │   └── common.h
│   └── outdir
└── docs
    ├── README
    └── prog (*)
```

Prerequisites appear in make's order, including order-only ones (`outdir`) and
those of every double-colon rule (`docs`). A target whose prerequisites were
already shown is marked `(*)` and not expanded again; this also cuts cycles.

The kind flags filter the tree: `-p` shows phony prerequisites and `-f` shows
files, whether or not they have a rule. `-v`, `-s`, and `-t` have no effect,
since variables and pattern rules are not prerequisites. A hidden prerequisite
hides everything below it; TARGET itself is always shown. So by default (`-vp`)
the tree outlines the phony targets reachable from TARGET through other phony
targets; `-a` shows the full tree, as above.

## Limitations

- Reading the makefiles is not side-effect free. `-q` keeps make from building
  targets, but make still remakes out-of-date included makefiles (running their
  recipes), and any `$(shell ...)` expanded during parsing (e.g. in a `:=`
  assignment) runs.
- A target's location is that of its recipe. Make does not record where a rule
  without a recipe appears, so such targets show `-`.
- If make stops on a makefile parse error (`*** ... Stop.` on stderr), the
  database it prints has no phony markers, so the default output shows only
  global variables.
- Tested with GNU Make 4.3. The database format is not a stable interface.
