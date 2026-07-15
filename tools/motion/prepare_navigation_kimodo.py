"""Build rich Kimodo requests from the real navigation preflight route."""
from __future__ import annotations

import json
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
NAV_OUT = ROOT / "output/navigation-kimodo-proof"
SHOW_OUT = ROOT / "output/kimodo-control-showcase"


def distance(a, b):
    return math.sqrt(sum((float(a[i]) - float(b[i])) ** 2 for i in range(3)))


def sample_polyline(points, count):
    lengths = [distance(a, b) for a, b in zip(points, points[1:])]
    total = sum(lengths)
    if total <= 1e-6:
        return [list(points[0]) for _ in range(count)]
    out = []
    for index in range(count):
        target = total * index / max(1, count - 1)
        traversed = 0.0
        for segment, length in enumerate(lengths):
            if traversed + length >= target or segment == len(lengths) - 1:
                t = 0.0 if length <= 1e-6 else (target - traversed) / length
                out.append([points[segment][axis] + (points[segment + 1][axis] - points[segment][axis]) * t for axis in range(3)])
                break
            traversed += length
    return out


def heading(points, index):
    for radius in range(1, len(points)):
        before = points[max(0, index - radius)]
        after = points[min(len(points) - 1, index + radius)]
        dx, dz = after[0] - before[0], after[2] - before[2]
        length = math.hypot(dx, dz)
        if length > 1e-5:
            return [dx / length, 0.0, dz / length]
    return [0.0, 0.0, 1.0]


def timed_path(route, duration=18.0, fps=30):
    points = route["smoothed_path"]
    transit = [-2.2, 0.0, -4.25]
    transit_index = min(range(len(points)), key=lambda i: distance(points[i], transit))
    first = sample_polyline(points[: transit_index + 1], round(6.5 * fps) + 1)
    wait = [list(first[-1]) for _ in range(round(2.0 * fps))]
    second = sample_polyline(points[transit_index:], round((duration - 8.5) * fps))
    values = first + wait + second
    values = values[: round(duration * fps)]
    if len(values) < round(duration * fps):
        values.extend([list(values[-1])] * (round(duration * fps) - len(values)))
    return [
        {"time": index / fps, "position": point, "heading": heading(values, index)}
        for index, point in enumerate(values)
    ]


def waypoint_subset(dense, every=30):
    values = [dense[index] for index in range(0, len(dense), every)]
    if values[-1]["time"] != dense[-1]["time"]:
        values.append(dense[-1])
    return values


def navigation_request(route):
    dense = timed_path(route)
    return {
        "schema_version": 1,
        "request_id": "MARA_COLLISION_SAFE_LOBBY_TRANSIT_ODD_HOURS",
        "performer": "mara_soma",
        "duration": 18.0,
        "prompt_sequence": [
            {"start": 0.0, "end": 5.8, "text": "A woman walks briskly through a lobby while visibly annoyed, turning naturally around furniture."},
            {"start": 5.8, "end": 8.5, "text": "She slows, stops at a bus shelter, waits impatiently, and looks toward the street."},
            {"start": 8.5, "end": 14.6, "text": "She resumes a purposeful walk along the curved sidewalk route and approaches the convenience store."},
            {"start": 14.6, "end": 16.5, "text": "She reaches for the store door with her right hand, opens it, and walks through carefully."},
            {"start": 16.5, "end": 18.0, "text": "She navigates the store aisle, stops at the counter, and picks up a small object with her right hand."},
        ],
        "root_waypoints": waypoint_subset(dense, 30),
        "dense_root_path": dense,
        "arrival_heading": route["arrival_heading"],
        "full_body_keyframes": [
            {"id": "TRANSIT_WAIT_POSE", "time": 7.4, "reference_motion": "official_soma_mixed", "reference_frame": 108, "target_root": [-2.2, 0.96, -4.25], "strict": False}
        ],
        "joint_constraints": [],
        "end_effector_constraints": [
            {"id":"RIGHT_HAND_STORE_HANDLE","time":16.0,"joint":"RightHand","position":[17.42,1.08,-5.30],"rotation_xyzw":[0.09894614,0.69671262,0.70110131,-0.11514399],"position_weight":1.0,"rotation_weight":0.85,"strict":True,"reference_motion":"official_soma_ee","reference_frame":94},
            {"id":"RIGHT_HAND_COUNTER_PICKUP","time":17.9,"joint":"RightHand","position":[20.05,1.02,-8.48],"rotation_xyzw":[0.3236726,0.62027354,0.62973759,-0.33753127],"position_weight":1.0,"rotation_weight":0.8,"strict":True,"reference_motion":"official_soma_ee","reference_frame":94},
            {"id":"LEFT_FOOT_COUNTER_SUPPORT","time":17.9,"joint":"LeftFoot","position":[19.30,0.04,-8.05],"rotation_xyzw":[-0.09993928,0.81921671,-0.0748,0.55973304],"position_weight":0.9,"rotation_weight":0.7,"strict":True,"reference_motion":"official_soma_ee","reference_frame":94},
        ],
        "environment_constraints": [
            {"id":"WAIT_AT_TRANSIT","smart_interaction_id":"SMART_BUS_STOP_WAIT","target_id":"INTERACTION_VOLUME_TRANSIT_WAIT","approach_region":"NAV_REGION_TRANSIT_POCKET","staging_slot":[-2.2,0.0,-4.25],"facing":[1.0,0.0,0.0],"clearance_radius":0.45,"start":6.5,"end":8.5},
            {"id":"OPEN_STORE","smart_interaction_id":"SMART_DOOR_OPEN","target_id":"CONTROL_STORE_ENTRY","approach_region":"NAV_REGION_STORE_VESTIBULE","staging_slot":[17.0,0.0,-5.3],"facing":[0.0,0.0,-1.0],"clearance_radius":0.45,"start":14.6,"end":16.5},
            {"id":"COUNTER_PICKUP","smart_interaction_id":"SMART_PICKUP_SMALL","target_id":"INTERACT_COUNTER","approach_region":"NAV_REGION_ODD_HOURS_INTERIOR","staging_slot":[19.55,0.0,-8.15],"facing":[1.0,0.0,-0.2],"clearance_radius":0.45,"start":16.5,"end":18.0},
        ],
        "contact_events": [
            {"id":"STORE_HANDLE_CONTACT","start":15.9,"end":16.15,"performer_joint":"RightHand","target_id":"CONTROL_STORE_ENTRY","state_transition":"door.open=true"},
            {"id":"COUNTER_OBJECT_CONTACT","start":17.8,"end":18.0,"performer_joint":"RightHand","target_id":"PROP_COUNTER_PACKAGE","state_transition":"package.held_by=mara"},
        ],
        "candidate_count": 2,
        "seed": 424242,
        "strictness": 0.9,
        "continuation_pose": None,
        "output_stem": str((NAV_OUT / "candidates/navigation_candidate").resolve()),
        "navigation_contract": "assets/world/navigation/connected_navigation.json",
        "actor_radius": 0.34,
    }


