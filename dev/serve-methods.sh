#!/bin/bash
# Serve the live caption methods under evaluation, one socket each, where
# Myna Captions lists them as engines. Ctrl+C stops them all.
#
#   dev/serve-methods.sh [method ...]     default: every method
#
# Methods: nemotron-onnx (English) and nemotron35-onnx (French and 17 more
# locales; set the language in the app, or it detects), Myna's torch-free
# adapter on the exports in NEMOTRON_DIR and NEMOTRON35_DIR (see dev/nemotron/
# in MYNA_TREE); nemotron-560, nemotron-1120 and nemotron35-560, the same
# models under torch; and parakeet-bounded
#
# MYNA_TREE is a Myna checkout carrying the captions-live branch (Parakeet's
# bounded window and the Nemotron server). NEMO_VENV holds NeMo and torch,
# kept out of Myna's own lock; it is built on first use, from the uv cache
# where it can be.
set -euo pipefail
MYNA_TREE=${MYNA_TREE:-$HOME/Projects/myna-captions-live}
NEMO_VENV=${NEMO_VENV:-$HOME/.cache/myna/captions-nemo-venv}
if [ ! -x "$NEMO_VENV/bin/python" ]; then
    uv venv -q -p 3.12 "$NEMO_VENV"
    VIRTUAL_ENV=$NEMO_VENV uv pip install -q "nemo_toolkit[asr]==3.0.0" "torch==2.14.0" \
        "websockets>=15" --index-strategy unsafe-best-match \
        --extra-index-url https://download.pytorch.org/whl/cpu
fi
HF=$HOME/.cache/huggingface/hub
NEMO_MODEL=${NEMO_MODEL:-$(ls "$HF"/models--nvidia--nemotron-speech-streaming-en-0.6b/snapshots/*/*.nemo)}
NEMO35_MODEL=${NEMO35_MODEL:-$(ls "$HF"/models--nvidia--nemotron-3.5-asr-streaming-0.6b/snapshots/*/*.nemo 2>/dev/null || true)}
NEMOTRON_DIR=${NEMOTRON_DIR:-$HOME/.cache/myna/models/nemotron-en-rc6}
NEMOTRON35_DIR=${NEMOTRON35_DIR:-$HOME/.cache/myna/models/nemotron35-rc6}
DIR=${XDG_RUNTIME_DIR:?}/myna-captions
mkdir -p "$DIR"
methods=("$@")
[ ${#methods[@]} -gt 0 ] || methods=(nemotron-onnx nemotron35-onnx parakeet-bounded)

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
    nemotron-onnx | nemotron35-onnx)
        dir=$NEMOTRON_DIR label="Nemotron English, ONNX int8 (commits ~0.3 s)"
        [ "$m" = nemotron35-onnx ] && dir=$NEMOTRON35_DIR \
            label="Nemotron multilingual 3.5, ONNX int8 (French and more)"
        echo "$label" > "$DIR/$m.label"
        "$MYNA_TREE/server/.venv/bin/myna-server" --adapter nemotron --model "$dir" \
            --preload --socket "$DIR/$m.sock" &
        ;;
    nemotron-1120 | nemotron-560)
        rc=13 label="Nemotron English, 1.12 s chunks (commits ~0.7 s)"
        [ "$m" = nemotron-560 ] && rc=6 label="Nemotron English, 0.56 s chunks (commits ~0.3 s)"
        echo "$label" > "$DIR/$m.label"
        "$NEMO_VENV/bin/python" "$MYNA_TREE/dev/captions_eval/nemotron_server.py" \
            "$NEMO_MODEL" "$DIR/$m.sock" --right-context "$rc" &
        ;;
    nemotron35-560)
        [ -n "$NEMO35_MODEL" ] || { echo "nemotron-3.5-asr-streaming-0.6b is not downloaded" >&2; exit 2; }
        echo "Nemotron multilingual 3.5, 0.56 s chunks (French and more)" > "$DIR/$m.label"
        "$NEMO_VENV/bin/python" "$MYNA_TREE/dev/captions_eval/nemotron_server.py" \
            "$NEMO35_MODEL" "$DIR/$m.sock" --right-context 6 --language auto &
        ;;
    *) echo "unknown method $m" >&2; exit 2 ;;
    esac
    pids+=($!)
done
echo "serving ${methods[*]} under $DIR"
wait
