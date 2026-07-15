# World Tooling

## Commands
- Assemble: `uv run --no-project python tools/world/assemble_world.py --seed 424242`
- Preflight: `uv run --no-project python tools/world/preflight.py`
- Tests: `uv run --no-project python tools/world/test_assemble_world.py -v`

## Rules
- Assembly selects reviewed registry modules; never generate raw meshes here.
- The same seed, registry version, and algorithm version must reproduce the same layout fingerprint.
- Fail with actionable missing module/socket/version messages.
- Keep preflight structural and fast; visual review belongs to one thumbnail/tour pass.
