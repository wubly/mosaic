// Minimal WebDriver BiDi client for diagnosing Firefox-family browsers (Node 22+, built-in WebSocket).
// Injects a script into every frame before page scripts, then navigates, and writes console
// output ([spike] lines and errors) to a file.
//   node bidi.mjs <port> <url> <script.js> <out.log> <seconds>
import { readFileSync, appendFileSync, writeFileSync } from "node:fs";

const [port, url, scriptPath, outPath, seconds = "60"] = process.argv.slice(2);
const script = readFileSync(scriptPath, "utf8");
writeFileSync(outPath, "");

let ws;
for (let i = 0; i < 60; i++) {
  try {
    ws = new WebSocket(`ws://127.0.0.1:${port}/session`);
    await new Promise((ok, fail) => { ws.onopen = ok; ws.onerror = fail; });
    break;
  } catch {
    ws = null;
    await new Promise((r) => setTimeout(r, 500));
  }
}
if (!ws) { console.error("BiDi didn't come up"); process.exit(1); }

let nextId = 0;
const pending = new Map();
ws.onmessage = (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id !== undefined && pending.has(msg.id)) {
    pending.get(msg.id)(msg);
    pending.delete(msg.id);
  } else if (msg.method === "log.entryAdded") {
    const { level, text } = msg.params;
    if ((text || "").startsWith("[spike]") || level === "error" || level === "warn") {
      appendFileSync(outPath, `${level} ${text}\n`);
    }
  }
};
const send = (method, params = {}) => new Promise((ok) => {
  const id = ++nextId;
  pending.set(id, ok);
  ws.send(JSON.stringify({ id, method, params }));
});

console.log("session", JSON.stringify((await send("session.new", { capabilities: {} })).type));
const pre = await send("script.addPreloadScript", { functionDeclaration: `() => { ${script} }` });
console.log("preload", pre.type, pre.error || "");
await send("session.subscribe", { events: ["log.entryAdded"] });
const tree = await send("browsingContext.getTree");
const context = tree.result.contexts[0].context;
await send("browsingContext.navigate", { context, url, wait: "none" });
console.log("navigated", url);
setTimeout(() => process.exit(0), Number(seconds) * 1000);
