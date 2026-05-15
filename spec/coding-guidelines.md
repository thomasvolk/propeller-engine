# Rust Coding Guidelines

Based on the [official Rust Style Guide](https://doc.rust-lang.org/style-guide/).

## Automatic Formatting (Mandatory)

**All code must be formatted with `rustfmt` before committing.** No exceptions.

Run formatting:

```
cargo fmt
```

CI must reject any code that is not `rustfmt`-clean. Configure your editor to run `rustfmt` on save.

---

## Guiding Principles

Formatting priority (highest to lowest):

1. **Readability** — scan-ability, avoid misleading layout, accessible in plain text (diffs, grep, error messages)
2. **Aesthetics** — visual consistency with the broader Rust ecosystem
3. **VCS-friendliness** — minimize diffs, prevent rightward drift, minimize vertical space
4. **Simplicity** — prefer rules that are easy to apply and implement

---

## Indentation and Line Width

- Indent with **4 spaces** (never tabs)
- Maximum line width: **100 characters**
- Prefer **block indent** over visual indent to reduce rightward drift and minimize diffs

---

## Naming Conventions

| Item | Convention | Example |
|---|---|---|
| Types, traits, enum variants | `UpperCamelCase` | `MyStruct`, `MyVariant` |
| Functions, methods | `snake_case` | `my_function()` |
| Local variables, struct fields | `snake_case` | `my_var`, `my_field` |
| Macros | `snake_case` | `my_macro!` |
| Constants, immutable statics | `SCREAMING_SNAKE_CASE` | `MAX_SIZE` |

When a name conflicts with a reserved keyword, prefer a trailing underscore (`crate_`) over misspelling or raw identifiers.

---

## Comments

- Prefer line comments (`//`) over block comments (`/* */`)
- Single space after the opening sigil: `// comment`
- Write complete sentences: capital first letter, period at end
- Limit comment-only lines to **80 characters**
- Place comments on their own line, above the code they describe
- Use `///` for doc comments on items; use `//!` only at module or crate level
- Place doc comments before attributes

---

## Trailing Commas

Use trailing commas in all comma-separated lists that span multiple lines:

```rust
// correct
foo(
    arg1,
    arg2,
    arg3,
)

// wrong
foo(
    arg1,
    arg2,
    arg3
)
```

Single-line lists do not use trailing commas.

---

## Blank Lines

- Zero or one blank line between statements or items within a block
- One blank line between top-level items
- No trailing whitespace on any line (including blank lines)

---

## Imports

- Place `extern crate` statements first, alphabetically sorted
- Follow with `use` statements and `mod` declarations
- Version-sort imports within a group (`x8` before `x16`)
- `self` and `super` sort first within a group; glob imports sort last
- No spaces inside braces: `use foo::{Bar, Baz};`

---

## Functions

```rust
[pub] [unsafe] [extern ["ABI"]] fn foo(arg1: i32, arg2: i32) -> i32 {
    ...
}
```

If the signature does not fit on one line, break after `(` and place each argument on its own indented line with a trailing comma:

```rust
pub fn long_function_name(
    argument_one: SomeType,
    argument_two: AnotherType,
) -> ReturnType {
    ...
}
```

---

## Structs and Enums

**Struct fields:** each on its own line, block-indented, trailing comma.

```rust
struct Foo {
    field_one: i32,
    field_two: String,
}
```

Prefer `struct Foo;` over `struct Foo {}` for unit structs.

**Enum variants:** each on its own line, block-indented, trailing comma.

```rust
enum Direction {
    North,
    South,
    East,
    West,
}
```

If any variant is multi-line, use multi-line formatting for all struct-like variants.

---

## Traits and Impls

- Block-indent trait and impl items
- Format empty traits and impls on a single line: `trait Foo {}`
- Space after `:`, spaces around `+` in bounds: `T: Foo + Bar`

---

## Expressions

**Blocks:** newline after `{` and before `}` unless the block qualifies as single-line (single expression, no statements, no comments).

**Closures:** omit `{}` when the body is a single expression without statements.

**Binary operators:** spaces around all operators. Break after assignment operators, before other operators when wrapping.

**Ranges:** no spaces around `..` or `..=`: `0..10`, `x..=y`.

**Casts:** format like a binary operator; break before `as` when wrapping.

---

## Control Flow

- Opening brace on the same line as the keyword
- `else` and `else if` on the same line as the closing brace of the preceding block
- Single-line `if-else` only in expression context and only when small

---

## Match Expressions

- Break after `{` and before `}`; block-indent arms once
- Trailing comma after arm patterns unless the arm body is a block
- Do not start a pattern with `|`

```rust
match value {
    Foo::A => handle_a(),
    Foo::B => {
        handle_b();
    }
    Foo::C | Foo::D => handle_cd(),
}
```

---

## Types

- References: `&T`, `&mut T`, `&'a T`, `&'a mut T` (no space after `&`)
- Raw pointers: `*const T`, `*mut T` (no space after `*`)
- Slices: `[T]` (no spaces)
- Arrays: `[T; N]` (space after `;`)
- Generics: `Foo<T, U>` (spaces after commas, no trailing comma)
- Trait bounds: `T + U + V` (single space between types and `+`)

---

## Cargo.toml

- Same line width (100 chars) and 4-space indentation as Rust code
- Blank line between the last key-value pair of a section and the next section header
- No blank lines within a section
- In `[package]`: `name` and `version` first, `description` last
- Other sections: version-sort keys alphabetically
- Single space before and after `=`: `key = "value"`
- No unnecessary quotes around bare keys
- Authors format: `Full Name <email@address>`
- License: valid SPDX expression (e.g. `MIT OR Apache-2.0`)