def panel_request():
    fps, duration = 30, 6.0
    control = []
    for frame in range(round(duration * fps)):
        time = frame / fps
        if time < 2.8:
            u = time / 2.8
            x = 0.15 + 0.85 * u
            z = 0.10 + 1.00 * u + 0.18 * math.sin(math.pi * u)
            h = [0.65, 0.0, 0.76]
        else:
            x, z, h = 1.0, 1.1, [1.0, 0.0, 0.0]
        control.append({"time":time,"position":[x,0.0,z],"heading":h})
    return {
        "schema_version":1,"request_id":"SOMA_PANEL_PRESS_MIXED_CONSTRAINTS","performer":"canonical_soma","duration":duration,
        "prompt_sequence":[
            {"start":0.0,"end":2.6,"text":"A person follows a curved approach toward a wall panel and decelerates."},
            {"start":2.6,"end":4.8,"text":"The person turns, plants both feet, reaches with the right hand, and firmly presses the panel."},
            {"start":4.8,"end":6.0,"text":"The person retracts the hand and recovers into a balanced attentive stance."},
        ],
        "root_waypoints":waypoint_subset(control,30),"dense_root_path":control,"arrival_heading":[1.0,0.0,0.0],
        "full_body_keyframes":[{"id":"PANEL_BODY_PROXY","time":3.75,"reference_motion":"official_soma_mixed","reference_frame":108,"target_root":[1.0,0.96,1.1],"strict":False}],
        "joint_constraints":[],
        "end_effector_constraints":[
            {"id":"PANEL_RIGHT_HAND","time":4.0,"joint":"RightHand","position":[1.48,1.18,1.1],"rotation_xyzw":[-0.07175786,0.70401973,0.70440343,0.05498039],"position_weight":1.0,"rotation_weight":1.0,"strict":True,"reference_motion":"official_soma_ee","reference_frame":94},
            {"id":"PANEL_LEFT_FOOT","time":4.0,"joint":"LeftFoot","position":[0.82,0.04,1.2],"rotation_xyzw":[-0.0430384,0.38189777,-0.11717763,0.91573533],"position_weight":0.9,"rotation_weight":0.75,"strict":True,"reference_motion":"official_soma_ee","reference_frame":94},
            {"id":"PANEL_RIGHT_FOOT","time":4.0,"joint":"RightFoot","position":[1.12,0.04,1.0],"rotation_xyzw":[-0.00680938,0.22786964,-0.02993351,0.97320761],"position_weight":0.9,"rotation_weight":0.75,"strict":True,"reference_motion":"official_soma_ee","reference_frame":94},
        ],
        "environment_constraints":[{"id":"PANEL_SMART_OBJECT","smart_interaction_id":"SMART_PANEL_PRESS","target_id":"INTERACT_ELEVATOR_PANEL","approach_region":"NAV_REGION_LOBBY","staging_slot":[1.0,0.0,1.1],"facing":[1.0,0.0,0.0],"clearance_radius":0.45,"start":2.6,"end":4.8}],
        "contact_events":[{"id":"PANEL_CONTACT","start":3.9,"end":4.15,"performer_joint":"RightHand","target_id":"INTERACT_ELEVATOR_PANEL","state_transition":"panel.pressed=true"}],
        "candidate_count":2,"seed":515151,"strictness":0.95,"continuation_pose":None,
        "output_stem":str((SHOW_OUT / "panel_candidates/panel_candidate").resolve()),
    }


def main():
    route = json.loads((NAV_OUT / "resolved_route.json").read_text(encoding="utf-8"))
    nav = navigation_request(route)
    panel = panel_request()
    NAV_OUT.mkdir(parents=True, exist_ok=True)
    SHOW_OUT.mkdir(parents=True, exist_ok=True)
    (NAV_OUT / "kimodo_request.json").write_text(json.dumps(nav, indent=2), encoding="utf-8")
    (SHOW_OUT / "kimodo_requests.json").write_text(json.dumps([nav, panel], indent=2), encoding="utf-8")
    print(json.dumps({"navigation_dense_samples":len(nav["dense_root_path"]),"navigation_waypoints":len(nav["root_waypoints"]),"panel_dense_samples":len(panel["dense_root_path"]),"requests":2}))


if __name__ == "__main__":
    main()
