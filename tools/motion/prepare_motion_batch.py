"""Prepare the durable Kimodo motion-lab batch from reviewed request metadata."""
from __future__ import annotations

import argparse
import json
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]


def main() -> int:
    parser=argparse.ArgumentParser()
    parser.add_argument("--source",default="data/motion_lab/motion_requests.json")
    parser.add_argument("--output",default="output/motion-lab/batch.request.json")
    args=parser.parse_args()
    requests=json.loads((ROOT/args.source).read_text(encoding="utf-8"))
    raw_root=ROOT/"output/motion-lab/raw"
    expanded=[]
    seen=set()
    for request in requests:
        semantic=request["semantic"]
        if semantic in seen: raise ValueError(f"duplicate semantic: {semantic}")
        seen.add(semantic)
        item=dict(request)
        item["output_stem"]=(raw_root/semantic/"motion").as_posix()
        expanded.append(item)
    output=ROOT/args.output; output.parent.mkdir(parents=True,exist_ok=True)
    output.write_text(json.dumps(expanded,indent=2),encoding="utf-8")
    print(f"prepared {len(expanded)} motion requests -> {output}")
    return 0


if __name__=="__main__": raise SystemExit(main())
