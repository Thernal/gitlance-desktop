#!/bin/sh
# motion.sh — damped-spring motion for CSS, in plain sh and awk (nothing else is needed).
#
#   sh motion.sh spring <zeta> <wn> [points]
#       the step response of a damped spring as a CSS linear() easing function.
#       zeta 0.4-0.5 = a pop that overshoots, 0.7-0.8 = stiff, 1 = no overshoot. wn = speed (8-10 settles in one duration).
#   sh motion.sh shrink <big> [zeta] [wn] [points] [name]
#       @keyframes that take scale(<big>) to scale(1) with the spring applied in LOG space.
#       A plain spring over a big scale range overshoots to a negative scale (0.05 -> -4); log space does not.
#   sh motion.sh nudge <name> <period_ms> <ty|scale|squash|rot> <t0:amp[:freq:decay:dur]> ...
#       @keyframes for a looping animation of period <period_ms>: at each event time a damped oscillation
#       (amp, freq in Hz, decay per second, duration in ms) and rest in between. Events in time order, not overlapping.
#       ty = translateY(px), scale = scale(1+v), squash = scale(1+.6v, 1-v), rot = rotate(deg).
#
# Example (the splash of Velino): sh motion.sh nudge o-b1L 6400 ty 0:4.4 3600:3.1
set -eu
cmd="${1:-}"; [ -n "$cmd" ] || { sed -n '2,17p' "$0"; exit 1; }
shift
case "$cmd" in
  spring)
    awk -v z="${1:-.5}" -v w="${2:-9}" -v n="${3:-40}" 'BEGIN {
      wd = w * sqrt(1 - z * z); s = "linear(";
      for (i = 0; i <= n; i++) { t = i / n; y = 1 - exp(-z * w * t) * (cos(wd * t) + (z * w / wd) * sin(wd * t)); if (i == n) y = 1; s = s (i ? ", " : "") sprintf("%g", y) }
      print s ")" }' ;;
  shrink)
    awk -v big="${1:-26}" -v z="${2:-.86}" -v w="${3:-10.5}" -v n="${4:-48}" -v name="${5:-o-shrink}" 'BEGIN {
      wd = w * sqrt(1 - z * z); printf "@keyframes %s { ", name;
      for (i = 0; i <= n; i++) { t = i / n; y = 1 - exp(-z * w * t) * (cos(wd * t) + (z * w / wd) * sin(wd * t)); if (i == n) y = 1;
        printf "%.2f%% { transform: scale(%.4f); } ", t * 100, exp(log(big) * (1 - y)) }
      print "}" }' ;;
  nudge)
    name="${1:?name}"; period="${2:?period_ms}"; fmt="${3:?ty|scale|squash|rot}"; shift 3
    awk -v name="$name" -v period="$period" -v fmt="$fmt" -v ev="$*" 'function out(t, v) {
        if (fmt == "ty") x = sprintf("translateY(%.2fpx)", v); else if (fmt == "scale") x = sprintf("scale(%.3f)", 1 + v);
        else if (fmt == "squash") x = sprintf("scale(%.4f, %.4f)", 1 + .6 * v, 1 - v); else x = sprintf("rotate(%.2fdeg)", v);
        if (t == lastt && started) return; started = 1; lastt = t; printf "%.3f%% { transform: %s; } ", t / period * 100, x }
      BEGIN { started = 0; lastt = -1; pi = atan2(0, -1); printf "@keyframes %s { ", name; out(0, 0);
        n = split(ev, e, " ");
        for (k = 1; k <= n; k++) { m = split(e[k], a, ":"); t0 = a[1]; amp = a[2]; f = (m > 2 ? a[3] : 3.4); d = (m > 3 ? a[4] : 6); dur = (m > 4 ? a[5] : 720);
          for (t = 0; t < dur; t += 24) { s = t / 1000; out(t0 + t, amp * exp(-d * s) * sin(2 * pi * f * s)) }
          out(t0 + dur, 0) }
        out(period, 0); print "}" }' ;;
  *) echo "motion.sh: unknown command '$cmd'" >&2; exit 1 ;;
esac
