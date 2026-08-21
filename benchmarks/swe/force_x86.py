"""Force the SWE-bench harness to use x86_64 images for every instance.

On Apple Silicon the harness prefers arm64 images, but ~40 Verified instances have no
arm64 build and cannot get one: their environments pin packages (e.g. setuptools 38.2.4)
that conda never built for aarch64. The x86_64 images exist for all 500 and run here
under emulation, so this simply widens the harness's own USE_X86 escape hatch to
everything. It changes which image is pulled, never what is executed inside it.
"""
import swebench.harness.test_spec.test_spec as ts

class _Everything(frozenset):
    def __contains__(self, item):  # noqa: D105
        return True

ts.USE_X86 = _Everything()
