#!/bin/sh
# Regenerate the schema documentation from real command output.
#
# Documented shapes drift from code the moment they are written by hand, so they are
# derived from a live index instead. Point REPO at any indexed repository.
set -eu
REPO="${1:-../../fixtures/minierp}"
reify -C "$REPO" --json context "approval for corporate orders" > /tmp/reify-ctx.json
reify -C "$REPO" --json why "SalesOrder.requires_approval"       > /tmp/reify-why.json
reify -C "$REPO" --json impact "requires_approval"               > /tmp/reify-impact.json
reify -C "$REPO" --json preflight "app/order.py"                 > /tmp/reify-pre.json
echo "Now run the shape extractor in docs/json-schema/ to rebuild README.md"
