import re, os, subprocess, shutil, tempfile

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
node = shutil.which("node")
print("node:", node)

def scripts_of(path):
    s = open(path, "r", encoding="utf-8", errors="replace").read()
    return re.findall(r"<script[^>]*>(.*?)</script>", s, re.S)

for rel in ["dist/index.html", "dist/tune.html"]:
    p = os.path.join(root, rel)
    s = open(p, "r", encoding="utf-8", errors="replace").read()
    print("=" * 50)
    print(rel, "size", len(s), "scripts", len(scripts_of(p)))
    # 关键符号校验
    if rel.endswith("index.html"):
        for k in ['data-m="10"', 'dynamic: "tune"', "function openTuneMode", "function renderTune",
                  'kind === "tune"', 'invoke("tune_open"', 'invoke("tune_ctx"', 'invoke("tune_close"', 'invoke("tune_drag"']:
            print("  ", k, s.count(k))
        print("   旧菜单项 data-m=\"9\":", s.count('data-m="9"'), " 微调菜单:", s.count("微调模式"))
    else:
        for k in ["tune_ctx", "tune_close", "tune_drag", "agent_ask_stream", "module:tune:", "log-alpha", "execBtn"]:
            print("  ", k, s.count(k))
    # 抽出脚本做 node --check
    for i, js in enumerate(scripts_of(p)):
        if not js.strip():
            continue
        tmp = os.path.join(tempfile.gettempdir(), "chk_%s_%d.js" % (os.path.basename(rel), i))
        with open(tmp, "w", encoding="utf-8") as f:
            f.write(js)
        r = subprocess.run([node, "--check", tmp], capture_output=True, text=True, encoding="utf-8", errors="replace")
        print("   node --check script#%d rc=%d %s" % (i, r.returncode, (r.stderr or "").strip()[:400]))
