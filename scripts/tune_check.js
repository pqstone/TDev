// 微调浮窗（dist/tune.html）逻辑自验：mock DOM + mock __TAURI__，真跑 boot/send/ASK
const fs = require("fs");
const vm = require("vm");

const path = require("path");
const HTML = path.join(__dirname, "..", "dist", "tune.html");
const html = fs.readFileSync(HTML, "utf8");
const mm = html.match(/<script[^>]*>([\s\S]*?)<\/script>/);
if (!mm) { console.log("FAIL 没找到 script"); process.exit(1); }
const code = mm[1];

let PASS = 0, FAIL = 0;
function ok(name, cond, extra) {
  if (cond) { PASS++; console.log("  PASS " + name); }
  else { FAIL++; console.log("  FAIL " + name + (extra !== undefined ? "  -> " + JSON.stringify(extra) : "")); }
}

// ---------- mock DOM ----------
const ELS = {};
function makeEl(id) {
  const el = { id, textContent: "", value: "", title: "", disabled: false, focused: false,
    style: {}, scrollTop: 0, scrollHeight: 100, children: [], listeners: {}, _html: "",
    parentNode: null, checked: false };
  el.classList = {
    _s: new Set(),
    add(c) { this._s.add(c); }, remove(c) { this._s.delete(c); },
    contains(c) { return this._s.has(c); },
    toggle(c, on) { if (on === undefined) { this._s.has(c) ? this._s.delete(c) : this._s.add(c); } else { on ? this._s.add(c) : this._s.delete(c); } }
  };
  el.addEventListener = function (t, fn) { (el.listeners[t] = el.listeners[t] || []).push(fn); };
  el.appendChild = function (c) { c.parentNode = el; el.children.push(c); el.scrollHeight += 12; return c; };
  el.removeChild = function (c) { el.children = el.children.filter(function (x) { return x !== c; }); };
  el.remove = function () { if (el.parentNode) el.parentNode.removeChild(el); };
  el.focus = function () { el.focused = true; };
  el.querySelector = function () { return null; };
  Object.defineProperty(el, "innerHTML", {
    get() { return el._html; },
    set(v) { el._html = v; if (v === "") el.children = []; }
  });
  el.fire = function (t, ev) { (el.listeners[t] || []).forEach(function (f) { f.call(el, ev || {}); }); };
  return el;
}
function byId(id) { if (!ELS[id]) ELS[id] = makeEl(id); return ELS[id]; }

const rootStyle = {};
const bodyEl = makeEl("body");
const documentMock = {
  title: "",
  documentElement: { style: { setProperty(k, v) { rootStyle[k] = v; } } },
  body: bodyEl,
  getElementById: byId,
  createElement: function (t) { return makeEl("new-" + t); },
  querySelector: function () { return null; }
};

const store = {};
const localStorageMock = {
  getItem(k) { return Object.prototype.hasOwnProperty.call(store, k) ? store[k] : null; },
  setItem(k, v) { store[k] = String(v); },
  removeItem(k) { delete store[k]; }
};

// ---------- mock Tauri ----------
const INVOKES = [];
const EVT = {};
let RUN_NO = 0;
const tauriMock = {
  core: {
    invoke(cmd, args) {
      INVOKES.push({ cmd: cmd, args: args });
      if (cmd === "tune_ctx") return Promise.resolve({ prjGuid: "G1", prjName: "TestProject", cwd: "E:/ws/TestProject" });
      if (cmd === "agent_ask_stream") {
        RUN_NO++;
        const n = RUN_NO;
        return new Promise(function (resolve) {
          setTimeout(function () {
            (EVT["agent_stream"] || []).forEach(function (f) {
              f({ payload: { type: "reasoning", text: "先看数据结构" } });
              f({ payload: { type: "tool", text: "read_file：doc/data-model.md" } });
              f({ payload: { type: "tool_done", text: "read_file", duration: 0.4, error: false } });
              f({ payload: { type: "delta", text: "已把登录按钮改成绿色。" } });
              if (n === 2) {
                // 第二轮：Agent 要求确认
                f({ payload: { type: "delta", text: "先确认一下：\n[[ASK:{\"title\":\"项目改名\",\"body\":\"会影响库名与展示\",\"options\":[{\"v\":\"保持原名\",\"label\":\"保持原名\"},{\"v\":\"改名\",\"label\":\"改成 ForeignTrade\"}]}]]" } });
              }
            });
            resolve(n === 2 ? "" : "已把登录按钮改成绿色。");
          }, 5);
        });
      }
      return Promise.resolve(null);
    }
  },
  event: {
    listen(name, fn) { (EVT[name] = EVT[name] || []).push(fn); return Promise.resolve(function () {}); }
  }
};

