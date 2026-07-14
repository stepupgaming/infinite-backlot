import json
import tempfile
import unittest
from pathlib import Path

import numpy as np

from gemmy_run_kimodo import motion_preview_observation


class MotionPreviewObservationTests(unittest.TestCase):
    def test_unconstrained_motion_still_emits_real_frame_count(self):
        with tempfile.TemporaryDirectory() as directory:
            motion = Path(directory) / "motion.npz"
            np.savez(motion, posed_joints=np.zeros((12, 77, 3), dtype=np.float32))
            self.assertEqual(
                motion_preview_observation(motion, None),
                {
                    "type": "motion_preview",
                    "frame": 0,
                    "total_frames": 12,
                    "waypoint_count": 0,
                },
            )

    def test_constraints_report_exact_waypoint_count(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            motion = root / "motion.npz"
            constraints = root / "constraints.json"
            np.savez(motion, posed_joints=np.zeros((1, 8, 77, 3), dtype=np.float32))
            constraints.write_text(
                json.dumps([{"frame_indices": [0, 4]}, {"frame_indices": [7]}]),
                encoding="utf-8",
            )
            observation = motion_preview_observation(motion, str(constraints))
            self.assertEqual(observation["total_frames"], 8)
            self.assertEqual(observation["waypoint_count"], 3)


if __name__ == "__main__":
    unittest.main()
