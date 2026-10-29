#!/usr/bin/env bash
# 组装 docreader 文档解析服务到 runtime/docreader/(便携/U盘布局,免装 Python)
#
# 产出布局(与 src-tauri/src/service.rs 的 ensure_docreader 便携检测对应):
#   runtime/docreader/
#     python/          # python-build-standalone CPython 3.13 整体拷贝(可搬移),
#                      # 依赖直接装入 python/Lib/site-packages(site.py 自动发现)
#     docreader/       # WeKnora 仓库 docreader 包源码(python -m docreader.main)
#
# 用法: bash scripts/assemble-docreader.sh [WeKnora仓库路径]
set -euo pipefail

DESKTOP_DIR="$(cd "$(dirname "$0")/.." && pwd)"
REPO_DIR="${1:-$(cd "$DESKTOP_DIR/../WeKnora" && pwd)}"
SRC_DIR="$REPO_DIR/docreader"

[ -f "$SRC_DIR/pyproject.toml" ] || { echo "错误: 未找到 $SRC_DIR/pyproject.toml"; exit 1; }
command -v uv >/dev/null || { echo "错误: 未安装 uv (https://docs.astral.sh/uv)"; exit 1; }

RUNTIME="$DESKTOP_DIR/runtime"
DOCREADER_OUT="$RUNTIME/docreader"
PY_OUT="$DOCREADER_OUT/python"
STAGE="$(mktemp -d)"

cleanup() { rm -rf "$STAGE"; }
trap cleanup EXIT

echo ">> 定位 Python 3.13 (uv 管理, python-build-standalone)"
# uv python find 返回 python.exe 文件路径,取其所在安装目录
PY_SRC="$(dirname "$(uv python find 3.13)")"
echo "   源: $PY_SRC"

echo ">> 同步 docreader 依赖 (uv.lock 冻结) -> $STAGE/venv"
(cd "$SRC_DIR" && UV_PROJECT_ENVIRONMENT="$STAGE/venv" uv sync --frozen --no-install-project --quiet)
[ -d "$STAGE/venv/Lib/site-packages" ] || { echo "错误: 依赖同步失败"; exit 1; }

echo ">> 组装 $DOCREADER_OUT"
# 全量重建;脚本不碰 runtime/data,用户数据安全
rm -rf "$DOCREADER_OUT"
mkdir -p "$DOCREADER_OUT"

echo ">> 拷贝 Python 运行时 -> python/"
cp -r "$PY_SRC" "$PY_OUT"
# 运行时用不到的开发产物,省 ~20MB
rm -rf "$PY_OUT/include" "$PY_OUT/libs" "$PY_OUT/tcl" "$PY_OUT/Scripts" "$PY_OUT/pythonw.exe"

echo ">> 安装依赖到 python/Lib/site-packages/"
mkdir -p "$PY_OUT/Lib/site-packages"
cp -r "$STAGE/venv/Lib/site-packages/." "$PY_OUT/Lib/site-packages/"

echo ">> 拷贝 docreader 源码 -> docreader/"
mkdir -p "$DOCREADER_OUT/docreader"
(cd "$SRC_DIR" && tar cf - \
    --exclude='__pycache__' --exclude='.venv' --exclude='.pytest_cache' \
    --exclude='testdata' --exclude='tests' --exclude='scripts' \
    --exclude='Makefile' --exclude='.pylintrc' --exclude='uv.lock' \
    .) | tar xf - -C "$DOCREADER_OUT/docreader"

# gomod/uv 缓存常带只读位,下次 tauri 构建覆盖会报 os error 5
chmod -R u+w "$DOCREADER_OUT" 2>/dev/null || true

SIZE_MB=$(du -sm "$DOCREADER_OUT" | cut -f1)
echo ""
echo "=== docreader 运行时组装完成 (共 ${SIZE_MB}MB) ==="
ls -la "$DOCREADER_OUT"
echo ""
echo "自检: 导入关键依赖"
"$PY_OUT/python.exe" -c "import grpc, pandas, PIL, pypdf, pydantic, trafilatura, playwright; print('依赖导入 OK')" || {
  echo "警告: 依赖导入自检失败,便携包将无法解析文档"; exit 1
}
