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
cargo build
scripts/regen_goldens.py --fslc PATH/TO/fslc --fsl-root PATH/TO/fsl-checkout \
    --fsl-testgen target/debug/fsl-testgen
```

which runs `fslc testgen-input` and `fslc testgen` of
[ymm-oss/fsl](https://github.com/ymm-oss/fsl) for a fixed list of specs
(`CASES` in the script) and records their bytes.

There are two kinds of expected file, and they are evidence of different
things:

- **Frozen targets** (`vitest`, `swift`, `kotlin`, `dart`, `phpunit`): the
  bytes `fslc testgen` printed. `fslc` is the oracle; the test shows that this
  repository reproduces it. After regenerating, these files change only when
  `fslc` changed.
- **`pytest`**: the bytes this repository's own generator printed, because its
  suite differs from `fslc`'s on purpose ([docs/PYTEST.md](docs/PYTEST.md)).
  These files only detect a change you did not intend: review their diff by
  reading it. That the generated suite works is shown by
  `crates/fsl-testgen/tests/pytest_run.rs`, which runs it with pytest against
  the implementations in `crates/fsl-testgen/tests/pytest_run/`. The tests need
  `python3 -m pytest` (or `FSL_TESTGEN_PYTHON` pointing at an interpreter that
  has pytest) and fail without it.

`scripts/compare_with_fslc.py` runs the same comparison over every spec of an
fsl checkout without writing anything.
