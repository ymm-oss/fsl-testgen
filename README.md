# fsl-testgen

Conformance-test generators for [FSL](https://github.com/ymm-oss/fsl) specs.

`fslc`, the FSL compiler, checks a spec and writes everything a test generator
needs into one JSON document, `testgen-input.v1`. `fsl-testgen` turns that
document into a test suite for your language and test framework. The meaning
of the spec stays in `fslc`; this repository only renders.

```sh
fslc testgen-input cart.fsl | fsl-testgen generate --target vitest -o cart.conformance.test.ts
```

`fslc testgen-input` prints a document only after its gate has passed (bounded
verification, scenarios, the refinement fold and a violation-free walk), so a
suite is generated only for a spec that passed. `fsl-testgen` never reads the
spec itself.

## Targets

| Target | Framework | `--allow-unwired` |
|---|---|---|
| `pytest` | pytest | skips |
| `vitest` | Vitest (TypeScript) | skips |
| `swift` | Swift Testing | skips |
| `kotlin` | kotlin.test | refused: kotlin.test has no portable runtime skip |
| `dart` | package:test | skips |
| `phpunit` | PHPUnit 10+ | skips |

`fsl-testgen targets` lists them. Any other target is an external executable:
see [Plugins](#plugins).

Every generated suite has an adapter for you to connect to your implementation
(`reset`, `step`, `observe`). Until the adapter is wired, every generated test
**fails**: a suite that never calls the implementation is not evidence that it
conforms. `--allow-unwired` generates a suite that skips instead.

Every generated suite is self-contained: the expected states of the scenarios
and of a fixed walk are baked into it, and it needs neither the spec nor `fslc`
when it runs. Regenerate it when the spec changes.

The output of `vitest`, `swift`, `kotlin`, `dart` and `phpunit` is
byte-for-byte what `fslc testgen --target T` prints for the same spec; the
tests and `scripts/compare_with_fslc.py` check it. That is why their generated
text still names `fslc testgen` in places. `pytest` is the exception: the suite
`fslc testgen` prints imports the unpublished `fslc.runtime`, the one generated
here does not. [docs/PYTEST.md](docs/PYTEST.md) says what that changes and what
the old suite checked that the new one cannot.

## Command line

```
fsl-testgen generate --target TARGET [--input FILE] [-o FILE | --out-dir DIR]
                     [--allow-unwired] [--spec-path PATH] [--plugin-timeout SECONDS]
fsl-testgen targets
fsl-testgen plugin-check [--timeout SECONDS] COMMAND [ARGUMENT...]
fsl-testgen --version
```

- `--input FILE`: the `testgen-input.v1` document. Without it the document is
  read from stdin.
- `-o FILE`: write the suite to `FILE` instead of stdout. The file is replaced
  as a whole or not at all. The directory must exist.
- `--out-dir DIR`: write every generated file under the existing directory
  `DIR`, each under the name the target gives it (`test_conformance.py`,
  `conformance.test.ts`, `ConformanceTests.swift`, `<Spec>ConformanceTest.kt`,
  `conformance_test.dart`, `<Spec>ConformanceTest.php`). Needed for a target
  that generates more than one file.
- `--spec-path PATH`: where the spec is. The document records only the file
  name of the spec (`source.name`). No built-in target reads the option: no
  built-in suite refers to the spec. A plugin is told.
- `--plugin-timeout SECONDS`: how long an external target may take (60 by
  default).

### Exit codes and errors

| Code | `kind` | Meaning |
|---|---|---|
| 0 | | generated |
| 2 | `usage` | bad arguments, or a target that does not exist |
| 3 | `input` | the document is not JSON, has another major version, or failed a cross-check |
| 4 | `target` | the target refused the options or the document |
| 5 | `io` | the input could not be read or the output written |
| 6 | `plugin` | an external target broke the protocol: it could not be started, exited non-zero, ran out of time, or answered with something `fsl-testgen` refuses |
| 7 | `config` | `fsl-testgen.toml` is invalid, or names a built-in target |

A failure prints nothing on stdout and one JSON object on one line of stderr:

```json
{"result":"error","kind":"input","message":"unsupported testgen-input schema_version '2.0.0'; expected 1.x"}
```

With `-o`, success prints `{"result":"generated","target":…,"spec":…,"output":…}`
on stdout; with `--out-dir`, `"out_dir"` and the list of `"files"` instead of
`"output"`. Nothing is written unless the exit code is 0.

`plugin-check` is a report for people: one line per check on stdout, exit code
0 when every check passed and 1 when one failed.

## Plugins

A target that is not built in is an executable: `fsl-testgen-target-NAME` on
`PATH`, or a command named in `fsl-testgen.toml`:

```toml
[targets.go]
command = ["python3", "tools/fsl_testgen_go.py"]
```

`fsl-testgen` checks the document, sends `{protocol, input, options}` to the
executable's standard input, reads `{protocol, files, diagnostics}` from its
standard output, checks that, and does the writing itself. A plugin is never
started for a document that failed a cross-check, cannot write outside the
output directory, and cannot replace a built-in target.

[docs/PLUGIN.md](docs/PLUGIN.md) is the protocol, with its two JSON Schemas in
[`schemas/`](schemas/README.md) and an example in
[`examples/unittest-plugin/`](examples/unittest-plugin). `fsl-testgen
plugin-check COMMAND` checks what can be checked mechanically about a plugin,
and says what cannot: above all, that the suite it generates fails while its
adapter is not wired.

## Compatibility with the contract

`fsl-testgen` reads `testgen-input.v1`
(`https://fsl.dev/schemas/fslc/kernel/testgen-input.v1.schema.json`, vendored
in [`schemas/`](schemas/README.md)).

- Any `schema_version` `1.x.y` is accepted; any other major is rejected with
  exit code 3. A later minor may add optional members only where the schema is
  declared extensible, so a `1.x` document that carries a member this version
  does not know at a closed location is rejected rather than half-read.
- The order of object members is part of the contract: suites print states,
  parameters and scenarios in the order given. The document is parsed with the
  order preserved.
- Before a target runs, the document goes through the same cross-checks `fslc`
  applies to its own emitters' input: `spec`, `walk.spec` and
  `kernel.spec.name` agree; scenarios and the walk name only state fields of
  `state` and actions and parameters of `actions`; `state` and `actions` agree
  with `kernel` when there is one; `kernel` is `null` only for a compose root;
  scenarios are in normalized order. A document that fails is rejected (exit
  code 3) and no target sees it.
- `test_plan`, when present, is checked for shape and otherwise not used.
- `fslc domain testgen` applies post-processing that is not part of
  `testgen-input.v1`; a domain document is generated here as any other.

## Building

```sh
cargo build --release      # the binary is target/release/fsl-testgen
cargo test
```

The toolchain is pinned in `rust-toolchain.toml`.

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
