"""Assemble the single budgeted world-expansion-3 showcase from real Blender previews."""
from __future__ import annotations
import json
import subprocess
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
PRE=ROOT/"assets/reference/world-expansion-3/hero_previews"
OUT=ROOT/"output/world-expansion-3"
SEGMENTS=[
 ("location_odd_hours_v3","ODD HOURS / CC0 CASH REGISTER + STORE KIT"),
 ("location_apartment_lobby_v3","APARTMENT LOBBY / NAVIGATION-READY DRESSING"),
 ("location_transit_pocket_v3","TRANSIT POCKET / CC0 BENCH + STREET LAMP"),
 ("location_laundromat_v3","LAUNDROMAT / REUSABLE APPLIANCE KIT"),
 ("location_maintenance_workshop_v3","MAINTENANCE WORKSHOP / CC0 TOOL + MATERIAL KIT"),
]

def run(cmd):
 p=subprocess.run(cmd,cwd=ROOT,text=True,capture_output=True)
 if p.returncode: raise RuntimeError(p.stderr[-3000:])

def main():
 OUT.mkdir(parents=True,exist_ok=True)
 inputs=[]
 for module,_ in SEGMENTS:inputs += ["-i",str(PRE/f"{module}.png")]
 filters=[]
 for i,(_,label) in enumerate(SEGMENTS):
  safe=label.replace("'","\\'").replace(":","\\:")
  filters.append(f"[{i}:v]zoompan=z='min(zoom+0.00028,1.045)':x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':d=175:s=640x360:fps=25,drawbox=x=0:y=292:w=640:h=68:color=black@0.68:t=fill,drawtext=fontfile='C\\:/Windows/Fonts/arialbd.ttf':text='{safe}':fontcolor=white:fontsize=20:x=24:y=314[v{i}]")
 filters.append("".join(f"[v{i}]" for i in range(len(SEGMENTS)))+f"concat=n={len(SEGMENTS)}:v=1:a=0[outv]")
 run(["ffmpeg","-y",*inputs,"-filter_complex",";".join(filters),"-map","[outv]","-c:v","libx264","-preset","medium","-crf","18","-pix_fmt","yuv420p","-movflags","+faststart",str(OUT/"world_showcase.mp4")])
 # Contact sheet, one frame from each actual Blender preview.
 sheet_inputs=[]
 for module,_ in SEGMENTS:sheet_inputs += ["-i",str(PRE/f"{module}.png")]
 run(["ffmpeg","-y",*sheet_inputs,"-filter_complex","[0:v]scale=320:180[a];[1:v]scale=320:180[b];[2:v]scale=320:180[c];[3:v]scale=320:180[d];[4:v]scale=320:180[e];color=c=#111827:s=320x180[f];[a][b][c][d][e][f]xstack=inputs=6:layout=0_0|320_0|0_180|320_180|0_360|320_360[out]","-map","[out]","-frames:v","1",str(OUT/"contact_sheet.png")])
 catalog=json.loads((ROOT/"assets/world/kits/infinite_backlot_asset_library_v3.catalog.json").read_text())
 provenance=json.loads((ROOT/"assets/world/kits/polyhaven_cc0_intake.provenance.json").read_text())
 catalog["polyhaven_intake"]={"asset_count":len(provenance["assets"]),"manifest":"assets/world/kits/polyhaven_cc0_intake.provenance.json"}
 (OUT/"asset_index.json").write_text(json.dumps(catalog,indent=2)+"\n")
 locations=[]
 for module,label in SEGMENTS:
  side=json.loads((ROOT/f"assets/world/locations/{module}.scene.json").read_text());locations.append({"module_id":module,"label":label,"asset":side["asset"],"source_blend":side["source_blend"],"walkable_regions":len(side["walkable_regions"]),"portals":len(side["portals"]),"colliders":len(side["colliders"]),"interactions":len(side["interactions"]),"preview":side["preview"],"polyhaven_cc0_assets":side["provenance"]["polyhaven_cc0_assets"]})
 (OUT/"location_index.json").write_text(json.dumps({"schema_version":1,"location_count":len(locations),"locations":locations},indent=2)+"\n")
 print(json.dumps({"duration_seconds":35,"locations":len(locations),"assets":catalog["asset_count"],"materials":catalog["material_count"],"output":str(OUT/"world_showcase.mp4")}))
if __name__=="__main__":main()
