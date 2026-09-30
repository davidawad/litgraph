# Brief: save both motion pieces as videos (bead lg-ks7)

1. `git mv marketing/engine/litgraph.mp4 docs/media/litgraph-engine.mp4`.
2. Record `marketing/graph/index.html` to `docs/media/litgraph-graph.mp4`:
   1920x1080, H.264 (yuv420p, `+faststart`), exactly one 42 s loop, starting at
   t=0. The page supports `#t=<seconds>` to open paused at a frame, and has
   play/replay controls; read its script to drive it deterministically.
   - Serve the file over a local HTTP server (Google Fonts must load; wait for
     `document.fonts.ready` before recording).
   - Use Playwright (Chromium). If there is no system ffmpeg or H.264 encoder,
     get one without root: `pip install imageio-ffmpeg` provides a static ffmpeg
     binary (`python -c "import imageio_ffmpeg; print(imageio_ffmpeg.get_ffmpeg_exe())"`).
     Prefer frame-accurate capture (step the animation clock and screenshot at
     30 fps, then encode) over real-time screen recording if the page's timeline
     can be driven; otherwise record Playwright webm and transcode.
   - Target size under ~10 MB (CRF ~23-28).
3. Also export a 1280x720 poster frame for each video as PNG
   (`docs/media/litgraph-graph.png`, `docs/media/litgraph-engine.png`), taken
   from the most representative frame.
4. Update `marketing/graph/NOTES.md` and `marketing/engine/NOTES.md` to point at
   the new video paths, and add `docs/media/README.md` (one line per file).
5. Verify with ffprobe (duration, resolution, codec) and put that output in
   `docs/media/README.md`.
6. Commit on this branch (`docs(media): ...`). Do not push, merge, or close
   anything. Delete this BRIEF.md in the same commit.
