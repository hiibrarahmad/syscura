"""Turns the 4K stills from record.mjs into the LinkedIn video.

Each still gets a slow zoom toward its focus point (rendered from the 4K
source, so text stays sharp), scenes are joined with short crossfades, and
captions fade in over the scenes that share them.
"""
import json
import pathlib
import subprocess

HERE = pathlib.Path(__file__).parent
FR = HERE / "frames"
CLIPS = HERE / "clips"
CLIPS.mkdir(exist_ok=True)
FPS = 30
W, H = 1920, 1080
FADE = 0.35   # crossfade between scenes
CUT = 0.12    # quick blend for the click moment

scenes = json.loads((HERE / "scenes.json").read_text(encoding="utf-8"))


def run(args):
    r = subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", *args])
    if r.returncode:
        raise SystemExit(f"ffmpeg failed: {' '.join(map(str, args))}")


# 1. One clip per still.
for i, s in enumerate(scenes):
    n = round(s["dur"] * FPS)
    z0, z1 = s["zoom"]
    fx, fy = s["focus"]
    z = f"{z0}+({z1}-{z0})*on/{max(n - 1, 1)}"
    # Bring the focus point toward the middle of the frame (a little above,
    # since captions sit at the bottom), without leaving the picture.
    x = f"clip({fx}*iw-iw/zoom/2,0,iw-iw/zoom)"
    y = f"clip({fy}*ih-ih/zoom*0.44,0,ih-ih/zoom)"
    run([
        "-i", FR / f"{s['img']}.png",
        "-vf", f"zoompan=z='{z}':x='{x}':y='{y}':d={n}:s={W}x{H}:fps={FPS},format=yuv420p",
        "-frames:v", str(n), "-c:v", "libx264", "-crf", "12", "-preset", "medium", CLIPS / f"{i:02d}.mp4",
    ])

# 2. Timeline: when each scene starts once crossfades overlap them.
starts, t = [], 0.0
for i, s in enumerate(scenes):
    starts.append(t)
    if i + 1 < len(scenes):
        t += s["dur"] - (CUT if scenes[i + 1].get("cut") else FADE)
total = t + scenes[-1]["dur"]

# 3. Captions: one span per run of scenes with the same caption.
spans = []
for i, s in enumerate(scenes):
    cap = s.get("cap")
    if not cap:
        continue
    end = starts[i] + s["dur"]
    if spans and spans[-1][0] == cap and spans[-1][2] >= starts[i] - 0.01:
        spans[-1][2] = end
    else:
        spans.append([cap, starts[i], end])

# 4. Join clips with crossfades, then lay the captions on top.
inputs, filters = [], []
for i in range(len(scenes)):
    inputs += ["-i", CLIPS / f"{i:02d}.mp4"]
prev = "[0:v]"
for i in range(1, len(scenes)):
    d = CUT if scenes[i].get("cut") else FADE
    off = starts[i]
    out = f"[x{i}]"
    filters.append(f"{prev}[{i}:v]xfade=transition=fade:duration={d}:offset={off:.3f}{out}")
    prev = out
base = len(scenes)
for j, (cap, a, b) in enumerate(spans):
    inputs += ["-loop", "1", "-t", f"{total:.3f}", "-i", FR / f"{cap}.png"]
    k = base + j
    a_in, b_out = a + 0.25, b - 0.45
    filters.append(
        f"[{k}:v]scale={W}:{H},format=rgba,"
        f"fade=t=in:st={a_in:.3f}:d=0.3:alpha=1,fade=t=out:st={b_out:.3f}:d=0.3:alpha=1[c{j}]"
    )
    out = f"[o{j}]"
    filters.append(f"{prev}[c{j}]overlay=0:0:enable='between(t,{a_in:.3f},{b_out + 0.3:.3f})'{out}")
    prev = out
filters.append(f"{prev}format=yuv420p[v]")

final = HERE / "syscura-demo.mp4"
run([
    *inputs, "-filter_complex", ";".join(filters), "-map", "[v]",
    "-t", f"{total:.3f}", "-r", str(FPS), "-c:v", "libx264", "-crf", "17", "-preset", "slow",
    "-profile:v", "high", "-pix_fmt", "yuv420p", "-movflags", "+faststart", final,
])
print(f"{final} · {total:.1f} s · {len(scenes)} scenes · {len(spans)} captions")
