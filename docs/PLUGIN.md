# Target plugins (protocol 1)

A target that is not built in is an executable. `fsl-testgen` sends it the
checked document and the options, the executable answers with the files of the
suite, and `fsl-testgen` writes them. This is the one extension point: there
are no templates to override and nothing is loaded into the process.

```sh
fslc testgen-input cart.fsl | fsl-testgen generate --target go --out-dir internal/cart
```

## How a target name is resolved

For `--target NAME`:

1. A built-in target of that name is used. Nothing can replace a built-in
   target: an executable called `fsl-testgen-target-pytest` is never looked at,
   and a `[targets.pytest]` entry in a configuration file is an error
   (exit code 7) for every generation that sees the file.
2. Otherwise the nearest `fsl-testgen.toml` in the working directory or a
   directory above it is consulted:

   ```toml
   [targets.go]
   command = ["python3", "tools/fsl_testgen_go.py"]   # required, argv
   timeout_seconds = 30                                # optional
   ```

   The command runs **in the directory of the configuration file**, so its
   relative arguments mean the same wherever `fsl-testgen` is started. There is
   no shell: `command` is an argument vector.
3. Otherwise an executable `fsl-testgen-target-NAME` on `PATH` is used. It runs
   in the working directory of `fsl-testgen`.
4. Otherwise the target is unknown (exit code 2).

A target name is letters, digits, `-`, `_` and `.`, starting with a letter or
digit. The configuration file is closed: unknown members are errors.

A team-internal target is an executable in a private repository, put on `PATH`
or named in the project's `fsl-testgen.toml`.

## The exchange

`fsl-testgen` starts the executable once per generation, writes one request to
its standard input, closes it, and reads one response from its standard output.

### Request ([`schemas/plugin-request.v1.schema.json`](../schemas/plugin-request.v1.schema.json))

```json
{
  "protocol": 1,
  "input": { "…": "the testgen-input.v1 document" },
  "options": { "unwired": "fail", "spec_path": null, "output": null }
}
```

- `input` is the `testgen-input.v1` document exactly as `fslc testgen-input`
  wrote it. Before the plugin is started the document has passed every
  cross-check of `fsl-testgen` (the ones listed in the README); **a plugin is
  never started for a document that failed one**, so a plugin does not have to
  re-check that scenarios and walk name only known state fields, actions and
  parameters, or that `spec`, `walk.spec` and `kernel.spec.name` agree.
- **Member order is part of the contract.** Print state objects, parameter
  objects and scenarios in the order given, and parse the request with a parser
  that keeps the order of object members. For a compose root the order of
  `walk.initial` differs from `state`.
- `kernel` is `null` for a compose root. Everything a generator needs is in
  `state`, `actions`, `scenarios` and `walk`; read `kernel` only for extras
  (types, for a typed adapter), and be ready for it to be absent.
- `scenarios` may be empty, and so may `walk.steps`.
- `source.name` is the raw file name of the spec. It can contain quotes, `*/`,
  `?>` and other characters that end a comment or a string in your language:
  escape it for the position you print it in. The same holds for scenario
  names, action names and string values.
- `options.unwired`: see "What a plugin must do" below.
- `options.spec_path` and `options.output` are `--spec-path` and `-o` /
  `--out-dir` as given on the command line (or `null`). They are information:
  the plugin does not write, and a relative path is relative to the directory
  `fsl-testgen` was started in, which is not the plugin's when it was found
  through a configuration file.

### Response ([`schemas/plugin-response.v1.schema.json`](../schemas/plugin-response.v1.schema.json))

```json
{
  "protocol": 1,
  "files": [{ "path": "cart_conformance_test.go", "content": "package cart\n…" }],
  "diagnostics": [{ "severity": "warning", "message": "…" }]
}
```

- Exit with status 0 and print exactly this object on standard output. Use
  standard error for anything else; it is quoted when the plugin fails.
- `files[].path` is relative to the output directory, with `/` between
  components. `fsl-testgen` refuses an absolute path, a drive letter, a `.` or
  `..` component, an empty component, a backslash, a NUL, and the same path
  twice. It also refuses, when writing, a path that leaves the output
  directory through a symbolic link that is already there.
- A diagnostic with severity `error` refuses the generation: nothing is
  written, and `fsl-testgen` exits with code 4 and the messages. Use it for an
  option or a document your target cannot handle. `warning` and `info` are
  printed on standard error after a successful generation, one JSON object per
  line: `{"result":"diagnostic","severity":…,"message":…}`.
- The response is closed: unknown members are errors.

### What `fsl-testgen` does with the response

A plugin never writes. `fsl-testgen` checks the whole response first and then
writes:

- one file and no `-o` / `--out-dir`: its content is printed on standard output;
- one file and `-o FILE`: written to `FILE` (the file's own `path` is checked
  but not used);
- `--out-dir DIR`: every file is written to `DIR/path`. `DIR` must exist;
  directories below it are created;
- several files without `--out-dir`: refused (exit code 2).

