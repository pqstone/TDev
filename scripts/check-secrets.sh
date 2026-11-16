#!/usr/bin/env bash
# 推送前敏感信息扫描 —— 防止把 Token / Key / 密码提交到 public 仓库
#
# 用法:
#   bash scripts/check-secrets.sh            # 扫工作区已跟踪文件（秒级，push 前必跑）
#   bash scripts/check-secrets.sh --history  # 额外扫全历史（慢，验收"历史是否干净"时跑）
#   bash scripts/check-secrets.sh --all      # 工作区 + 未跟踪文件（准备 git add 前跑）
#
# 退出码: 0 = 干净；1 = 命中；2 = 环境错误
# 输出里命中的密钥会打码，避免扫描本身又把明文打到日志/终端里。
set -uo pipefail
cd "$(dirname "$0")/.." || exit 2

MODE="tracked"
case "${1:-}" in
  --history) MODE="history" ;;
  --all)     MODE="all" ;;
  "")        ;;
  *) echo "未知参数: $1（可用 --history / --all）" >&2; exit 2 ;;
esac

# 高危模式（扩展正则）。加一条就多一层保险。
PATTERNS=$(cat <<'EOF'
AKIA[0-9A-Z]{16}
gh[pousr]_[A-Za-z0-9]{20,}
github_pat_[A-Za-z0-9_]{20,}
sk-[A-Za-z0-9_-]{20,}
ark-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}
xox[baprs]-[A-Za-z0-9-]{10,}
AIza[0-9A-Za-z_-]{30,}
Bearer[[:space:]]+[A-Za-z0-9._-]{16,}
BEGIN (RSA|OPENSSH|EC|DSA|PGP) PRIVATE KEY
https://[^/@:]+:[^/@]{6,}@
(api[_-]?key|apikey|access[_-]?key|secret[_-]?key|client[_-]?secret|passwd|password|token)[[:space:]]*[:=][[:space:]]*["'][^"']{8,}["']
EOF
)

GLOBS=(
  ':!node_modules'
  ':!target'
  ':!src-tauri/target'
  ':!dist'
  ':!*.lock'
  ':!package-lock.json'
  ':!scripts/check-secrets.sh'
)

mask() {
  # 命中值过短就整体保留，长的一律保留前 4 位 + ***
  awk '{
    line=$0
    # 把疑似机密片段打码：含 16 位以上连续字母数字的部分
    gsub(/[A-Za-z0-9._-]{16,}/, "&***", line)
    print line
  }' | sed -E 's/([A-Za-z0-9._-]{4})[A-Za-z0-9._-]{16,}\*\*\*/\1***g/g'
}

echo "=== 扫描范围: $MODE ==="
hits=0

scan_tracked() {
  local out
  out=$(git grep -nIE -f <(printf '%s\n' "$PATTERNS") -- . "${GLOBS[@]}" 2>/dev/null)
  if [ -n "$out" ]; then
    echo "$out" | mask
    hits=$((hits + $(printf '%s\n' "$out" | wc -l)))
  fi
}

case "$MODE" in
  tracked|all) scan_tracked ;;
esac

if [ "$MODE" = "all" ]; then
  echo "--- 未跟踪文件 ---"
  while IFS= read -r f; do
    [ -z "$f" ] && continue
    case "$f" in
      *.png|*.jpg|*.jpeg|*.ico|*.gif|*.zip|*.exe|*.dll|*.woff*|*.ttf) continue ;;
    esac
    out=$(grep -nIE -f <(printf '%s\n' "$PATTERNS") -- "$f" 2>/dev/null)
    if [ -n "$out" ]; then
      printf '%s\n' "$out" | sed "s|^|$f:|" | mask
      hits=$((hits + $(printf '%s\n' "$out" | wc -l)))
    fi
  done < <(git ls-files --others --exclude-standard)
fi

if [ "$MODE" = "history" ]; then
  echo "--- 全历史（git log -G 扩展正则，只报提交号，不打印明文）---"
  # 用单个合并正则让 git 自己遍历每个提交的 diff —— 比逐个 -S 子串匹配精确得多，
  # 也不会因为正则里带 [] 被 -S 当普通子串而误报一堆无关提交。
  MEGA=$(printf '%s\n' "$PATTERNS" | paste -sd'|' -)
  hist=$(git log --all --format='%h %s' -G"$MEGA" -- . "${GLOBS[@]}" 2>/dev/null)
  if [ -n "$hist" ]; then
    printf '%s\n' "$hist" | sed 's/^/  ⚠ /'
    hits=$((hits + $(printf '%s\n' "$hist" | wc -l)))
    echo "  → 复核命令: git show <提交号> -- <文件>（确认是真实密钥就轮换它并清理历史）"
  fi
fi

echo "=== 结论 ==="
if [ "$hits" -eq 0 ]; then
  echo "✅ 未发现疑似敏感信息"
  exit 0
else
  echo "❌ 发现 $hits 处疑似敏感信息 —— 不要 push！先改成从环境变量/本地文件读取。"
  echo "   本项目约定：密钥放 %USERPROFILE%\\.tdev\\ 或环境变量，仓库里只留占位符。"
  exit 1
fi
