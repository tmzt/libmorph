#!/usr/bin/env bash
# The S23 check of plan 20261029_02 (audio streams that recover after a
# resume or a lost device), adb only: nothing on the phone is tapped.
#
#   examples/android/capture_resume.sh [<cycles>] [--no-build] [--label <text>]
#
# Builds and installs the audiocapture example (examples/android/build.sh
# audiocapture --install), grants it the microphone, launches it, and then,
# <cycles> times (default 20), takes it away from the front in one of three
# ways, in turn, and brings it back with `am start`:
#
#   home-5s    KEYCODE_HOME, 5 s away;
#   home-60s   KEYCODE_HOME, 60 s away;
#   speech     KEYCODE_HOME, then RECOGNIZE_SPEECH (another app takes the
#              microphone) for 5 s, then KEYCODE_BACK.
#
# After each return it needs, within 5 s, a new `wrote ... frames` line with
# a nonzero frame count and, when the build logs it, `capture_state Running`.
# It records whether a `resume failed`, `lost` or `stalled` line, and a
# `restored` line, appeared in that cycle (from leaving to the next cycle). The whole logcat and a summary go
# to out/audiocapture/YYYYMMDD_NN_capture_resume/ (logcat.txt, summary.txt),
# and an entry is appended to out/audiocapture.md. Exits non-zero if any
# cycle had no samples after its return.
#
# The phone must be unlocked: a lock screen stops the run (exit 3), since
# nothing here unlocks it. While it runs the screen is kept on while plugged
# in (`svc power stayon true`), and the setting is put back at exit.
#
# --no-build reuses the installed APK. --label names the build under test in
# the summary (default: `git describe` of the checkout).
set -euo pipefail

CYCLES=20
BUILD=1
LABEL=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --no-build) BUILD=0; shift ;;
        --label) LABEL="${2:?--label needs a value}"; shift 2 ;;
        [0-9]*) CYCLES="$1"; shift ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
APP_ID="dev.highbay.libmorph.example.audiocapture"
ACTIVITY="$APP_ID/dev.highbay.libmobile.MobileNativeActivity"
[[ -n "$LABEL" ]] || LABEL="$(git -C "$ROOT" describe --tags --always --dirty)"

# ── evidence directory, named like the plans ─────────────────────────────────
DAY="$(date +%Y%m%d)"
AREA="$ROOT/out/audiocapture"
mkdir -p "$AREA"
N=1
while compgen -G "$AREA/${DAY}_$(printf %02d "$N")_*" >/dev/null; do N=$((N + 1)); done
NAME="${DAY}_$(printf %02d "$N")_capture_resume"
OUT="$AREA/$NAME"
mkdir -p "$OUT"
LOG="$OUT/logcat.txt"
SUMMARY="$OUT/summary.txt"

if [[ "$BUILD" == 1 ]]; then
    "$ROOT/examples/android/build.sh" audiocapture --install
fi
# The script cannot unlock the phone: a lock screen ends the run.
locked() { adb shell dumpsys window | grep -q 'isKeyguardShowing=true'; }
adb shell input keyevent KEYCODE_WAKEUP
if locked; then
    echo "error: the phone is locked; unlock it and run again" >&2
    exit 3
fi
# adb's key events do not keep the screen on: keep it on while plugged in
# for the run, then put the setting back.
STAY_ON="$(adb shell settings get global stay_on_while_plugged_in | tr -d '\r')"
adb shell am force-stop "$APP_ID"
adb shell pm grant "$APP_ID" android.permission.RECORD_AUDIO
adb logcat -c
adb logcat -v time -s audiocapture libmobile libmorph >"$LOG" &
LOGCAT=$!
trap 'kill "$LOGCAT" 2>/dev/null || true; adb shell settings put global stay_on_while_plugged_in "${STAY_ON:-0}"' EXIT
adb shell svc power stayon true
adb shell am start -n "$ACTIVITY" >/dev/null

# Lines of the log so far, and how many lines after line $1 match $2.
lines() { wc -l <"$LOG" | tr -d ' '; }
count_after() { tail -n "+$(($1 + 1))" "$LOG" | grep -cE "$2" || true; }
# A `wrote` line with a nonzero frame count.
WROTE='wrote .* \([1-9][0-9]* frames'

echo "==> waiting for two written files"
for _ in $(seq 1 60); do
    [[ "$(count_after 0 "$WROTE")" -ge 2 ]] && break
    sleep 0.5
