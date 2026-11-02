#!/usr/bin/env bash
set -euo pipefail
shopt -s inherit_errexit

best_effort() {
  "$@" || return 0
}

chmod_best_effort() {
  chmod +x -- "${exe}" 2>>"${log}" || return 0
}

record_failure_best_effort() {
  printf '%s\n' 'update apply failed' >>"${log}" || return 0
}

pid=${1}
archive=${2}
staging=${3}
install=${4}
exe=${5}
log=${6}
extracted=${staging}/extracted
i=0
while kill -0 "${pid}" 2>/dev/null; do
  i=$((i + 1))
  if [[ ${i} -ge 120 ]]; then
    break
  fi
  best_effort sleep 1
done
best_effort mkdir -p -- "${extracted}"
if tar -xzf "${archive}" -C "${extracted}" 2>>"${log}" \
  && cp -R -- "${extracted}/." "${install}/" 2>>"${log}"; then
  chmod_best_effort
else
  record_failure_best_effort
fi
cd "${install}" && nohup "${exe}" >/dev/null 2>>"${log}" &
rm -rf -- "${staging}"
