#!/usr/bin/env bash
# 更新 WeKnora Desktop 的 runtime 部署目录(保留 data/ 用户数据与日志)
# 用法: bash scripts/assemble-runtime.sh [WeKnora仓库路径]
set -euo pipefail

DESKTOP_DIR="$(cd "$(dirname "$0")/.." && pwd)"
REPO_DIR="${1:-$(cd "$DESKTOP_DIR/../WeKnora" && pwd)}"

# 优先 linkmind-lite.exe(新名);兼容仓库里仍是旧名 weknora-lite.exe 的构建产物
LITE_EXE="$REPO_DIR/linkmind-lite.exe"
[ -f "$LITE_EXE" ] || LITE_EXE="$REPO_DIR/weknora-lite.exe"
[ -f "$LITE_EXE" ] || { echo "错误: 未找到 lite 二进制(先在 WeKnora 仓库构建: go build -o linkmind-lite.exe ./cmd/server)"; exit 1; }

RUNTIME="$DESKTOP_DIR/runtime"
[ -d "$RUNTIME" ] || mkdir -p "$RUNTIME"
# 若有服务在跑,先结束(数据库/二进制被占用无法替换)
taskkill //IM weknora-lite.exe //F >/dev/null 2>&1 || true
sleep 1

echo ">> 更新 Lite 二进制 ($(basename "$LITE_EXE") -> linkmind-lite.exe)"
cp -f "$LITE_EXE" "$RUNTIME/linkmind-lite.exe"

echo ">> 更新 Web 界面 (web/)"
rm -rf "$RUNTIME/web"
cp -r "$REPO_DIR/web" "$RUNTIME/web"

echo ">> 更新配置 (config/)"
rm -rf "$RUNTIME/config"
cp -r "$REPO_DIR/config" "$RUNTIME/config"
rm -f "$RUNTIME/config/builtin_models.yaml.example" "$RUNTIME/config/models.json.example"

echo ">> 更新 SQLite 迁移 (migrations/sqlite/)"
mkdir -p "$RUNTIME/migrations/sqlite"
cp -rf "$REPO_DIR/migrations/sqlite/." "$RUNTIME/migrations/sqlite/"

echo ">> 更新结巴分词词典 (dict/)"
# gojieba 默认词典路径是编译期写死的模块缓存绝对路径,换机即崩;
# 词典必须随包分发,启动时由桌面端注入 JIEBA_DICT_DIR 指向此目录
GOJIEBA_DICT=$(ls -d "${GOMODCACHE:-$HOME/go/pkg/mod}"/github.com/yanyiwu/gojieba@*/deps/cppjieba/dict 2>/dev/null | tail -1)
if [ -n "$GOJIEBA_DICT" ] && [ -f "$GOJIEBA_DICT/jieba.dict.utf8" ]; then
  rm -rf "$RUNTIME/dict"
  cp -r "$GOJIEBA_DICT" "$RUNTIME/dict"
elif [ ! -f "$RUNTIME/dict/jieba.dict.utf8" ]; then
  echo "警告: 未找到 gojieba 词典且 runtime/dict 不存在,安装到其他电脑将无法启动"
fi
# gomod 缓存中的词典文件自带只读属性,原样复制后,下次 tauri 构建覆盖这些文件会报
# os error 5(access denied)。这里统一去掉只读位。
chmod -R u+w "$RUNTIME/dict" 2>/dev/null || true

echo ""
echo "=== runtime 更新完成 (data/ 已保留) ==="
ls -la "$RUNTIME"
