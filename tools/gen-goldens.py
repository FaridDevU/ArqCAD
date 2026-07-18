#!/usr/bin/env python3
"""Validate af-io-dxf DXF export goldens with ezdxf (an independent implementation).

For every file in ``crates/af-io-dxf/tests/golden/dxf/export/*.dxf``:
  1. open it with ``ezdxf.readfile`` (invalid structure fails),
  2. run ``doc.audit()`` and require **0 errors and 0 fixes**,
  3. count model-space entities and compare them with the expected count.

This cross-checks our export against a mature, license-compatible DXF implementation
(ezdxf, MIT). The acceptance criterion is a clean ``ezdxf.audit()`` result.

Usage:
    python3 tools/gen-goldens.py            # report (never fails)
    python3 tools/gen-goldens.py --check    # exit 1 if validation fails (CI/local)

Environment setup (ezdxf is not installed system-wide; use a virtual environment):
    python3 -m venv .venv
    .venv/bin/pip install ezdxf        # tested with ezdxf 1.4.4
    .venv/bin/python tools/gen-goldens.py --check

Note: despite its historical name, this script **does not generate** the .dxf files
(the exporter does that through ``UPDATE_GOLDEN=1 cargo test -p af-io-dxf``); it only
audits them. The name is retained for compatibility with existing automation.
"""

from __future__ import annotations

import sys
from pathlib import Path

# Expected entities per golden (independent of the count ezdxf obtains when it rereads
# our export). Keep this in sync with the tests/export.rs fixture.
EXPECTED_COUNTS = {
    "empty.dxf": 0,
    "fixture.dxf": 4,
}

REPO = Path(__file__).resolve().parent.parent
GOLDEN_DIR = REPO / "crates" / "af-io-dxf" / "tests" / "golden" / "dxf" / "export"


def main() -> int:
    check = "--check" in sys.argv[1:]
    try:
        import ezdxf
    except ImportError:
        msg = "ezdxf is not installed; see the virtual-environment setup in the docstring."
        print(f"SKIP: {msg}")
        # In --check mode, a missing ezdxf package is an environment failure, not an
        # export failure: return 2 to distinguish it from an invalid golden (1).
        return 2 if check else 0

    files = sorted(GOLDEN_DIR.glob("*.dxf"))
    if not files:
        print(f"no golden files found in {GOLDEN_DIR}")
        return 1 if check else 0

    ok = True
    for path in files:
        name = path.name
        try:
            doc = ezdxf.readfile(str(path))
        except Exception as exc:  # noqa: BLE001 - report every read failure
            print(f"FAIL {name}: could not open file: {exc}")
            ok = False
            continue

        auditor = doc.audit()
        n_err = len(auditor.errors)
        n_fix = len(auditor.fixes)
        ents = [e.dxftype() for e in doc.modelspace()]
        n = len(ents)
        expected = EXPECTED_COUNTS.get(name)

        problems = []
        if n_err:
            problems.append(f"{n_err} audit errors")
        if n_fix:
            problems.append(f"{n_fix} audit fixes")
        if expected is not None and n != expected:
            problems.append(f"count {n} != expected {expected}")

        status = "OK  " if not problems else "FAIL"
        print(f"{status} {name}: acadver={doc.dxfversion} insunits={doc.header.get('$INSUNITS')} "
              f"entities={n} {ents}")
        for e in auditor.errors:
            print(f"     ERR  {e}")
        for e in auditor.fixes:
            print(f"     FIX  {e.code} {e.message}")
        if problems:
            print(f"     -> {'; '.join(problems)}")
            ok = False

    if check and not ok:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
