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
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/testlogs"
export FAKE_NATIVE_ROOT="$fixture"
export BAZEL="$fixture/bazel"
cat >"$BAZEL" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$1" == info ]]; then echo "$FAKE_NATIVE_ROOT/testlogs"; exit 0; fi
printf '%q ' "$@" >>"$FAKE_NATIVE_ROOT/commands.txt"
printf '\n' >>"$FAKE_NATIVE_ROOT/commands.txt"
if [[ "${FAKE_NATIVE_MODE:-}" == failure ]]; then exit 23; fi
name=''
for argument in "$@"; do
  case "$argument" in
    --build_event_json_file=*)
      path="${argument#*=}"
      name="$(basename "$path" .bep.json)"
      if [[ "${FAKE_NATIVE_MODE:-}" != missing-bep ]]; then printf '{}\n' >"$path"; fi ;;
  esac
done
target="${!#}"
relative="${target#//}"
relative="${relative/:/\/}"
relative="${relative#/}"
mkdir -p "$FAKE_NATIVE_ROOT/testlogs/$relative"
printf '%s\n' "$name" >"$FAKE_NATIVE_ROOT/testlogs/$relative/test.xml"
printf '%s\n' "$name" >"$FAKE_NATIVE_ROOT/testlogs/$relative/test.log"
if [[ "$name" == go ]]; then echo '--- PASS: TestNative (0.00s)'; exit 0; fi
count=1
case "$name" in
  *branch_project_open) count=2 ;;
  *-views) count=3 ;;
  abandonment) count=8 ;;
esac
case "${FAKE_NATIVE_MODE:-}" in
  zero) count=0 ;;
  wrong-count) count=9 ;;
esac
printf 'test result: ok. %s passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n' "$count"
MOCK
chmod +x "$BAZEL"
for shard in transactions storage search consumers identities snapshots bindings abandonment; do
  export WINDOWS_NATIVE_RESULTS="$fixture/$shard"
  bash "$root/scripts/run-windows-native-bazel.sh" "$shard" >"$fixture/$shard.log" 2>&1
done
# Repeated filtered invocations must retain their own XML before overwrite.
grep -qx instance-identity-default-views "$fixture/snapshots/testlogs/instance-identity-default-views/test.xml"
grep -qx instance-identity-minimal-views "$fixture/snapshots/testlogs/instance-identity-minimal-views/test.xml"
# Original 25 Rust selections, Go, and the separate daily eight-group selection.
[[ "$(wc -l <"$fixture/commands.txt" | tr -d ' ')" == 27 ]]
grep -q -- '--//:windows_native_profile=facade_all' "$fixture/commands.txt"
grep -q -- '--test_arg=--test-threads=4' "$fixture/commands.txt"
grep -q -- '--test_arg=-test.count=1' "$fixture/commands.txt"
for mode in zero failure missing-bep; do
  export WINDOWS_NATIVE_RESULTS="$fixture/refuse-$mode"
  if FAKE_NATIVE_MODE="$mode" bash "$root/scripts/run-windows-native-bazel.sh" transactions >"$fixture/refuse-$mode.log" 2>&1; then
    echo "native runner accepted $mode" >&2; exit 1
  fi
done
export WINDOWS_NATIVE_RESULTS="$fixture/refuse-count"
if FAKE_NATIVE_MODE=wrong-count bash "$root/scripts/run-windows-native-bazel.sh" snapshots >"$fixture/refuse-count.log" 2>&1; then
  echo 'native runner accepted wrong exact count' >&2; exit 1
fi
export WINDOWS_NATIVE_RESULTS="$fixture/refuse-threads"
if POWER_LOSS_TEST_THREADS=3 bash "$root/scripts/run-windows-native-bazel.sh" abandonment >"$fixture/refuse-threads.log" 2>&1; then
  echo 'native runner accepted unsupported fault concurrency' >&2; exit 1
fi
printf 'PASS | native shard selections and negative controls\n'
