# Schemas

## Copied from ymm-oss/fsl

`testgen-input.v1.schema.json` is the contract this repository reads. It
refers to three other schemas by `$id`, so they are vendored with it:

| File | `$id` |
|---|---|
| `testgen-input.v1.schema.json` | `https://fsl.dev/schemas/fslc/kernel/testgen-input.v1.schema.json` |
| `testgen-trace.v1.schema.json` | `https://fsl.dev/schemas/fslc/kernel/testgen-trace.v1.schema.json` |
| `test-plan.v1.schema.json` | `https://fsl.dev/schemas/fslc/kernel/test-plan.v1.schema.json` |
| `kernel.v1.schema.json` | `https://fsl.dev/schemas/fslc/kernel/kernel.v1.schema.json` |

They are byte-for-byte copies of `schemas/fslc/kernel/` in
[ymm-oss/fsl](https://github.com/ymm-oss/fsl) at commit
`4fa7e3281ace1ae5768f59a4a713adbdd4dc52cc` (pull request #2043, which adds
`fslc testgen-input`). Do not edit them here; copy them again when the
contract gets a new minor version.

The tests validate every golden input against `testgen-input.v1.schema.json`.
`fsl-testgen` itself does not load the schemas at run time: it reads a document
through the cross-checks in `fsl-testgen-core`, which are stricter than the
schema (they also compare names across `state`, `actions`, `scenarios`, `walk`
and `kernel`).

## Defined here

| File | What |
|---|---|
| `plugin-request.v1.schema.json` | what an external target executable reads on standard input |
| `plugin-response.v1.schema.json` | what it writes on standard output |

They are plugin protocol 1, described in [docs/PLUGIN.md](../docs/PLUGIN.md).
The tests validate every request `fsl-testgen` sends against the request
schema, and check that the response schema and the reader in
`fsl-testgen-core` accept and reject the same samples.