Files are written to temporary names first and renamed into place only when all
of them could be written, so a refusal leaves the output directory as it was.
Each rename replaces one file as a whole; several files are not replaced as one
atomic unit, and if a rename fails part-way the error names the files already
replaced.

| The plugin… | Exit code | `kind` |
|---|---|---|
| returns a path outside the output directory, or the same path twice | 6 | `plugin` |
| prints something that is not the response object, or another `protocol` | 6 | `plugin` |
| exits with a status other than 0 (whatever it printed) | 6 | `plugin` |
| cannot be started | 6 | `plugin` |
| does not finish in time | 6 | `plugin` |
| answers with an `error` diagnostic | 4 | `target` |

In every one of these cases nothing is written.

### Time limit

A plugin has 60 seconds. `timeout_seconds` in the configuration changes that
for one target, and `--plugin-timeout SECONDS` overrides both. A plugin that is
not done by then is killed and the generation fails. Only the process
`fsl-testgen` started is killed; a process that one leaves behind is not. A
response larger than 64 MiB is refused.

## What a plugin must do

These are requirements on the plugin author. The first one cannot be checked
from outside, and `fsl-testgen` does not claim to check it.

1. **An unwired suite fails.** Every generated suite has an adapter the user
   connects to the implementation. With `options.unwired == "fail"` (the
   default), every generated test must fail until the adapter is wired, with a
   message that says so. A suite that never calls the implementation and still
   passes is not evidence that the implementation conforms; it is a green
   result that says nothing, and it is exactly what a newly generated,
   untouched suite would otherwise produce in CI (ymm-oss/fsl issue #1200).
2. **`skip` skips, or is refused.** With `options.unwired == "skip"`
   (`--allow-unwired`) every generated test is reported as *skipped* by the
   test framework while the adapter is not wired. If the framework cannot skip
   at run time, answer with an `error` diagnostic instead of generating tests
   that return early and pass. The built-in `kotlin` target does this.
3. **A forbidden scenario asserts the rejection.** A scenario with
   `kind == "forbidden"` and a `forbidden_step` runs its `steps` and then
   asserts that the implementation rejects `forbidden_step` (with
   `rejected_by` as the reason, when it is not `null`).
4. **Every step and every expected state is asserted**, for each scenario and
   for the walk (`walk.initial`, then `expected` after each step).
5. **The same request gives the same response.** No timestamps, no random
   names, no dependence on the environment.

## `fsl-testgen plugin-check`

```sh
fsl-testgen plugin-check [--timeout SECONDS] COMMAND [ARGUMENT...]
```

runs the plugin over the golden inputs of this repository (they are compiled
into the binary) and prints one line per check. It exits 0 when every check
passed and 1 when one failed.

What it checks, for each input with `unwired` `fail` and `skip`:

- the plugin exits 0 and its response is valid, including the path rules;
- a second run gives the same response;
- there is at least one file and no file is empty; a refusal (an `error`
  diagnostic) fails the check for `fail` and is reported as a note for `skip`;
- the inputs include a compose root (`kernel: null`), a document with no
  scenarios and no walk steps, and a spec whose file name is
  `we'ird*na>me?.fsl`. For that one a note says whether the name appears
  verbatim in the output; whether that is safe depends on where it is printed.

And once for every scenario kind in the inputs (`acceptance`,
`action_coverage`, `deadlock`, `forbidden`, `leadsTo`, `reachable`): the output
for a document with only the scenarios of that kind differs from the output for
the same document without scenarios. That shows the kind is not ignored. It
does not show the generated test is right.

**What it does not check**, because it runs the plugin and never the generated
suite:

- that the suite fails while unwired (requirement 1), or skips rather than
  passes with `skip` (requirement 2);
- that the generated files compile, run and assert what requirements 3 and 4
  ask for;
- that strings from the document are escaped correctly for the language.

Passing `plugin-check` therefore means the plugin speaks the protocol. Show the
rest the way this repository does for its own targets: generate a suite, run it
unwired and expect failures, run it against an implementation that conforms and
one that does not. `crates/fsl-testgen/tests/plugin.rs` does the first of these
for the example plugin, and `crates/fsl-testgen/tests/pytest_run.rs` does all of
them for the built-in pytest target.

## An example

[`examples/unittest-plugin/`](../examples/unittest-plugin) is a complete plugin
in about a hundred lines of Python: a target for Python's standard `unittest`.

```sh
cd examples/unittest-plugin
fslc testgen-input ../../path/to/spec.fsl | fsl-testgen generate --target unittest --out-dir .
fsl-testgen plugin-check ./fsl-testgen-target-unittest
```

## Compatibility

`protocol` is an integer. A plugin exits with a non-zero status for a
`protocol` it does not know; `fsl-testgen` rejects a response with another
value. A change that an existing plugin could not ignore gets a new number.
`input` follows `testgen-input.v1`: any `1.x` document can arrive, and a later
minor only adds optional members where that schema is declared extensible.
