#!/usr/bin/env bash
# 生成自动更新清单 latest.json(tauri-plugin-updater 格式)。
#
# 前置: npm run tauri build 已完成,且构建时设置了签名环境变量:
#   TAURI_SIGNING_PRIVATE_KEY=updater-keys/linkmind.key
#   TAURI_SIGNING_PRIVATE_KEY_PASSWORD=<生成密钥时设置的密码,--ci 生成为空>
#
# 用法:
#   bash scripts/make-update-manifest.sh <版本号> <下载基础URL> [更新说明]
# 示例(GitHub Releases):
#   bash scripts/make-update-manifest.sh 0.1.4 \
#     "https://github.com/zhanglaoweicn/weknora-desktop/releases/download/v0.1.4" \
#     "修复VPN兼容与日志问题;新增应用内自动更新"
# 产物: dist-update/latest.json 与 dist-update/ 内的安装包副本(含 .sig),
#       把整个 dist-upload/ 目录内容上传到发布页/服务器即可。
#
# 注意: 上传到 GitHub Releases 时请把安装包文件名中的空格改为点号或横杠
# (GitHub 会把空格规范化),URL 与文件名保持一致;本脚本用本地文件名
# 生成 URL,空格会按 %20 编码,自建服务器可直接使用。
set -euo pipefail

VERSION="${1:?用法: make-update-manifest.sh <版本号> <下载基础URL> [更新说明]}"
BASE_URL="${2:?缺少下载基础URL,例如 https://github.com/USER/REPO/releases/download/v0.1.4}"
NOTES="${3:-本次更新包含稳定性修复与体验改进。}"

DESKTOP="$(cd "$(dirname "$0")/.." && pwd)"
NSIS_DIR="$DESKTOP/src-tauri/target/release/bundle/nsis"
OUT_DIR="$DESKTOP/dist-update"

# 找最新的安装包与签名文件
INSTALLER=$(ls -t "$NSIS_DIR"/*-setup.exe 2>/dev/null | head -1 || true)
[ -n "${INSTALLER:-}" ] || { echo "错误: 未找到安装包 $NSIS_DIR/*-setup.exe,请先执行 npm run tauri build"; exit 1; }
SIG="$INSTALLER.sig"
[ -f "$SIG" ] || { echo "错误: 未找到签名文件 $SIG。请确认构建时设置了 TAURI_SIGNING_PRIVATE_KEY 与 createUpdaterArtifacts"; exit 1; }

FILENAME=$(basename "$INSTALLER")
ENCODED_NAME=${FILENAME// /%20}
DOWNLOAD_URL="${BASE_URL%/}/$ENCODED_NAME"
SIGNATURE=$(tr -d '\n\r' < "$SIG")
PUB_DATE=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

mkdir -p "$OUT_DIR"
cp -f "$INSTALLER" "$OUT_DIR/"
cp -f "$SIG" "$OUT_DIR/"

cat > "$OUT_DIR/latest.json" <<EOF
{
  "version": "$VERSION",
  "notes": "$NOTES",
  "pub_date": "$PUB_DATE",
  "platforms": {
    "windows-x86_64": {
      "signature": "$SIGNATURE",
      "url": "$DOWNLOAD_URL"
    }
  }
}
EOF

echo "==== latest.json 已生成 ===="
cat "$OUT_DIR/latest.json"
echo ""
echo "上传清单: $OUT_DIR/ 内的 $FILENAME、$FILENAME.sig、latest.json"
echo "注意: latest.json 必须能通过 应用设置里的更新源地址(或 tauri.conf 内置端点) 访问到。"
