#!/usr/bin/env bash
# Warn (non-blocking) when docs/ui-parity.md sections have gone stale.
#
# A section is stale if its "Last verified: YYYY-MM-DD" line is older than
# STALE_DAYS (default 60). Prints one line per stale section and exits 0
# so CI treats this as a nudge, not a gate.

set -eu

DOC="${1:-docs/ui-parity.md}"
STALE_DAYS="${STALE_DAYS:-60}"

if [ ! -f "$DOC" ]; then
  echo "ui-parity-check: $DOC not found" >&2
  exit 0
fi

today_epoch=$(date +%s)
stale_seconds=$((STALE_DAYS * 86400))

current_section=""
stale_count=0

while IFS= read -r line; do
  case "$line" in
    "## "*)
      current_section="${line### }"
      ;;
    "**Last verified**"*)
      # Extract the first YYYY-MM-DD in the line.
      date_str=$(printf '%s\n' "$line" | grep -oE '[0-9]{4}-[0-9]{2}-[0-9]{2}' | head -n1 || true)
      if [ -z "$date_str" ]; then
        continue
      fi
      # macOS date -j vs GNU date -d.
      if verified_epoch=$(date -j -f "%Y-%m-%d" "$date_str" +%s 2>/dev/null); then
        :
      elif verified_epoch=$(date -d "$date_str" +%s 2>/dev/null); then
        :
      else
        echo "ui-parity-check: could not parse date '$date_str' in section '$current_section'" >&2
        continue
      fi
      age=$((today_epoch - verified_epoch))
      if [ "$age" -gt "$stale_seconds" ]; then
        age_days=$((age / 86400))
        printf 'ui-parity: %s stale — last verified %s (%d days ago, threshold %d)\n' \
          "$current_section" "$date_str" "$age_days" "$STALE_DAYS"
        stale_count=$((stale_count + 1))
      fi
      ;;
  esac
done < "$DOC"

if [ "$stale_count" -eq 0 ]; then
  echo "ui-parity: all sections fresh (threshold ${STALE_DAYS} days)"
fi

exit 0
