# Contributing

Everything in this repository is public and written in English.

## Layout

- `crates/fsl-testgen-core` — reads `testgen-input.v1` (`TestgenInput`, with
  every cross-check), the literal and identifier helpers targets print with
  (`render`), and the `Target` trait: a checked document and options in,
  generated files out. No file system access.
- `crates/fsl-testgen` — the command line and the built-in targets
  (`src/targets/`), each an implementation of `Target`.
- `schemas/` — the vendored contract schemas ([schemas/README.md](schemas/README.md)).
- `tests/golden/` — checked-in inputs and expected outputs.
- `scripts/` — the golden generator and the comparison with `fslc`.

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

## Adding a built-in target

1. Add `crates/fsl-testgen/src/targets/<name>.rs` with a unit struct that
   implements `fsl_testgen_core::Target`. Read the document only through
   `TestgenInput`; print states, parameters and scenarios in the order given.
   Escape every string that comes from the document for the position it is
   printed in (`TestgenInput::source_name()` is already escaped for comment and
   string positions of the existing targets; scenario and action names are not
   escaped for you).
2. Decide what the suite does while its adapter is not wired. The default
   (`UnwiredAdapter::Fail`) must make every generated test fail; `Skip` may skip,
   or the target refuses it with an error when the framework cannot skip at run
   time. A test that passes without calling the implementation is the one
   outcome that is not allowed.
3. Register it in `builtin_targets()` in `src/targets/mod.rs`, add it to
   `TARGETS` in `crates/fsl-testgen/tests/support/mod.rs` and to the table in
   `README.md`.
4. Add its expected outputs to the goldens (below) and extend
   `unwired_adapter_fails_by_default_and_skips_only_on_opt_in` in
   `src/targets/tests.rs` with the failing construct and the skipping one.

## Goldens

`tests/golden/<case>/` holds `input.json` (a `testgen-input.v1` document),
`case.json` (how it was produced) and one expected file per target:
`<target>` and `<target>.unwired` (`--allow-unwired`), or
`<target>.unwired.error` with the message when the target refuses.

The tests only read these files. They are written by

```sh
scripts/regen_goldens.py --fslc PATH/TO/fslc --fsl-root PATH/TO/fsl-checkout
```

which runs `fslc testgen-input` and `fslc testgen` of
[ymm-oss/fsl](https://github.com/ymm-oss/fsl) for a fixed list of specs
(`CASES` in the script) and records their bytes. Review the diff after
regenerating: the expected files of a frozen target change only when `fslc`
changed.

`scripts/compare_with_fslc.py` runs the same comparison over every spec of an
fsl checkout without writing anything.