const windowMock = { __TAURI__: tauriMock };

const ctx = vm.createContext({
  window: windowMock, document: documentMock, localStorage: localStorageMock,
  console: console, setTimeout: setTimeout, JSON: JSON, Math: Math, String: String, Number: Number,
  parseInt: parseInt, Promise: Promise, Array: Array, Date: Date, Object: Object
});
vm.runInContext(code, ctx, { filename: "tune-inline.js" });

function tick(ms) { return new Promise(function (r) { setTimeout(r, ms || 20); }); }

(async function () {
  await tick(40);

  console.log("[1] 启动 / 上下文");
  ok("取到项目名", byId("prjName").textContent === "TestProject", byId("prjName").textContent);
  const log = byId("log");
  ok("启动有提示行", log.children.length >= 3, log.children.length);
  ok("注册了 agent_stream 监听", (EVT["agent_stream"] || []).length === 1);
  ok("默认窗体不透明度 0.90", rootStyle["--win-alpha"] === "0.90", rootStyle["--win-alpha"]);
  ok("滑块最低 10%", /id="alpha"[^>]*min="10"/.test(html) || /min="10"[^>]*id="alpha"/.test(html));
  byId("alpha").value = "10";
  byId("alpha").fire("input", {});
  ok("拉到 10% 生效", rootStyle["--win-alpha"] === "0.10", rootStyle["--win-alpha"]);
  ok("窗体透明度记忆到 tune_win_alpha", store["tune_win_alpha"] === "10", store["tune_win_alpha"]);

  console.log("[2] 回车不发送 / 只能点按钮");
  const ta = byId("input");
  ok("输入框没有 keydown 监听（回车=换行）", (ta.listeners.keydown || []).length === 0);
  ta.value = "把登录按钮改成绿色";
  const before = INVOKES.length;
  ok("没点执行时不发请求", INVOKES.length === before);

  console.log("[3] 点「执行」→ 与 AI开发 共享");
  byId("execBtn").fire("click", {});
  const sent = INVOKES.filter(function (x) { return x.cmd === "agent_ask_stream"; });
  ok("点了才发请求", sent.length === 1, INVOKES.length);
  if (sent.length) {
    const a = sent[0].args;
    ok("会话名与 AI开发 一致", a.sessionId === "module:ai-code:G1", a.sessionId);
    ok("cwd 钉在项目目录", a.cwd === "E:/ws/TestProject", a.cwd);
    ok("多轮记忆开关打开", a.sessionMemory === true, a.sessionMemory);
    ok("提示词含 ASK 协议", String(a.instructions).indexOf("[[ASK:") >= 0);
    ok("提示词说明与 AI开发 共用", String(a.instructions).indexOf("共用同一份聊天记录") >= 0);
    ok("指令原文传入", a.question === "把登录按钮改成绿色", a.question);
  }
  ok("执行中按钮禁用", byId("execBtn").disabled === true);

  // 共享聊天记录（AI开发 同 key）
  let hist = JSON.parse(store["ai_hist_G1"] || "[]");
  ok("聊天写入 ai_hist_<guid>", hist.length === 1 && hist[0].t === "把登录按钮改成绿色" && hist[0].me === true, hist);

  await tick(60);
  console.log("[4] 共享执行日志");
  const shared = JSON.parse(store["ai_log_G1"] || "[]");
  ok("执行日志写入 ai_log_<guid>", shared.length >= 3, shared.length);
  ok("日志带 class（AI开发 能识别）", shared.some(function (r) { return r.c === "tool"; }));
  ok("日志带时间戳", shared.every(function (r) { return typeof r.t === "number"; }));
  ok("用户指令进日志", shared.some(function (r) { return r.c === "me"; }));
  hist = JSON.parse(store["ai_hist_G1"] || "[]");
  ok("Agent 回复进聊天记录", hist.length === 2 && hist[1].me === false, hist.length);
  ok("完成后按钮恢复", byId("execBtn").disabled === false);

  console.log("[5] Agent 询问确认项 → 弹窗");
  ta.value = "项目改名为 ForeignTrade";
  byId("execBtn").fire("click", {});
  await tick(80);
  const masks = bodyEl.children.filter(function (c) { return c.id === "askMask"; });
  ok("出现确认弹窗", masks.length === 1, bodyEl.children.length);
  const mask = masks[0];
  ok("弹窗标题渲染", String(mask.innerHTML).indexOf("项目改名") >= 0);
  ok("弹窗渲染选项", String(mask.innerHTML).indexOf("改成 ForeignTrade") >= 0);
  ok("弹窗期间执行按钮禁用", byId("execBtn").disabled === true, byId("execBtn").disabled);
  ok("日志记了 ASK 行", log.children.some(function (c) { return c.className === "ln ask"; }));
  ok("ASK 已写入共享日志", JSON.parse(store["ai_log_G1"]).some(function (r) { return r.c === "ask"; }));

  // 模拟用户选第二项并点「确定并继续」
  mask.querySelector = function () { return { value: "改名" }; };
  const beforeAns = INVOKES.filter(function (x) { return x.cmd === "agent_ask_stream"; }).length;
  byId("askOk").onclick();
  const afterAns = INVOKES.filter(function (x) { return x.cmd === "agent_ask_stream"; });
  ok("点确定后把答案发回 Agent", afterAns.length === beforeAns + 1, afterAns.length);
  const lastAsk = afterAns[afterAns.length - 1].args;
  ok("答案内容正确", lastAsk.question === "改名", lastAsk.question);
  ok("答案用同一会话", lastAsk.sessionId === "module:ai-code:G1", lastAsk.sessionId);
  ok("弹窗已关闭", bodyEl.children.filter(function (c) { return c.id === "askMask"; }).length === 0);
  ok("答案进聊天记录", JSON.parse(store["ai_hist_G1"]).some(function (h) { return h.t === "改名" && h.me; }));

  await tick(60);
  ok("回答后按钮恢复可点", byId("execBtn").disabled === false);

  console.log("[6] 清屏 & 结构");
  byId("clearBtn").fire("click", {});
  ok("清屏同时清共享日志", JSON.parse(store["ai_log_G1"] || "[]").length === 0);
  ok("有拖拽区 / 关闭按钮", (byId("dragZone").listeners.mousedown || []).length === 1 && (byId("closeBtn").listeners.click || []).length === 1);
  ok("有「回到主窗口」按钮", (byId("backBtn").listeners.click || []).length === 1);
  const back = function () { return INVOKES.filter(function (x) { return x.cmd === "tune_back_to_main"; }).length; };
  const b0 = back();
  byId("backBtn").fire("click", {});
  ok("点「主窗口」调用 tune_back_to_main", back() === b0 + 1, back());
  byId("closeBtn").fire("click", {});
  ok("✕ 同样把主窗口捞回来", back() === b0 + 2, back());

  console.log("\nPASS " + PASS + "  FAIL " + FAIL);
  process.exit(FAIL ? 1 : 0);
})();
