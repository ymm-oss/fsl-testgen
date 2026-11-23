# Implementations the generated pytest suites are run against

`tests/pytest_run.rs` generates the pytest suite of a golden input, appends one
line that binds `Adapter` to a class of these modules, and runs pytest for
real. Each module holds a small hand-written implementation of the spec that
conforms, and one that does not, with the step at which it must be caught.

| Module | Golden case | Spec in ymm-oss/fsl |
|---|---|---|
| `cart.py` | `kernel` | `specs/cart_v1.fsl` |
| `bank_system.py` | `compose` | `specs/bank_system.fsl` (with `bank_impl.fsl`, `audit_log.fsl`) |
| `orders.py` | `forbidden` | `examples/gallery/valid/small_forbidden_guarded_cancel.fsl` |
