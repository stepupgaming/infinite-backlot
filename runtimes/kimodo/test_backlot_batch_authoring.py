import tempfile
import unittest

import numpy as np

import torch

import backlot_batch_kimodo as batch
from kimodo.constraints import compute_global_heading, create_pairs
from kimodo.skeleton import SOMASkeleton30


class BatchAuthoringTests(unittest.TestCase):
    @unittest.skipUnless(torch.cuda.is_available(), "CUDA Kimodo runtime required")
    def test_constraint_pair_indices_normalize_to_frame_device(self):
        pairs = create_pairs(torch.tensor([4, 9], device="cuda"), torch.tensor([1, 2]))
        self.assertEqual(pairs.device.type, "cuda")
        self.assertEqual(pairs.tolist(), [[4, 1], [4, 2], [9, 1], [9, 2]])

    def test_reference_pose_is_rotated_to_authored_heading(self):
        skeleton = SOMASkeleton30()
        positions, rotations = batch.load_reference("official_soma_ee", 94)
        request = {
            "dense_root_path": [
                {"time": 0.0, "position": [2.0, 0.0, 3.0], "heading": [1.0, 0.0, 0.0]}
            ]
        }
        positioned, _ = batch.orient_reference_pose(
            positions, rotations, request, 0.0, skeleton, "cpu"
        )
        heading = compute_global_heading(torch.from_numpy(positioned[None]), skeleton)[0]
        np.testing.assert_allclose(heading.numpy(), [0.0, 1.0], atol=1e-4)
        np.testing.assert_allclose(positioned[skeleton.root_idx, [0, 2]], [2.0, 3.0], atol=1e-4)

    def test_prompt_sequence_and_dense_root_constraint_are_preserved(self):
        request = {
            "duration": 4.0,
            "prompt_sequence": [
                {"start": 0.0, "end": 1.5, "text": "walks cautiously"},
                {"start": 1.5, "end": 4.0, "text": "turns and reaches"},
            ],
            "dense_root_path": [
                {"time": 0.0, "position": [0.0, 0.0, 0.0], "heading": [0.0, 0.0, 1.0]},
                {"time": 2.0, "position": [1.0, 0.0, 1.0], "heading": [1.0, 0.0, 0.0]},
                {"time": 4.0, "position": [2.0, 0.0, 1.0], "heading": [1.0, 0.0, 0.0]},
            ],
        }
        prompts, frames = batch.prompt_inputs(request, 30.0)
        self.assertEqual(prompts, ["walks cautiously", "turns and reaches"])
        self.assertEqual(frames, [45, 75])
        constraint = batch.root_constraint_dict(request, 30.0)
        self.assertEqual(constraint["type"], "root2d")
        self.assertEqual(constraint["frame_indices"], [0, 60, 119])
        self.assertEqual(constraint["smooth_root_2d"][-1], [2.0, 1.0])
        self.assertEqual(constraint["global_root_heading"][0], [1.0, 0.0])
        self.assertEqual(constraint["global_root_heading"][1], [0.0, 1.0])

    def test_text_export_normalization_removes_trailing_whitespace(self):
        with tempfile.TemporaryDirectory() as directory:
            path = batch.Path(directory) / "motion.bvh"
            path.write_text("ROOT Hips  \n1 2 3 \n", encoding="utf-8")
            batch.normalize_text_export(path)
            self.assertEqual(path.read_text(encoding="utf-8"), "ROOT Hips\n1 2 3\n")

    def test_structural_candidate_score_rejects_obstacle_intersection(self):
        metrics = batch.CandidateMetrics(
            root_path_deviation=0.02,
            hand_target_error=0.03,
            hand_orientation_error_deg=2.0,
            foot_slide=0.01,
            floor_penetration=0.0,
            body_obstacle_intersections=1,
            duration_error=0.0,
            arrival_heading_error_deg=1.0,
            contact_timing_error=0.02,
            joint_limit_violations=0,
        )
        evaluation = batch.evaluate_metrics(metrics)
        self.assertFalse(evaluation["valid"])
        self.assertIn("body_obstacle_intersection", evaluation["rejection_reasons"])

    def test_root_error_uses_requested_corridor(self):
        target = np.array([[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]], dtype=np.float32)
        generated = np.array([[0.0, 0.0], [1.0, 0.1], [2.0, 0.0]], dtype=np.float32)
        self.assertAlmostEqual(batch.root_path_deviation(generated, target), 0.1, places=5)


if __name__ == "__main__":
    unittest.main()
