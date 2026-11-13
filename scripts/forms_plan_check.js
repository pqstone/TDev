// 校验：地基步骤已变为 6 步，且计划提示词里不再有「设计数据结构」专属段落
const fs = require("fs");
const path = require("path");
const html = fs.readFileSync(path.join(__dirname, "..", "dist", "index.html"), "utf8");

let pass = 0, fail = 0;
function ok(n, c, extra) {
  if (c) { pass++; console.log("  PASS " + n); }
  else { fail++; console.log("  FAIL " + n + (extra !== undefined ? "  -> " + JSON.stringify(extra) : "")); }
}

// 抠出 aiBuildBaseSteps 函数体
const m = html.match(/function aiBuildBaseSteps\(\) \{([\s\S]*?)\n    \}/);
ok("找到 aiBuildBaseSteps", !!m);
const body = m ? m[1] : "";
const ctxx = {
  AGENT_CONST: { stack: { database: "MySQL 8", server: "Tomcat 9", backend: "Java 17" } },
  aiShortStack: function (s) { return String(s || "").split(/[，,]/)[0]; }
};
const vm = require("vm");
vm.createContext(ctxx);
const steps = vm.runInContext("(function(){" + body + "})()".replace("return [", "return [") , ctxx);
// 上面直接跑不通（函数体里是 return [...]），换个方式：包成函数
const fn = new Function("AGENT_CONST", "aiShortStack", "function aiBuildBaseSteps(){" + body + "}\nreturn aiBuildBaseSteps();");
const list = fn(ctxx.AGENT_CONST, ctxx.aiShortStack);
console.log("  steps:", list.map(s => s.name).join(" / "));
ok("地基是 6 步", list.length === 6, list.length);
ok("不再有「设计数据结构」", !list.some(s => s.name.indexOf("设计数据结构") >= 0));
ok("第 6 步是权限校验", list[5].name === "做权限校验", list[5] && list[5].name);

// 提示词残留检查
const promptHasStep7Block = html.indexOf("【这一步只做数据结构设计：只出文档，不写一行代码】") >= 0;
ok("第 7 步专属方法论已从计划提示词移除", !promptHasStep7Block);
ok("第 0 条改指向业务表单模块", html.indexOf("由「业务表单」模块按业务流程生成") >= 0);
ok("计划面板按钮改为业务表单", html.indexOf("📐 业务表单") >= 0);
ok("按钮跳转模块 11", html.indexOf("dmBtn.onclick = function () { render(11); }") >= 0);
ok("业务表单菜单项存在", html.indexOf('data-m="11"') >= 0);
ok("MODULES 有 11", html.indexOf('11: { t: "业务表单"') >= 0);
ok("PRJ_REFRESH 登记 11", html.indexOf("11: function () { renderForms(); }") >= 0);
ok("场景 forms 已注册", html.indexOf("sessionPrefix: \"module:forms\"") >= 0);
ok("历史 key 独立", html.indexOf('if (sceneId === "forms") return "forms_hist_"') >= 0);
ok("生成提示词含输出契约", html.indexOf("[[FORMS]]") >= 0);

console.log("\nPASS " + pass + "  FAIL " + fail);
process.exit(fail ? 1 : 0);
