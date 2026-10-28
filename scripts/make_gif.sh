#!/bin/sh
# Turn a screen recording into a README-sized GIF.
#
# Usage:  scripts/make_gif.sh recording.mov assets/gifs/play-together.gif [start] [duration]
#   start, duration: optional trim in seconds (e.g. 2.5 12)
#
# 960 px wide, 12 fps, one palette per clip. Aim for under 5 MB so the
# README loads fast; if it's bigger, trim the clip or lower FPS/WIDTH.
set -e
IN="$1"; OUT="$2"
[ -n "$IN" ] && [ -n "$OUT" ] || { echo "usage: $0 input.mov output.gif [start] [duration]"; exit 1; }
FPS="${FPS:-12}"; WIDTH="${WIDTH:-960}"
TRIM=""
[ -n "$3" ] && TRIM="-ss $3"
[ -n "$4" ] && TRIM="$TRIM -t $4"
mkdir -p "$(dirname "$OUT")"
ffmpeg -v error -y $TRIM -i "$IN" -vf \
  "fps=$FPS,scale=$WIDTH:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle" \
  "$OUT"
ls -lh "$OUT" | awk '{print $5, $9}'
