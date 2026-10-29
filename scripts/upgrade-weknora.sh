#!/usr/bin/env bash
# WeKnora 一键升级脚本(本机开发环境)
# 用法: bash scripts/upgrade-weknora.sh
# 前置: D:\tools 下的工具链(go/mingw64)、Rust、Node 已就绪
set -euo pipefail

REPO=/d/KnowLege/WeKnora
DESKTOP=/d/KnowLege/weknora-desktop

echo "==== [1/6] 拉取 WeKnora 最新代码 ===="
cd "$REPO"
git fetch --depth 1 origin 2>/dev/null || git fetch origin
UPSTREAM=$(git rev-parse --abbrev-ref --symbolic-full-name @{u} 2>/dev/null || echo "origin/main")
git reset --hard "$UPSTREAM"   # 本地无改动,直接对齐远端
git log --oneline -1

echo "==== [2/6] 构建 WeKnora 前端 ===="
cd "$REPO/frontend"
if [ ! -d node_modules ]; then npm ci --no-audit --no-fund; fi
npm run build
cd "$REPO" && rm -rf web && cp -r frontend/dist web

echo "==== [3/6] 检查 anydoc 引擎库 ===="
if [ ! -f "$REPO/third_party/anydoc-go/lib/windows_amd64/libanydoc_go.a" ]; then
  echo "缺少 anydoc 静态库,重新构建(GNU 工具链)..."
  export PATH="/d/tools/mingw64/bin:/c/Users/hly/.cargo/bin:$PATH"
  cd "$REPO/third_party/anydoc-go"
  export CC=x86_64-w64-mingw32-gcc CXX=x86_64-w64-mingw32-g++ AR=x86_64-w64-mingw32-ar
  export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
  cargo build --release --locked --target x86_64-pc-windows-gnu
  mkdir -p lib/windows_amd64
  cp target/x86_64-pc-windows-gnu/release/libanydoc_go.a lib/windows_amd64/
else
  echo "anydoc 库已存在,跳过"
fi

echo "==== [4/6] 构建 WeKnora Lite 二进制 ===="
cd "$REPO"
export PATH="/d/tools/go/bin:/d/tools/mingw64/bin:$PATH"
export GOPATH=/d/tools/gopath GOMODCACHE=/d/tools/gopath/mod GOCACHE=/d/tools/gocache
export GOTMPDIR=/d/tmp TMP=/d/tmp TEMP=/d/tmp
export CGO_ENABLED=1 EDITION=lite
export CGO_CFLAGS="-Wno-deprecated-declarations -ID:/tools/sqlite-include"
export LDFLAGS="-w -s $(bash ./scripts/get_version.sh ldflags)"
go build -tags "sqlite_fts5 anydoc" -ldflags="$LDFLAGS -extldflags=-lntdll" -o weknora-lite.exe ./cmd/server
ls -la weknora-lite.exe

echo "==== [5/6] 更新部署目录(保留 data/) ===="
cd "$DESKTOP"
bash scripts/assemble-runtime.sh

echo "==== [6/6] 更新桌面正式版 exe ===="
cd "$DESKTOP/src-tauri" && cargo build --release --features custom-protocol 2>&1 | tail -1
cp -f target/release/weknora-desktop.exe "$DESKTOP/portable/WeKnoraDesktop.exe"

echo ""
echo "==== 升级完成 ===="
echo "现在: 关闭正在运行的 WeKnora Desktop,重新双击桌面快捷方式即可。"
echo "(服务首次启动会自动执行数据库迁移)"
