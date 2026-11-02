#!/bin/bash
# Serve the live caption methods under evaluation, one socket each, where
# Myna Captions lists them as engines. Ctrl+C stops them all.
#
#   dev/serve-methods.sh [method ...]     default: every method
#
# Methods: parakeet-bounded, nemotron-1120, nemotron-560
#
# MYNA_TREE is a Myna checkout carrying the captions-live branch (Parakeet's
# bounded window and the Nemotron server); NEMO_VENV a Python environment
# with nemo_toolkit[asr] and torch, kept out of Myna's own lock.
set -euo pipefail
MYNA_TREE=${MYNA_TREE:-$HOME/Projects/myna-captions-live}
NEMO_VENV=${NEMO_VENV:?set NEMO_VENV to a venv with nemo_toolkit[asr]==3.0.0 and torch}
NEMO_MODEL=${NEMO_MODEL:-$(ls "$HOME"/.cache/huggingface/hub/models--nvidia--nemotron-speech-streaming-en-0.6b/snapshots/*/*.nemo)}
DIR=${XDG_RUNTIME_DIR:?}/myna-captions
mkdir -p "$DIR"
methods=("$@")
[ ${#methods[@]} -gt 0 ] || methods=(parakeet-bounded nemotron-1120 nemotron-560)

pids=()
cleanup() {
    kill "${pids[@]}" 2>/dev/null || true
    for m in "${methods[@]}"; do rm -f "$DIR/$m.sock" "$DIR/$m.label"; done
}
trap cleanup EXIT INT TERM

for m in "${methods[@]}"; do
    rm -f "$DIR/$m.sock"
    case $m in
    parakeet-bounded)
        echo "Parakeet, bounded window (commits ~3 s)" > "$DIR/$m.label"
        "$MYNA_TREE/server/.venv/bin/myna-server" --adapter parakeet --streaming \
            --stream-strategy local-agreement --stream-cadence-s 1 \
            --preload --socket "$DIR/$m.sock" &
        ;;
    nemotron-1120 | nemotron-560)
        rc=13; [ "$m" = nemotron-560 ] && rc=6
        echo "Nemotron streaming, ${m#nemotron-} ms chunks" > "$DIR/$m.label"
        "$NEMO_VENV/bin/python" "$MYNA_TREE/dev/captions_eval/nemotron_server.py" \
            "$NEMO_MODEL" "$DIR/$m.sock" --right-context "$rc" &
        ;;
    *) echo "unknown method $m" >&2; exit 2 ;;
    esac
    pids+=($!)
done
echo "serving ${methods[*]} under $DIR"
wait
