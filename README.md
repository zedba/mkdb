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
make -pRrq [-f FILE] 2>/dev/null | mkdb [-a]
mkdb [-a] [--] DUMP...
```

By default `mkdb` lists phony targets and variables. `-a` also lists file
targets and pattern rules. `--` ends options, for a DUMP whose name starts with
`-`. `-h` prints a summary; `--help` prints more detail.

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
target    .PHONY         -
target    prog           Makefile:6
variable  CC             Makefile:1
variable  MAKEFILE_LIST  Makefile:1
variable  prog:CFLAGS    Makefile:4
```

| Kind       | Meaning                                              |
|------------|------------------------------------------------------|
| `phony`    | Target listed as a prerequisite of `.PHONY`          |
| `target`   | Any other target (`-a` only)                         |
| `pattern`  | Pattern rule, e.g. `%.o: %.c` (`-a` only)            |
| `variable` | Variable; `SCOPE:VAR` if target- or pattern-specific |

The location is `FILE:LINE` when make records it. Otherwise it is the
variable's origin in angle brackets (`<environment>`, `<default>`,
`<automatic>`, `<command line>`, `<makefile>`, `<override>`), or `-` for a
target without a recipe. Paths under the directory make ran in (`CURDIR`) are
shown relative to it.

## Limitations

- A target's location is that of its recipe. Make does not record where a rule
  without a recipe appears, so such targets show `-`.
- If make stops on a makefile parse error (`*** ... Stop.` on stderr), the
  database it prints has no phony markers, so the default output shows only
  variables.
- Tested with GNU Make 4.3. The database format is not a stable interface.
