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

echo "[4/5] deploy (atomic with rollback)"
BIN=/root/tryingopen-2api-cd/target/release/tryingopen2api
ls -la "$BIN"
mkdir -p /opt/tryingopen2api/data /opt/tryingopen2api/bin
# 保留上一版二进制以便回滚
if [ -f /opt/tryingopen2api/bin/tryingopen2api ]; then
  cp -f /opt/tryingopen2api/bin/tryingopen2api /opt/tryingopen2api/bin/tryingopen2api.prev
fi
cp -f "$BIN" /opt/tryingopen2api/bin/tryingopen2api.new
chmod +x /opt/tryingopen2api/bin/tryingopen2api.new
# 原子替换：新版本先就位再启动，失败了回滚旧版
systemctl stop tryingopen2api || true
sleep 1
mv -f /opt/tryingopen2api/bin/tryingopen2api.new /opt/tryingopen2api/bin/tryingopen2api
systemctl start tryingopen2api
sleep 5

echo "[5/5] verify"
if curl -sS -m 10 http://127.0.0.1:47831/healthz; then
  echo "CD_DEPLOY_OK"
else
  echo "CD_DEPLOY_FAILED_HEALTHZ"
  # 回滚到上一版
  if [ -f /opt/tryingopen2api/bin/tryingopen2api.prev ]; then
    systemctl stop tryingopen2api || true
    mv -f /opt/tryingopen2api/bin/tryingopen2api.prev /opt/tryingopen2api/bin/tryingopen2api
    chmod +x /opt/tryingopen2api/bin/tryingopen2api
    systemctl start tryingopen2api
    sleep 3
    curl -sS -m 10 http://127.0.0.1:47831/healthz && echo "CD_ROLLED_BACK" || echo "CD_ROLLBACK_FAILED"
  fi
  exit 1
fi
