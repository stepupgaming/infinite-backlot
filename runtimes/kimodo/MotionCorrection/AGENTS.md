# Purpose

Own the vendored native motion-correction extension used by Kimodo post-processing.

# Ownership

- `src/cpp/` owns the C++ math, inverse-kinematics, trajectory-correction, and Python binding implementation.
- `python/motion_correction/` owns the Python wrapper.
- `CMakeLists.txt`, `setup.py`, `MANIFEST.in`, and `run_test.py` own native build packaging and the retained smoke test.

# Local Contracts

- Preserve the Python module name, binding ABI, numeric conventions, and call surface consumed by the sibling Kimodo package.
- Keep C++ headers/implementation and Python packaging synchronized.
- Preserve upstream attribution and do not commit compiled extensions, build directories, or virtual environments.
- Manage build-time Python dependencies through the parent `uv` environment; never install with `pip` directly.

# Work Guidance

- Limit changes to native correctness, build compatibility, or an explicitly requested upstream refresh.
- Validate Windows compiler and Python 3.12 compatibility when touching build metadata or bindings.

# Verification

- Run `uv run --frozen --no-sync python MotionCorrection/run_test.py` from the Kimodo runtime after the extension is built.
- Run the parent Kimodo smoke worker after native correction behavior changes.

# Child DOX Index

No child DOX files yet.