done
if [[ "$(count_after 0 "$WROTE")" -lt 2 ]]; then
    echo "error: no samples after launch" >&2
    exit 1
fi
# Builds before the fix log no `capture_state`.
STATES=0
grep -q 'capture_state' "$LOG" && STATES=1

WAYS=(home-5s home-60s speech)
FAILED=0
{
    echo "capture_resume: $LABEL, $CYCLES cycles, $(date '+%Y-%m-%d %H:%M %Z')"
    echo "device: $(adb shell getprop ro.product.model | tr -d '\r'), Android $(adb shell getprop ro.build.version.release | tr -d '\r')"
    echo "cycle way samples capture_state output_state resume_failed lost stalled restored event_restored aaudio_error"
} >"$SUMMARY"

for cycle in $(seq 1 "$CYCLES"); do
    way="${WAYS[$(((cycle - 1) % 3))]}"
    adb shell input keyevent KEYCODE_WAKEUP
    if locked; then
        echo "cycle $cycle: the phone is locked; stopping" | tee -a "$SUMMARY" >&2
        FAILED=$((FAILED + CYCLES - cycle + 1))
        break
    fi
    away="$(lines)"
    adb shell input keyevent KEYCODE_HOME
    case "$way" in
        home-5s) sleep 5 ;;
        home-60s) sleep 60 ;;
        speech)
            adb shell am start -a android.speech.action.RECOGNIZE_SPEECH >/dev/null || true
            sleep 5
            adb shell input keyevent KEYCODE_BACK
            sleep 1
            ;;
    esac
    mark="$(lines)"
    adb shell am start -n "$ACTIVITY" >/dev/null
    samples=no
    state=-
    output=-
    for _ in $(seq 1 10); do
        sleep 0.5
        wrote="$(count_after "$mark" "$WROTE")"
        running="$(count_after "$mark" 'capture_state Running')"
        if [[ "$wrote" -ge 1 && ("$STATES" == 0 || "$running" -ge 1) ]]; then
            samples=yes
            break
        fi
    done
    # Let a recovery that is under way finish logging before the next cycle.
    sleep 2
    if [[ "$STATES" == 1 ]]; then
        state="$(tail -n "+$((mark + 1))" "$LOG" | grep -oE 'capture_state [A-Za-z]+' | tail -1 | cut -d' ' -f2)"
        state="${state:--}"
        output="$(tail -n "+$((mark + 1))" "$LOG" | grep -oE 'output_state [A-Za-z]+' | tail -1 | cut -d' ' -f2)"
        output="${output:--}"
    fi
    failed="$(count_after "$away" 'resume failed')"
    lost="$(count_after "$away" 'audio (capture|output) lost')"
    stalled="$(count_after "$away" 'stalled')"
    restored="$(count_after "$away" 'audio (capture|output) restored')"
    event="$(count_after "$away" 'audio: Restored')"
    aaudio="$(count_after "$away" 'libmobile: capture stream:')"
    line="$cycle $way $samples $state $output $failed $lost $stalled $restored $event $aaudio"
    echo "$line" | tee -a "$SUMMARY"
    [[ "$samples" == yes ]] || FAILED=$((FAILED + 1))
done

adb shell input keyevent KEYCODE_HOME
sleep 1
kill "$LOGCAT" 2>/dev/null || true
echo "cycles without samples: $FAILED of $CYCLES" | tee -a "$SUMMARY"

{
    echo
    echo "## $(date '+%Y-%m-%d %H:%M %Z') — capture_resume, $LABEL"
    echo
    echo "\`examples/android/capture_resume.sh $CYCLES\` on the connected phone"
    echo "($(sed -n 2p "$SUMMARY" | cut -d' ' -f2-)): $CYCLES cycles of leaving the"
    echo "audiocapture example (Home for 5 s, Home for 60 s, or Home and the speech"
    echo "recognizer holding the microphone for 5 s) and bringing it back with"
    echo "\`am start\`. Cycles without samples within 5 s of the return: $FAILED."
    echo "Summary: [summary.txt](./audiocapture/$NAME/summary.txt); log:"
    echo "[logcat.txt](./audiocapture/$NAME/logcat.txt)."
} >>"$ROOT/out/audiocapture.md"

echo "==> $OUT"
[[ "$FAILED" == 0 ]]
