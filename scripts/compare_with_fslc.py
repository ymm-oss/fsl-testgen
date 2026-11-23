#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Compare `fsl-testgen generate` with `fslc testgen` over a tree of specs.

    scripts/compare_with_fslc.py --fslc FSLC --fsl-testgen FSL_TESTGEN \\
        --fsl-root PATH/TO/fsl [--depth 4] [DIR ...]

For every `.fsl` file under the directories (default: specs examples,
relative to --fsl-root) this asks fslc for the `testgen-input.v1` document
and, when the gate lets it through, compares the bytes of every frozen target,
with and without --allow-unwired, against what fsl-testgen generates from the
document alone. Refusals are compared by message. Exit code 1 when anything
differs.
"""

import argparse
import json
import pathlib
import subprocess
import sys

# Targets whose output is frozen to `fslc testgen`'s bytes.
FROZEN_TARGETS = ["pytest", "vitest", "swift", "kotlin", "dart", "phpunit"]


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--fslc", required=True, type=pathlib.Path)
    parser.add_argument("--fsl-testgen", required=True, type=pathlib.Path)
    parser.add_argument("--fsl-root", required=True, type=pathlib.Path)
    parser.add_argument("--depth", default="4")
    parser.add_argument("--targets", default=",".join(FROZEN_TARGETS))
    parser.add_argument("directories", nargs="*", default=["specs", "examples"])
    arguments = parser.parse_args()
    root = arguments.fsl_root.resolve()
    fslc = str(arguments.fslc.resolve())
    testgen = str(arguments.fsl_testgen.resolve())
    targets = arguments.targets.split(",")

    def run(command, stdin=None):
        return subprocess.run(command, cwd=root, input=stdin, capture_output=True, check=False)

    specs = sorted(
        str(path.relative_to(root))
        for directory in arguments.directories
        for path in (root / directory).rglob("*.fsl")
    )
    gate_rejected = {}
    accepted = compared = identical = refusals = 0
    differences = []
    for spec in specs:
        document = run([fslc, "testgen-input", spec, "--depth", arguments.depth])
        if document.returncode != 0:
            gate_rejected[document.returncode] = gate_rejected.get(document.returncode, 0) + 1
            continue
        accepted += 1
        for target in targets:
            for flags in ([], ["--allow-unwired"]):
                label = " ".join([spec, target, *flags])
                theirs = run([fslc, "testgen", spec, "--depth", arguments.depth,
                              "--target", target, *flags])
                ours = run([testgen, "generate", "--target", target, "--spec-path", spec, *flags],
                           stdin=document.stdout)
                compared += 1
                if theirs.returncode == 0 and ours.returncode == 0:
                    if theirs.stdout == ours.stdout:
                        identical += 1
                    else:
                        differences.append(f"{label}: bytes differ")
                elif theirs.returncode == 2 and ours.returncode == 4:
                    refusals += 1
                    theirs_message = json.loads(theirs.stdout)["message"]
                    ours_message = json.loads(ours.stderr)["message"]
                    if theirs_message != ours_message:
                        differences.append(
                            f"{label}: refusal differs: {theirs_message!r} / {ours_message!r}")
                else:
                    differences.append(
                        f"{label}: fslc exit {theirs.returncode}, fsl-testgen exit "
                        f"{ours.returncode}: {ours.stderr.decode().strip()}")
    print(f"specs: {len(specs)}")
    print(f"accepted by the gate: {accepted}")
    print("rejected by the gate: "
          + (", ".join(f"{count} with exit {code}" for code, count in sorted(gate_rejected.items()))
             or "0"))
    print(f"comparisons: {compared}")
    print(f"byte-identical: {identical}")
    print(f"refusals with the same message: {refusals - sum('refusal differs' in d for d in differences)}")
    print(f"differences: {len(differences)}")
    for difference in differences:
        print(f"  {difference}")
    sys.exit(1 if differences else 0)


if __name__ == "__main__":
    main()
