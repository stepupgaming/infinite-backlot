import json
import struct
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CELLS = [
    "cell_street_extension",
    "cell_public_transit_pocket",
    "cell_industrial_transition",
]


def glb_json(path: Path):
    data = path.read_bytes()
    magic, version, length = struct.unpack_from("<III", data, 0)
    assert magic == 0x46546C67 and version == 2 and length == len(data)
    offset = 12
    while offset < length:
        size, chunk_type = struct.unpack_from("<II", data, offset)
        offset += 8
        chunk = data[offset : offset + size]
        offset += size
        if chunk_type == 0x4E4F534A:
            return json.loads(chunk.decode("utf-8").rstrip("\x00 \t\r\n"))
    raise AssertionError("GLB JSON chunk missing")


class WorldExpansionContractTests(unittest.TestCase):
    def test_three_cells_are_editable_exported_and_semantic(self):
        for cell_id in CELLS:
            source = ROOT / "assets/source/blender/world/cells" / f"{cell_id}.blend"
            glb = ROOT / "assets/world/cells" / f"{cell_id}.glb"
            sidecar = ROOT / "assets/world/cells" / f"{cell_id}.scene.json"
            self.assertTrue(source.is_file(), cell_id)
            self.assertTrue(glb.is_file(), cell_id)
            self.assertGreater(len(glb_json(glb).get("meshes", [])), 10, cell_id)
            data = json.loads(sidecar.read_text())
            self.assertEqual(data["module_id"], cell_id)
            self.assertGreaterEqual(len(data["sockets"]), 2)
            self.assertGreaterEqual(len(data["staging_marks"]), 4)
            self.assertGreaterEqual(len(data["camera_anchors"]), 3)
            self.assertGreaterEqual(len(data["lighting"]), 2)
            self.assertTrue(data["neighbor_compatibility"])

    def test_reusable_asset_library_is_deliberate_and_cataloged(self):
        catalog_path = ROOT / "assets/world/kits/infinite_backlot_expansion_assets.catalog.json"
        catalog = json.loads(catalog_path.read_text())
        self.assertGreaterEqual(len(catalog["assets"]), 20)
        self.assertTrue(all(item["quality_tier"] in {"production", "hero"} for item in catalog["assets"]))
        self.assertTrue(all(item["provenance"] == "project-owned-procedural" for item in catalog["assets"]))
        self.assertIn("geometry_nodes", catalog["techniques_applied"])
        self.assertIn("asset_browser", catalog["techniques_applied"])
        self.assertIn("linked_collections", catalog["techniques_applied"])

    def test_expanded_master_and_cells_are_registered(self):
        registry = json.loads((ROOT / "assets/world/registry.json").read_text())
        modules = {module["module_id"]: module for module in registry["modules"]}
        for module_id in CELLS + ["infinite_backlot_expanded_world"]:
            self.assertIn(module_id, modules)
            self.assertEqual(modules[module_id]["quality_tier"], "hero")
        self.assertEqual(registry["module_count"], len(registry["modules"]))
        cells = json.loads((ROOT / "assets/world/cells/world_cells.json").read_text())
        self.assertEqual(len(cells["cells"]), 6)
        expanded = next(cell for cell in cells["cells"] if cell["cell_id"] == "CELL_CONNECTED_NEIGHBORHOOD")
        self.assertGreaterEqual(len(expanded["connection_sockets"]), 4)


if __name__ == "__main__":
    unittest.main()
