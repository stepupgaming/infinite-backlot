"""Render canonical SOMA motion previews and build the motion review browser/reel.

This is deliberately a review tool, not an approval bot. It uses NVIDIA Kimodo's
bundled SOMA skin directly, avoiding KayKit retargeting during motion judgment.
"""
from __future__ import annotations

import argparse
import html
import json
import os
import subprocess
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
KIMODO_PY=Path(r"C:/Projects/gemmy/runtimes/kimodo/.venv/Scripts/python.exe")
RENDERER=ROOT/"runtimes/kimodo/gemmy_render_kimodo_skins.py"
OUTPUT=ROOT/"output/motion-showcase"
INDEX_PATH=ROOT/"assets/animations/library/motion_lab_index.json"


def run(command:list[str]) -> None:
    environment=os.environ.copy()
    environment.pop("PYTHONPATH",None)
    subprocess.run(command,cwd=ROOT,check=True,env=environment)


def render_previews(index:list[dict],face_stride:int=6) -> None:
    preview_dir=OUTPUT/"previews"; preview_dir.mkdir(parents=True,exist_ok=True)
    for i,item in enumerate(index,1):
        source=Path(item["source_npz"])
        target=preview_dir/f"{item['semantic']}.mp4"
        if target.exists() and target.stat().st_size>1024:
            print(f"[{i}/{len(index)}] reuse {target.name}")
        else:
            print(f"[{i}/{len(index)}] render {item['semantic']}")
            run([str(KIMODO_PY),str(RENDERER),"--input",str(source),"--output",str(target),"--ffmpeg","ffmpeg","--width","480","--height","270","--fps","30","--face-stride",str(face_stride),"--skin","chrome_editor"])
        item["preview"]=target.relative_to(ROOT).as_posix()
    INDEX_PATH.write_text(json.dumps(index,indent=2),encoding="utf-8")


def build_showcase(index:list[dict]) -> None:
    segment_dir=OUTPUT/"segments"; segment_dir.mkdir(parents=True,exist_ok=True)
    concat_lines=[]
    font=r"C\:/Windows/Fonts/segoeuib.ttf"
    for item in index:
        source=ROOT/item["preview"]; segment=segment_dir/f"{item['semantic']}.mp4"
        label=f"{item['category'].upper()}  /  {item['semantic'].replace('_',' ').upper()}"
        vf=f"scale=640:360:force_original_aspect_ratio=decrease,pad=640:360:(ow-iw)/2:(oh-ih)/2:color=0x080d18,drawtext=fontfile='{font}':text='{label}':x=24:y=22:fontsize=24:fontcolor=white:box=1:boxcolor=black@0.55:boxborderw=9"
        run(["ffmpeg","-y","-hide_banner","-loglevel","error","-t","2.4","-i",str(source),"-vf",vf,"-an","-c:v","libx264","-pix_fmt","yuv420p","-r","30",str(segment)])
        concat_lines.append(f"file '{segment.as_posix()}'")
    concat=OUTPUT/"segments.txt"; concat.write_text("\n".join(concat_lines),encoding="utf-8")
    showcase=OUTPUT/"motion_showcase.mp4"
    run(["ffmpeg","-y","-hide_banner","-loglevel","error","-f","concat","-safe","0","-i",str(concat),"-c","copy",str(showcase)])

    # One representative frame per motion, arranged into a lightweight sheet.
    thumbs=OUTPUT/"thumbs"; thumbs.mkdir(parents=True,exist_ok=True)
    for item in index:
        run(["ffmpeg","-y","-hide_banner","-loglevel","error","-ss","1.1","-i",str(ROOT/item["preview"]),"-frames:v","1","-vf","scale=240:135",str(thumbs/f"{item['semantic']}.png")])
    files=sorted(thumbs.glob("*.png")); inputs=[]; chains=[]; labels=[]
    for i,path in enumerate(files):
        inputs.extend(["-i",str(path)]); chains.append(f"[{i}:v]pad=240:135:(ow-iw)/2:(oh-ih)/2:black[v{i}]"); labels.append(f"[v{i}]")
    layout="|".join(f"{(i%5)*240}_{(i//5)*135}" for i in range(len(files)))
    graph=";".join(chains)+";"+"".join(labels)+f"xstack=inputs={len(files)}:layout={layout}:fill=black"
    run(["ffmpeg","-y","-hide_banner","-loglevel","error",*inputs,"-filter_complex",graph,"-frames:v","1",str(OUTPUT/"contact_sheet.png")])


