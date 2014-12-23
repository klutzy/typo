typo is a small utility which generates useful tables for Rust programmer.
It is intended to be used via [typo.vim](https://github.com/klutzy/typo.vim).

# Usage

```
typo [OPTIONS] [INPUT]

Options:
    --cfg SPEC
    -L PATH
    --sysroot PATH
    --tags PATH
    --node-id-map PATH
    --type-map PATH
```

-   `INPUT` must be root of crate. (typo tries to compile the file!)
-   Usually `--sysroot` should be set, where `rustc` is at `$SYSROOT/bin`.

# Tags

Tags file is generated if `--tags PATH` is passed.

Unlike Rust's default ctags.rust, typo parses Rust source code thus it can
generate better table.

Major sales points include:

-   Recognizes `mod other;` and jumps to `others.rs`.
-   Recognizes struct fields and enum variants.
-   Recognizes macro-generated items.

Unfortunately, typo doesn't recognize multiple crates yet.

# NodeId Map

NodeId map is generated if `--node-id-map PATH` is passed.

typo generates list of `(filename, start_pos, end_pos, node_id)`.
It will be used with other maps.

# TypeMap

Type map is generated if both `--node-id-map PATH` and `--type-map PATH` are
passed.

typo Generates list of `(node_id, type)`.
With NodeId map, it is possible to find type of expression from filename and
cursor position.

# TODOs

Currently the following features are planned:

-   Multiple crate support
-   Rename support (variable name, type, etc.)
