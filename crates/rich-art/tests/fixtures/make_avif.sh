#!/bin/sh
# Make halo-32.avif, the AVIF fixture of tests/avif.rs: halo-before.png
# scaled to 32 x 32 (halo-32.png), then that as a still AVIF, AV1 at
# its best quality (crf 0) in 4:4:4 (ffmpeg with libaom).
set -e
cd "$(dirname "$0")"
ffmpeg -y -loglevel error -i halo-before.png -vf scale=32:32 -frames:v 1 halo-32.png
ffmpeg -y -loglevel error -i halo-32.png -frames:v 1 -c:v libaom-av1 -still-picture 1 \
  -crf 0 -pix_fmt yuv444p halo-32.avif
