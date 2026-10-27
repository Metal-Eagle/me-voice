#!/usr/bin/env bash
# Exercises the packaged tray app without a microphone or a downloaded speech model.
set -euo pipefail
cd "$(dirname "$0")/.."
BIN="$PWD/dist/me-voice.app/Contents/MacOS/me-voice"
export ME_VOICE_HOME
ME_VOICE_HOME="$(mktemp -d -t me-voice-smoke)"
export ME_VOICE_DEV=0
PID=""
cleanup() {
    if [[ -n "$PID" ]]; then
        kill "$PID" 2>/dev/null || true
        wait "$PID" 2>/dev/null || true
    fi
    rm -rf "$ME_VOICE_HOME"
}
trap cleanup EXIT

mkdir -p "$ME_VOICE_HOME/config" "$ME_VOICE_HOME/data/models"
printf '%s' '{"modelId":"smoke","autoUpdate":false}' > "$ME_VOICE_HOME/config/settings.json"
printf '%s' '{"models":[{"id":"smoke","name":"Smoke fixture","file":"smoke.bin","sha256":null,"languages":["en"],"acft":false}]}' > "$ME_VOICE_HOME/config/models.json"
touch "$ME_VOICE_HOME/data/models/smoke.bin"

start() {
    "$BIN" > "$ME_VOICE_HOME/output.log" 2>&1 &
    PID=$!
    for _ in {1..100}; do
        if ! kill -0 "$PID" 2>/dev/null; then
            cat "$ME_VOICE_HOME/output.log"
            echo "Packaged app exited before initialization" >&2
            exit 1
        fi
        if grep -q "app initialized" "$ME_VOICE_HOME/output.log"; then
            return
        fi
        sleep 0.1
    done
    cat "$ME_VOICE_HOME/output.log"
    echo "Packaged app did not initialize within 10 seconds" >&2
    exit 1
}

quit() {
    "$BIN" quit
    for _ in {1..100}; do
        if ! kill -0 "$PID" 2>/dev/null; then
            wait "$PID"
            PID=""
            return
        fi
        sleep 0.1
    done
    echo "Packaged app did not quit within 10 seconds" >&2
    exit 1
}

start
quit
start
kill -KILL "$PID"
wait "$PID" 2>/dev/null || true
PID=""
start
quit
echo "Packaged macOS app starts, quits, and restarts after a forced exit."
