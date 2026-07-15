from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MODULE_PATH = ROOT / "tools/world/assemble_world.py"


def load_module():
    spec = importlib.util.spec_from_file_location("assemble_world", MODULE_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


class WorldAssemblyTests(unittest.TestCase):
    def test_same_seed_produces_identical_connected_layout(self):
        module = load_module()
        registry = json.loads((ROOT / "assets/world/registry.json").read_text())
        first = module.assemble(registry, 424242)
        second = module.assemble(registry, 424242)
        self.assertEqual(first, second)
        self.assertGreaterEqual(len(first["instances"]), 12)
        self.assertGreaterEqual(len(first["connections"]), 10)
        categories = {entry["category"] for entry in first["instances"]}
        self.assertIn("building_exterior", categories)
        self.assertIn("lobby", categories)
        self.assertIn("street", categories)
        self.assertTrue({"alley", "courtyard"} & categories)
        self.assertTrue({"hero_business", "business_exterior"} & categories)

    def test_different_seed_changes_floor_variation_without_losing_required_modules(self):
        module = load_module()
        registry = json.loads((ROOT / "assets/world/registry.json").read_text())
        first = module.assemble(registry, 424242)
        other = module.assemble(registry, 424243)
        self.assertNotEqual(first["layout_fingerprint"], other["layout_fingerprint"])
        self.assertEqual(
            {entry["role"] for entry in first["instances"]},
            {entry["role"] for entry in other["instances"]},
        )

    def test_registry_socket_references_are_validated(self):
        module = load_module()
        registry = json.loads((ROOT / "assets/world/registry.json").read_text())
        registry["modules"][0]["sockets"] = []
        with self.assertRaisesRegex(ValueError, "socket"):
            module.validate_registry(registry)


if __name__ == "__main__":
    unittest.main()
