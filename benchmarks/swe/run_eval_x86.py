#!/usr/bin/env python3
"""Run the official SWE-bench evaluation with x86_64 images forced (see force_x86)."""
import sys, pathlib, runpy
sys.path.insert(0, str(pathlib.Path(__file__).parent))
import force_x86  # noqa: F401  (patches USE_X86 before the harness parses args)
runpy.run_module("swebench.harness.run_evaluation", run_name="__main__")
