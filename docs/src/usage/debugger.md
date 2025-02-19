# Debugging programs

A very useful tool in the Miden compiler suite, is its TUI-based interactive debugger, accessible
via the `midenc debug` command.

> [!WARNING]
> The debugger is still quite new, and while very useful already, still has a fair number of
> UX annoyances. Please report any bugs you encounter, and we'll try to get them patched ASAP!

## Getting started

The debugger is launched by executing `midenc debug`, and giving it a path to a program compiled
by `midenc compile`. See [Program Inputs](#program-inputs) for information on how to provide inputs
to the program you wish to debug. Run `midenc help debug` for more detailed usage documentation.

The debugger may also be used as a library, but that is left as an exercise for the reader for now.

## Example

```shell
# Compile a program to MAST from a rustc-generated Wasm module
midenc compile foo.wasm -o foo.masl

# Load that program into the debugger and start executing it
midenc debug foo.masl
```

## Program inputs

To pass arguments to the program on the operand stack, or via the advice provider, you have two
options, depending on the needs of the program:

1. Pass arguments to `midenc debug` in the same order you wish them to appear on the stack. That
   is, the first argument you specify will be on top of the stack, and so on.
2. Specify a configuration file from which to load inputs for the program, via the `--inputs` option.

### Via command line

To specify the contents of the operand stack, you can do so following the raw arguments separator `--`.
Each operand must be a valid field element value, in either decimal or hexadecimal format. For example:

```shell
midenc debug foo.masl -- 1 2 0xdeadbeef
```

If you pass arguments via the command line in conjunction with `--inputs`, then the command line arguments
will be used instead of the contents of the `inputs.stack` option (if set). This lets you specify a baseline
set of inputs, and then try out different arguments using the command line.

### Via inputs config

While simply passing operands to the `midenc debug` command is useful, it only allows you to specify
inputs to be passed via operand stack. To provide inputs via the advice provider, you will need to use
the `--inputs` option. The configuration file expected by `--inputs` also lets you tweak the execution
options for the VM, such as the maximum and expected cycle counts.

An example configuration file looks like so:

```toml
# This section is used for execution options
[options]
max_cycles = 5000
expected_cycles = 4000

# This section is the root table for all inputs
[inputs]
# Specify elements to place on the operand stack, leftmost element will be on top of the stack
stack = [1, 2, 0xdeadbeef]

# This section contains input options for the advice provider
[inputs.advice]
# Specify elements to place on the advice stack, leftmost element will be on top
stack = [1, 2, 3, 4]

# The `inputs.advice.map` section is a list of advice map entries that should be
# placed in the advice map before the program is executed. Entries with duplicate
# keys are handled on a last-write-wins basis.
[[inputs.advice.map]]
# The key for this entry in the advice map
digest = '0x3cff5b58a573dc9d25fd3c57130cc57e5b1b381dc58b5ae3594b390c59835e63'
# The values to be stored under this key
values = [1, 2, 3, 4]

[[inputs.advice.map]]
digest = '0x20234ee941e53a15886e733cc8e041198c6e90d2a16ea18ce1030e8c3596dd38''
values = [5, 6, 7, 8]
```

## Usage

Once started, you will be dropped into the main debugger UI, stopped at the first cycle of
the program. The UI is organized into pages and panes, with the main/home page being the
one you get dropped into when the debugger starts. The home page contains the following panes:

* Source Code - displays source code for the current instruction, if available, with
  the relevant line and span highlighted, with syntax highlighting (when available)
* Disassembly - displays the 5 most recently executed VM instructions, and the current
  cycle count
* Stack Trace - displays a stack trace for the current instruction, if the program was
  compiled with tracing enabled. If frames are unavailable, this pane may be empty.
* Operand Stack - displays the contents of the operand stack and its current depth
* Breakpoints - displays the set of current breakpoints, along with how many were hit
  at the current instruction, when relevant

### Keyboard shortcuts

On the home page, the following keyboard shortcuts are available:

Shortcut | Mnemonic       | Description   |
---------|----------------|---------------|
`q`      | quit           | exit the debugger |
`h`      | next pane      | cycle focus to the next pane |
`l`      | prev pane      | cycle focus to the previous pane |
`s`      | step           | advance the VM one cycle |
`n`      | step next      | advance the VM to the next instruction |
`c`      | continue       | advance the VM to the next breakpoint, else to completion |
`e`      | exit frame     | advance the VM until we exit the current call frame, a breakpoint is triggered, or execution terminates |
`d`      | delete         | delete an item (where applicable, e.g. the breakpoints pane) |
`:`      | command prompt | bring up the command prompt (see below for details) |

When various panes have focus, additional keyboard shortcuts are available, in any pane
with a list of items, or multiple lines (e.g. source code), `j` and `k` (or the up and
down arrows) will select the next item up and down, respectively. As more features are
added, I will document their keyboard shortcuts below.

### Commands

From the home page, typing `:` will bring up the command prompt in the footer pane.

You will know the prompt is active because the keyboard shortcuts normally shown there will
no longer appear, and instead you will see the prompt, starting with `:`. It supports any
of the following commands:

Command      | Aliases      | Action            | Description   |
-------------|--------------|-------------------|---------------|
`quit`       | `q`          | quit              | exit the debugger |
`debug`      |              | show debug log    | display the internal debug log for the debugger itself |
`reload`     |              | reload program    | reloads the program from disk, and resets the UI (except breakpoints) |
`breakpoint` | `break`, `b` | create breakpoint | see [Breakpoints](#breakpoints) |
`read`       | `r`          | read memory       | inspect linear memory (see [Reading Memory](#reading-memory) |

## Breakpoints

One of the most common things you will want to do with the debugger is set and manage breakpoints.
Using the command prompt, you can create breakpoints by typing `b` (or `break` or `breakpoint`),
followed by a space, and then the desired breakpoint expression to do any of the following:

* Break at an instruction which corresponds to a source file (or file and line) whose name/path
  matches a pattern
* Break at the first instruction which causes a call frame to be pushed for a procedure whose name
  matches a pattern
* Break any time a specific opcode is executed
* Break at the next instruction
* Break after N cycles
* Break at CYCLE

The syntax for each of these can be found below, in the same order (shown using `b` as the command):

Expression          | Description   |
--------------------|---------------|
`b FILE[:LINE]`     | Break when an instruction with a source location in `FILE` (a glob pattern) <br />_and_ that occur on `LINE` (literal, if provided) are hit. |
`b in NAME`         | Break when the glob pattern `NAME` matches the fully-qualified procedure name<br /> containing the current instruction |
`b for OPCODE`      | Break when the an instruction with opcode `OPCODE` is exactly matched <br />(including immediate values) |
`b next`            | Break on the next instruction |
`b after N`         | Break after `N` cycles |
`b at CYCLE`        | Break when the cycle count reaches `CYCLE`. <br />If `CYCLE` has already occurred, this has no effect |

When a breakpoint is hit, it will be highlighted, and the breakpoint window will display the number
of hit breakpoints in the lower right.

After a breakpoint is hit, it expires if it is one of the following types:

* Break after N
* Break at CYCLE
* Break next

When a breakpoint expires, it is removed from the breakpoint list on the next cycle.

## Reading memory

Another useful diagnostic task is examining the contents of linear memory, to verify that expected
data has been written. You can do this via the command prompt, using `r` (or `read`), followed by
a space, and then the desired memory address and options:

The format for read expressions is `:r ADDR [OPTIONS..]`, where `ADDR` is a memory address in
decimal or hexadecimal format (the latter requires the `0x` prefix). The `read` command supports
the following for `OPTIONS`:

Option          | Alias | Values | Default | Description  |
----------------|-------|-----------------|---------|--------------|
`-mode MODE`    | `-m`  | <ul><li>`words` (`word` ,`w`)</li><li>`bytes` (`byte`, `b`)</ul> | `words` | Specify a memory addressing mode |
`-format FORMAT`| `-f`  | <ul><li>`decimal` (`d`)</li><li>`hex` (`x`)</li><li>`binary` (`bin`, `b`)</li></ul>| `decimal` | Specify the format used to print integral values |
`-count N`      | `-c`  | | `1` | Specify the number of units to read |
`-type TYPE`    | `-t`  | See [Types](#types) | `word` | Specify the type of value to read<br />This also has the effect of modifying the default `-format` and unit size for `-count` |

Any invalid combination of options, or invalid syntax, will display an error in the status bar.

### Types

Type    | Description  |
--------|--------------|
`iN`    | A signed integer of `N` bits |
`uN`    | An unsigned integer of `N` bits |
`felt`  | A field element |
`word`  | A Miden word, i.e. an array of four field elements |
`ptr` or `pointer`  | A 32-bit memory address (implies `-format hex`) |

## Roadmap

The following are some features planned for the near future:

* **Watchpoints**, i.e. cause execution to break when a memory store touches a specific address
* **Conditional breakpoints**, i.e. only trigger a breakpoint when an expression attached to it
  evaluates to true
* More DYIM-style breakpoints, i.e. when breaking on first hitting a match for a file or
  procedure, we probably shouldn't continue to break for every instruction to which that
  breakpoint technically applies. Instead, it would make sense to break and then temporarily
  disable that breakpoint until something changes that would make breaking again useful.
  This will rely on the ability to disable breakpoints, not delete them, which we don't yet
  support.
* More robust type support in the `read` command
* Display procedure locals and their contents in a dedicated pane
