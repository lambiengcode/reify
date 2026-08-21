#!/bin/bash
# Evaluate both arms in batches: pull a batch's x86 images, grade both arms on exactly
# those instances, then delete the images. Peak disk stays at one batch, not 200 images.
set -u
SB=/private/tmp/claude-501/-Users-lambiengcode-Documents-reify/025dbc2c-af5b-4475-b2a8-20daab4cac22/scratchpad
cd $SB/swe
BATCH=${BATCH:-10}
python3 - <<'PY' > $SB/swe/all_ids.txt
import json, pathlib
SB = pathlib.Path("/private/tmp/claude-501/-Users-lambiengcode-Documents-reify/025dbc2c-af5b-4475-b2a8-20daab4cac22/scratchpad/swe")
ids = sorted({json.loads(l)["instance_id"] for l in open(SB/"preds-final-reify.jsonl") if l.strip()})
print("\n".join(ids))
PY
total=$(wc -l < $SB/swe/all_ids.txt | tr -d ' ')
echo "instances to grade: $total, batch size $BATCH"
n=0
while read -r batch; do
  n=$((n+1))
  ids=$(echo $batch)
  echo "### batch $n: $ids"
  for iid in $ids; do
    repo="${iid%-*}"; num="${iid##*-}"
    img="swebench/sweb.eval.x86_64.${repo//__/_1776_}-${num}:latest"
    docker pull --platform linux/amd64 "$img" >/dev/null 2>&1 || echo "  pull failed: $iid"
  done
  for arm in reify bm25; do
    $SB/swe/venv/bin/python run_eval_x86.py \
      --dataset_name princeton-nlp/SWE-bench_Verified \
      --predictions_path preds-final-$arm.jsonl \
      --instance_ids $ids \
      --run_id b${n}-$arm --max_workers 4 --namespace swebench --cache_level none 2>&1 \
      | grep -E "Instances (resolved|unresolved|with empty|with errors)" | sed "s/^/  $arm /"
  done
  docker system prune -af >/dev/null 2>&1
done < <(xargs -n $BATCH < $SB/swe/all_ids.txt)
echo "ALL BATCHES DONE"
