#!/usr/bin/env bash
# TryingOpen2API CD deploy script (runs on server)
set -euo pipefail
export PATH=$PATH:/root/.cargo/bin

echo "[1/5] extract source"
cd /root
rm -rf tryingopen-2api-cd
mkdir -p tryingopen-2api-cd
tar -xzf /root/tryingopen2api_src.tar.gz -C tryingopen-2api-cd

echo "[2/5] start build (nice, background)"
cd tryingopen-2api-cd
rm -f /root/cd_build.log
nohup nice -n 10 cargo build --release > /root/cd_build.log 2>&1 &
BUILD_PID=$!
echo "BUILD_PID=$BUILD_PID"

echo "[3/5] wait build (max 15min)"
for i in $(seq 1 90); do
  if [ -f /root/tryingopen-2api-cd/target/release/tryingopen2api ]; then
    if ! kill -0 "$BUILD_PID" 2>/dev/null; then
      echo "BUILD_READY after ${i}0s"
      break
    fi
  fi
  sleep 10
done
if [ ! -f /root/tryingopen-2api-cd/target/release/tryingopen2api ]; then
  echo "BUILD_TIMEOUT"
  tail -30 /root/cd_build.log
  exit 1
fi

echo "[4/5] deploy"
BIN=/root/tryingopen-2api-cd/target/release/tryingopen2api
ls -la "$BIN"
systemctl stop tryingopen2api
sleep 1
mkdir -p /opt/tryingopen2api/data /opt/tryingopen2api/bin
cp -f "$BIN" /opt/tryingopen2api/bin/tryingopen2api
chmod +x /opt/tryingopen2api/bin/tryingopen2api
systemctl start tryingopen2api
sleep 5

echo "[5/5] verify"
curl -sS -m 10 http://127.0.0.1:47831/healthz
echo "CD_DEPLOY_OK"
