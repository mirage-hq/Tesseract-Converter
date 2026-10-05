#!/usr/bin/env bash
# Diagnostic only; the scorer owns the strict pass/fail policy.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
output="${1:?usage: pair-prproj-evidence.sh <artifacts-dir>}"
python3 - "$root/tests/manifest.json" <<'PY' > "$output/references.tsv"
import json
import sys
for case in json.load(open(sys.argv[1]))['cases']:
    if case['proof'] == 'video_reference':
        print(f"{case['id']}\t{case['reference_video']['repo_path']}")
PY
# Letterbox each side so vertical canvases keep their aspect ratio.
fit='scale=960:540:force_original_aspect_ratio=decrease,pad=960:540:(ow-iw)/2:(oh-ih)/2'
while IFS=$'\t' read -r case_id repo_path; do
  reference="$root/$repo_path"
  actual="$output/$case_id/tesseract.mp4"
  [[ -f "$actual" && -f "$reference" ]] || continue
  case "$case_id" in
    *quicktime*) time=0.5 ;;
    *adjacent_cut|*distinct_media_gap|*repeated_adjacent_source|premiere_two_video_tracks) time=2.5 ;;
    *) time=1.5 ;;
  esac
  ffmpeg -hide_banner -loglevel error -ss "$time" -i "$reference" \
    -ss "$time" -i "$actual" \
    -filter_complex "[0:v]${fit}[left];[1:v]${fit}[right];[left][right]hstack=inputs=2" \
    -frames:v 1 -y "$output/$case_id/adobe-left_tesseract-right.png"
done < "$output/references.tsv"
