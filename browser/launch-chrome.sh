#!/usr/bin/env bash
# Chrome 136+ ignores --remote-debugging-port on your real profile.
# Open the inspect page so you can turn Remote debugging ON (Chrome 144+).
set -euo pipefail

echo "Chrome 136+ blocks port 9222 on your normal profile." >&2
echo "Opening chrome://inspect/#remote-debugging — turn Remote debugging ON." >&2
echo "Then in claw run /browser and click Allow if Chrome asks." >&2

if command -v open >/dev/null 2>&1; then
  open -a "Google Chrome" "chrome://inspect/#remote-debugging"
  exit 0
fi

echo "Open this URL in Google Chrome:" >&2
echo "  chrome://inspect/#remote-debugging" >&2
exit 0
