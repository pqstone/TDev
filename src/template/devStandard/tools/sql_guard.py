#!/usr/bin/env python3
"""SQL 守卫：禁止 SELECT *（非贪婪取列红线）。

用法（pre-commit 或 CI）：
    python tools/sql_guard.py                # 检查全部 java/sql 文件
    python tools/sql_guard.py path/file.java # 检查指定文件
退出码非 0 即视为违规，阻止提交/构建。
"""
import os
import re
import sys

# 匹配 SELECT 开头的 *（忽略注释行），并排除 "SELECT count(*)" / "COUNT(*)"
PATTERN = re.compile(r"SELECT\s+\*", re.IGNORECASE)
ALLOWED = re.compile(r"COUNT\s*\(\s*\*\s*\)", re.IGNORECASE)
TARGET_EXTS = (".java", ".sql")


def check_file(path):
    """返回 (违规数, 行号列表)。"""
    try:
        with open(path, "r", encoding="utf-8") as f:
            lines = f.readlines()
    except (OSError, UnicodeDecodeError):
        return 0, []
    hits = []
    for i, line in enumerate(lines, start=1):
        stripped = line.lstrip()
        # 跳过注释行：// 、/* 、*（Javadoc 内容行）、*/ 、-- 、#
        if stripped.startswith(("//", "/*", "*", "--", "#")):
            continue
        if PATTERN.search(line) and not ALLOWED.search(line):
            hits.append(i)
    return len(hits), hits


def main():
    targets = sys.argv[1:] or _walk_all()
    if not targets:
        print("OK: no files matched")
        return 0
    total = 0
    for p in targets:
        n, hits = check_file(p)
        if n:
            total += n
            print(f"VIOLATION {p}: {len(hits)} 处 SELECT *，行 {hits}（按需取列，禁止全字段查询）")
    if total:
        print(f"FAIL: 共 {total} 处 SELECT *，请改为按需取列后重试")
        return 1
    print("OK: 未发现 SELECT *")
    return 0


def _walk_all():
    roots = ["src/main"]
    files = []
    for root in roots:
        for dirpath, _, names in os.walk(root):
            for name in names:
                if name.endswith(TARGET_EXTS):
                    files.append(os.path.join(dirpath, name))
    return files


if __name__ == "__main__":
    sys.exit(main())
