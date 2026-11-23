# The pytest target is baked

`fsl-testgen generate --target pytest` writes a suite that reads nothing but
itself. It imports only `pytest`. The expected states of the scenarios and of
the walk are literals in the file, as in the other five targets.

This is a deliberate difference from `fslc testgen --target pytest`, whose
suite imports `fslc.runtime` (a Python module that is not published,
ymm-oss/fsl issue #1211) and opens the spec when the tests run. The other five
targets still print exactly what `fslc testgen` prints; pytest does not.

## What stays the same

- The adapter surface: `Adapter.reset()`, `Adapter.step(action, params)`,
  `Adapter.observe()`. An `Adapter` written for the old suite fits the new one.
- One test per scenario, with the same names, steps and expected states, and
  the same partial comparison (only the fields a scenario mentions).
- A forbidden scenario runs its steps and then asserts that the forbidden step
  is rejected: `step` must return a dict with `ok` false and, when the
  document names one, the expected `kind`.
- `test_random_walk_conformance` compares the whole observed state with `==`
  after `reset()` and after every step.
- An unwired suite fails every test. With `--allow-unwired` it skips them.

## What changes

- The walk is the document's `walk` (`testgen-trace.v1`), replayed. The old
  suite computed its walk when the tests ran, by stepping the runtime Monitor
  with `random.Random(0)` for up to 100 steps.
- A failing walk step says which step it was:
  `random walk step 2: bank.settle {}`.
- The header records `source.digest`, the SHA-256 of the spec the suite was
  generated from.
- The header no longer offers `MonitorSelfAdapter`, and messages name
  `fsl-testgen generate --allow-unwired`.

## What the old suite checked that this one cannot

Everything the runtime Monitor decided while the tests ran is now decided when
`fslc testgen-input` writes the document. Three consequences:

1. **The suite does not follow the spec.** The old walk test read the spec
   each time it ran. Change `forall i: ItemId { stock[i] = 1 }` to `= 2` in a
   cart spec and the old `test_random_walk_conformance` at once expects a
   stock of 2 (while the scenario tests in the same file, which were always
   baked, still expect 1). The baked suite expects 1 everywhere until it is
   regenerated. Nothing in the file notices that the spec moved on; the digest
   in the header is there so that you, or a check of your own, can.

2. **The spec is not re-checked when the tests run.** The old walk failed with
   `spec oracle violation` when the Monitor reported an invariant, `ensures`
   or type-bound violation of the spec itself during the walk. That check now
   happens once, in the gate of `fslc testgen-input`, which emits no document
   for a walk with a violation. A baked suite cannot report it.

3. **No second interpreter.** The old suite ran the spec through the Python
   runtime, the baked walk comes from the walk `fslc` itself takes. Where the
   two agree nothing is lost, and for the kernel, compose, requirements,
   business and forbidden goldens of this repository they do agree: the
   runtime's walk under `random.Random(0)` is, step for step and state for
   state, the walk in the document. Where they disagree the suites differ, and
   there is such a case: for the db and domain goldens the runtime Monitor
   names state fields `schema.version` or `Order.status`, where the document
   (and the scenario tests of the old suite itself) name them `schema_version`
   and `order_status`. The old suite therefore compared the same adapter
   against two spellings in one file; the baked suite uses the document's
   names throughout.

The suite is also no longer a way to run the Monitor against itself: without
`fslc.runtime` there is no `MonitorSelfAdapter`.
