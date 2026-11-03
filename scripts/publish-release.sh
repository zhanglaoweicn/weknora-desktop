#!/usr/bin/env bash
# 发布 Release:创建 GitHub Release(已存在则复用)并上传 dist-update/ 资产。
#
# 前置: make-update-manifest.sh 已生成 dist-update/{安装包,.sig,latest.json};
#       git 凭据管理器中已有可推送该仓库的 GitHub 凭据(发布走 REST API)。
#
# 用法: bash scripts/publish-release.sh <tag 如 v0.1.4> [dist目录 默认 dist-update]
set -euo pipefail

TAG="${1:?用法: publish-release.sh <tag> [dist目录]}"
DIST="${2:-dist-update}"
REPO="zhanglaoweicn/weknora-desktop"
API="https://api.github.com"
UPLOAD="https://uploads.github.com"

DESKTOP="$(cd "$(dirname "$0")/.." && pwd)"
cd "$DESKTOP"
[ -f "$DIST/latest.json" ] || { echo "错误: $DIST/latest.json 不存在,先跑 make-update-manifest.sh"; exit 1; }

# ---- 凭据(取自 git credential manager,绝不打印) ----
TOKEN=$(printf "protocol=https\nhost=github.com\n" | git credential fill \
        | grep "^password=" | cut -d= -f2-)
[ -n "$TOKEN" ] || { echo "错误: git 凭据管理器中没有 github.com 的凭据"; exit 1; }
AUTH="Authorization: token $TOKEN"

json_field() { python -c "import sys,json;d=json.load(sys.stdin);print(d.get('$1',''))" 2>/dev/null; }

# ---- 版本与说明取自 latest.json(Python 直读 UTF-8,ensure_ascii 规避
#      Windows 管道 GBK 编码把中文弄坏导致 "Problems parsing JSON") ----
BODY=$(PYTHONUTF8=1 python - "$DIST/latest.json" "$TAG" <<'PY'
import sys, json
latest_path, tag = sys.argv[1], sys.argv[2]
with open(latest_path, encoding="utf-8") as f:
    latest = json.load(f)
print(json.dumps({
    "tag_name": tag,
    "target_commitish": "main",
    "name": f"LinkMind Desktop v{latest['version']}",
    "body": latest.get("notes") or "本次更新包含稳定性修复与体验改进。",
    "draft": False,
    "prerelease": False,
}, ensure_ascii=True))
PY
)

# ---- 创建 Release(已存在则复用) ----
EXISTING=$(curl -sS -H "$AUTH" "$API/repos/$REPO/releases/tags/$TAG")
if echo "$EXISTING" | grep -q '"id"'; then
  RELEASE_ID=$(echo "$EXISTING" | json_field id)
  echo "Release $TAG 已存在(id=$RELEASE_ID),复用并补传资产"
else
  CREATED=$(curl -sS -X POST -H "$AUTH" -H "Content-Type: application/json" \
            -d "$BODY" "$API/repos/$REPO/releases")
  RELEASE_ID=$(echo "$CREATED" | json_field id)
  [ -n "$RELEASE_ID" ] || { echo "错误: 创建 Release 失败:"; echo "$CREATED" | head -5; exit 1; }
  echo "Release $TAG 已创建(id=$RELEASE_ID)"
fi

# ---- 上传资产(幂等:已存在同名资产先删除) ----
upload() {
  local file="$1" name="$2" i
  for i in 1 2 3 4 5; do
    echo "上传 $name (第 $i 次尝试)..."
    # 已有同名资产则先删,避免 422
    local existing_id
    existing_id=$(curl -sS -H "$AUTH" "$API/repos/$REPO/releases/$RELEASE_ID/assets" \
      | python -c "
import sys, json
for a in json.load(sys.stdin):
    if a['name'] == '$name':
        print(a['id']); break
" 2>/dev/null || true)
    if [ -n "${existing_id:-}" ]; then
      curl -sS -X DELETE -H "$AUTH" "$API/repos/$REPO/releases/assets/$existing_id" > /dev/null
    fi
    if curl -sS --fail -X POST -H "$AUTH" \
         -H "Content-Type: application/octet-stream" \
         --data-binary "@$DIST/$file" \
         "$UPLOAD/repos/$REPO/releases/$RELEASE_ID/assets?name=$name" > /dev/null; then
      echo "  $name 上传成功"
      return 0
    fi
    sleep 5
  done
  echo "错误: $name 多次上传失败"; return 1
}

for f in "$DIST"/*; do
  upload "$(basename "$f")" "$(basename "$f")"
done

echo ""
echo "==== 发布完成: https://github.com/$REPO/releases/tag/$TAG ===="
echo "更新端点自检:"
curl -sS -o /dev/null -w "latest.json(latest 跳转): %{http_code}\n" \
  "https://github.com/$REPO/releases/latest/download/latest.json" || true
