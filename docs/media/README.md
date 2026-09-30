# docs/media

- `litgraph-graph.mp4`: "The Docket Graph" (`marketing/graph/index.html`), one 42 s loop, 1920×1080 H.264; see `marketing/graph/NOTES.md`.
- `litgraph-graph.png`: its 1280×720 poster frame (t = 28.2 s, the solved graph with the optimal line).
- `litgraph-engine.mp4`: "Filed in Milliseconds" (`marketing/engine/index.html`), one 45 s loop, 1920×1080 H.264; see `marketing/engine/NOTES.md`.
- `litgraph-engine.png`: its 1280×720 poster frame (t = 1.5 s, the index card).

## ffprobe

```
$ ffprobe -v error -select_streams v:0 -show_entries stream=codec_name,profile,width,height,pix_fmt,r_frame_rate,nb_frames:format=duration,size -of default=nw=1 litgraph-graph.mp4
codec_name=h264
profile=High
width=1920
height=1080
pix_fmt=yuv420p
r_frame_rate=30/1
nb_frames=1260
duration=42.000000
size=2520486
$ ffprobe -v error -select_streams v:0 -show_entries stream=codec_name,profile,width,height,pix_fmt,r_frame_rate,nb_frames:format=duration,size -of default=nw=1 litgraph-engine.mp4
codec_name=h264
profile=High
width=1920
height=1080
pix_fmt=yuv420p
r_frame_rate=30/1
nb_frames=1350
duration=45.000000
size=3619138
```

Both files have `moov` before `mdat` (faststart).
