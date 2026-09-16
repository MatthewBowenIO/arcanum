#!/usr/bin/env bash
# arcanum — optional capability setup.
#
# arcanum works fully unprivileged except for two things:
#
#   1. xe (Battlemage / newer Arc) utilization + memory counters.
#      The xe driver fills DRM fdinfo cycle counters from the PMU,
#      which needs CAP_PERFMON (or root). Without it, xe GPUs show
#      0% utilization while i915 GPUs work fine.
#
#   2. CPU package power (RAPL energy counters, mode 0400 root:root).
#      Reading these needs CAP_DAC_OVERRIDE — a broad capability that
#      lets the binary read any file. That is why it is opt-in here.
#
# This script adds both capabilities to the arcanum binary.
# They are file capabilities: they survive reboots but are lost if you
# replace/rebuild the binary (just re-run this script).
#
# If you skip this, arcanum still runs: it just shows "needs root"
# for package power and 0% for xe utilization.

set -euo pipefail

BIN="$(readlink -f "$0")"
BIN="$(dirname "$BIN")/../target/release/arcanum"

if [[ ! -x "$BIN" ]]; then
    echo "error: $BIN not found."
    echo "Build first:  cargo build --release"
    exit 1
fi

echo "Granting to $BIN:"
echo "  cap_perfmon       — xe GPU utilization + memory counters"
echo "  cap_dac_override  — RAPL energy counters (CPU package power)"
echo
read -rp "Continue? [y/N] " answer
[[ "$answer" == "y" || "$answer" == "Y" ]] || { echo "Aborted."; exit 0; }

sudo setcap cap_perfmon,cap_dac_override=ep "$BIN"
echo "Done. Verify:  getcap "$BIN""