def build_browser(index:list[dict]) -> None:
    rows=[]
    for item in index:
        root=item.get("root_path",[]); start=root[0] if root else [0,0,0]; end=root[-1] if root else [0,0,0]
        rows.append(f"""<article data-category="{html.escape(item['category'])}" data-state="{html.escape(item['approval_state'])}">
<h2>{html.escape(item['semantic'].replace('_',' ').title())}</h2><video controls loop preload="metadata" src="../{html.escape(Path(item['preview']).relative_to('output').as_posix())}"></video>
<p><b>{html.escape(item['category'])}</b> · {item['duration']:.2f}s · seed {item['seed']} · <span class="state">{html.escape(item['approval_state'])}</span></p>
<p>{html.escape(item['prompt'])}</p><details><summary>Motion diagnostics</summary><pre>root {start} → {end}\nframes {item['validation']['frame_count']}\ncontact drift {item['validation']['contact_drift']:.5f}\nconstraints {html.escape(json.dumps(item['constraints']))}</pre></details></article>""")
    document=f"""<!doctype html><meta charset="utf-8"><title>Infinite Backlot Motion Lab</title><style>
body{{font:15px Segoe UI;background:#08101d;color:#e8f0ff;margin:0}}header{{position:sticky;top:0;background:#101d31;padding:16px;z-index:2}}main{{display:grid;grid-template-columns:repeat(auto-fit,minmax(360px,1fr));gap:16px;padding:16px}}article{{background:#132238;border:1px solid #29425f;border-radius:12px;padding:14px}}video{{width:100%;background:#000}}.state{{color:#ffb14a}}button,select{{margin-right:8px;padding:7px}}pre{{white-space:pre-wrap}}</style>
<header><b>Canonical SOMA Motion Lab</b> <select id="category"><option value="">all categories</option><option>locomotion</option><option>conversation</option><option>reaction</option><option>staging</option></select> <select id="state"><option value="">all states</option><option>generated</option><option>usable</option><option>needs_adjustment</option><option>rejected</option></select> <button onclick="document.querySelectorAll('video').forEach(v=>v.pause())">pause all</button><small> Native SOMA skin · video controls provide loop and scrub · diagnostics expose root/contact data.</small></header><main>{''.join(rows)}</main><script>
function filter(){{let c=category.value,s=state.value;document.querySelectorAll('article').forEach(x=>x.hidden=(c&&x.dataset.category!=c)||(s&&x.dataset.state!=s))}}category.onchange=filter;state.onchange=filter;</script>"""
    (OUTPUT/"index.html").write_text(document,encoding="utf-8")
    (OUTPUT/"motion_index.json").write_text(json.dumps(index,indent=2),encoding="utf-8")


def main()->int:
    parser=argparse.ArgumentParser(); parser.add_argument("--skip-render",action="store_true"); parser.add_argument("--face-stride",type=int,default=6); args=parser.parse_args()
    index=json.loads(INDEX_PATH.read_text(encoding="utf-8")); OUTPUT.mkdir(parents=True,exist_ok=True)
    if not args.skip_render: render_previews(index,args.face_stride)
    build_showcase(index); build_browser(index)
    print(f"motion showcase complete: {OUTPUT/'motion_showcase.mp4'}")
    print(f"browser: uv run --no-project python -m http.server 8765 --directory {OUTPUT}")
    return 0


if __name__=="__main__": raise SystemExit(main())
