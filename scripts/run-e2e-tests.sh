#!/usr/bin/env bash
# Run fileloft-e2e-uppy ignored tests. Starts chromedriver on CHROMEDRIVER_PORT (default 9515)
# when nothing is already listening; tears down only the process we started.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${CHROMEDRIVER_PORT:-9515}"
export WEBDRIVER_URL="${WEBDRIVER_URL:-http://127.0.0.1:${PORT}}"

listening_on_port() {
	# Bash built-in; no lsof/nc required.
	(echo >/dev/tcp/127.0.0.1/"${PORT}") >/dev/null 2>&1
}

started_chromedriver=0
cleanup() {
	if [[ "${started_chromedriver}" -eq 1 ]] && [[ -n "${chromedriver_pid:-}" ]]; then
		kill "${chromedriver_pid}" 2>/dev/null || true
	fi
}
trap cleanup EXIT

if listening_on_port; then
	echo "Using existing listener on ${WEBDRIVER_URL} (chromedriver or other)"
else
	if ! command -v chromedriver >/dev/null 2>&1; then
		echo "chromedriver is not in PATH. Install a ChromeDriver whose major version matches Chrome." >&2
		echo "Example: brew install chromedriver   (then allow in Privacy & Security if macOS blocks it)" >&2
		exit 1
	fi
	echo "Starting chromedriver on port ${PORT}..."
	chromedriver --port="${PORT}" &
	chromedriver_pid=$!
	started_chromedriver=1
	# Wait until the port accepts connections (avoids flaky first test).
	attempt=0
	while [ "${attempt}" -lt 50 ]; do
		if listening_on_port; then
			break
		fi
		sleep 0.1
		attempt=$((attempt + 1))
	done
	if ! listening_on_port; then
		echo "chromedriver did not become ready on port ${PORT}" >&2
		exit 1
	fi
fi

cd "${ROOT}"
cargo test --manifest-path "${ROOT}/Cargo.toml" -p fileloft-e2e-uppy -- --ignored "$@"
