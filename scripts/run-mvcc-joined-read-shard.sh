#!/usr/bin/env bash
# Copyright 2026 Nowledge
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

set -euo pipefail
binary="${1:?usage: run-mvcc-joined-read-shard.sh <test-binary> [libtest arguments]}"
shift
total="${TEST_TOTAL_SHARDS:-1}"
index="${TEST_SHARD_INDEX:-0}"
if [[ ! "$total" =~ ^[1-8]$ || ! "$index" =~ ^[0-7]$ ]] ||
  (( total > 8 || index >= total )); then
  echo 'invalid joined-read shard coordinates' >&2
  exit 64
fi
if [[ -n "${TEST_SHARD_STATUS_FILE:-}" ]]; then touch "$TEST_SHARD_STATUS_FILE"; fi
inventory="$("$binary" --list)"
names=()
while IFS= read -r entry; do
  case "$entry" in
    *': test') names+=("${entry%: test}") ;;
  esac
done <<<"$inventory"
if (( ${#names[@]} < total )); then
  echo 'joined-read inventory leaves an empty shard' >&2
  exit 1
fi
output="$(mktemp "${TEST_TMPDIR:-${TMPDIR:-/tmp}}/hawdb-joined-read.XXXXXX")"
trap 'rm -f "$output"' EXIT
selected=0
for (( case_index=index; case_index<${#names[@]}; case_index+=total )); do
  name="${names[$case_index]}"
  echo "joined-read shard $index/$total | $name"
  "$binary" "$name" --exact --nocapture "$@" 2>&1 | tee "$output"
  if ! grep -Eq '^test result: ok\. 1 passed; 0 failed; 0 ignored;' "$output"; then
    echo "joined-read case did not pass exactly once: $name" >&2
    exit 1
  fi
  selected=$((selected + 1))
done
echo "joined-read shard $index/$total | $selected cases passed"
