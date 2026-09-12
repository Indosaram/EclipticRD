#!/usr/bin/env node
// Exercise the installed release client, not an in-process backend mock.
import assert from "node:assert/strict";
import { spawn, execFile } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { promisify, stripVTControlCharacters } from "node:util";

const exec = promisify(execFile);
const argv = process.argv.slice(2);
const option = (name, fallback) => {
  const index = argv.indexOf(name);
  return index < 0 ? fallback : argv[index + 1];
};
const host = option("--host");
const transport = option("--transport", "http");
const performance = argv.includes("--performance");
assert(host, "--host is required");
assert(["http", "mcp"].includes(transport), "--transport must be http or mcp");
assert(process.env.MAHO_CLIENT, "MAHO_CLIENT must point to a compiled maho-client");
assert(process.env.MAHO_PAIRING_ID || process.env.MAHO_QA_PIN,
  "MAHO_PAIRING_ID or an isolated host's MAHO_QA_PIN is required");
const evidence = resolve(process.env.MAHO_EVIDENCE_DIR ?? `.omo/agent-first-qa-${Date.now()}`);
const port = process.env.MAHO_API_PORT ?? "28735";
await mkdir(evidence, { recursive: true, mode: 0o700 });

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  // A process may fail before the scenario reaches its next await.
  promise.catch(() => {});
  return { promise, resolve, reject };
}

async function bounded(promise, label, milliseconds = 20000) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(`${label} timed out`)), milliseconds);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

const args = ["--host", host,
  "--timeout-secs", "90", "--stats-json", resolve(evidence, "receiver.json")];
if (process.env.MAHO_PAIRING_ID) args.push("--pairing-id", process.env.MAHO_PAIRING_ID);
else args.push("--pin", process.env.MAHO_QA_PIN);
if (process.env.MAHO_TCP_PORT) args.push("--tcp-port", process.env.MAHO_TCP_PORT);
if (process.env.MAHO_UDP_PORT) args.push("--udp-port", process.env.MAHO_UDP_PORT);
if (process.env.MAHO_PAIRING_STORE) args.push("--pairing-store", process.env.MAHO_PAIRING_STORE);
if (performance) args.push("--frames", "120", "--nudge-ms", "50");
if (transport === "mcp") args.push("--mcp");
else args.push("--agent-server", port);

const frameReady = deferred();
const listenerReady = deferred();
const exit = deferred();
const pending = new Map();
const remainders = {};
const events = [];
let nextId = 1;
let stdout = "";
let stderr = "";
let protocolError;
let completed = false;
const child = spawn(process.env.MAHO_CLIENT, args, { stdio: ["pipe", "pipe", "pipe"] });
child.once("error", error => {
  frameReady.reject(error);
  listenerReady.reject(error);
  exit.reject(error);
  for (const request of pending.values()) request.reject(error);
});
child.once("close", (code, signal) => {
  if (transport === "mcp" && remainders.stdout?.trim()) {
    protocolError = new Error("MCP stdout ended without a newline-delimited response");
  }
  const result = { code, signal };
  exit.resolve(result);
  const error = new Error(`client exited before response: ${JSON.stringify(result)}`);
  frameReady.reject(error);
  listenerReady.reject(error);
  for (const request of pending.values()) request.reject(error);
});
child.stdin.on("error", error => {
  for (const request of pending.values()) request.reject(error);
});

function watchLines(stream, channel) {
  let remaining = "";
  stream.on("data", chunk => {
    const text = chunk.toString();
    if (channel === "stdout") stdout += text;
    else stderr += text;
    remaining += text;
    let end;
    while ((end = remaining.indexOf("\n")) >= 0) {
      const line = remaining.slice(0, end).replace(/\r$/, "");
      remaining = remaining.slice(end + 1);
      if (/Decoded frame progress.*decoded_frames=\d*[1-9]\d*/.test(stripVTControlCharacters(line))) {
        frameReady.resolve();
      }
      if (stripVTControlCharacters(line).includes(`Headless agent server listening on http://127.0.0.1:${port}`)) {
        listenerReady.resolve();
      }
      if (transport !== "mcp" || channel !== "stdout" || !line) continue;
      let message;
      try {
        message = JSON.parse(line);
        assert.equal(message.jsonrpc, "2.0");
      } catch {
        protocolError = new Error(`MCP stdout is not JSON-RPC: ${line.slice(0, 200)}`);
        for (const request of pending.values()) request.reject(protocolError);
        continue;
      }
      const request = pending.get(message.id);
      if (request) {
        pending.delete(message.id);
        request.resolve(message);
      } else if ("id" in message) {
        protocolError = new Error(`Unexpected MCP response ID: ${JSON.stringify(message.id)}`);
        for (const request of pending.values()) request.reject(protocolError);
      }
    }
    remainders[channel] = remaining;
  });
}
watchLines(child.stdout, "stdout");
watchLines(child.stderr, "stderr");

async function rpc(method, params) {
  if (protocolError) throw protocolError;
  const id = nextId++;
  const response = deferred();
  pending.set(id, response);
  const request = { jsonrpc: "2.0", id, method, params };
  events.push({ request });
  child.stdin.write(`${JSON.stringify(request)}\n`);
  const result = await bounded(response.promise, method);
  events.push({ response: result });
  return result;
}

async function http(path, body, expected = 200) {
  const command = ["-sS", "-i", "--max-time", "15", `http://127.0.0.1:${port}/api/v1/${path}`];
  if (body !== undefined) command.push("-H", "Content-Type: application/json",
    "-X", "POST", "--data", JSON.stringify(body));
  const result = await exec("curl", command, { maxBuffer: 32 * 1024 * 1024 });
  await writeFile(resolve(evidence, `http-${events.length}.txt`), result.stdout);
  const delimiter = result.stdout.indexOf("\r\n\r\n");
  assert(delimiter >= 0, "HTTP response must include headers");
  const status = Number(result.stdout.split(" ")[1]);
  assert.equal(status, expected, `HTTP ${path}`);
  const value = JSON.parse(result.stdout.slice(delimiter + 4));
  events.push({ http: path, status, body: body ?? null });
  return value;
}

async function image(base64, name, info) {
  const bytes = Buffer.from(base64, "base64");
  assert.equal(bytes.subarray(0, 8).toString("hex"), "89504e470d0a1a0a", "real PNG response");
  assert(info.width > 0 && info.height > 0, "nonempty remote screen geometry");
  assert.equal(bytes.readUInt32BE(16), info.width, "PNG width matches remote screen");
  assert.equal(bytes.readUInt32BE(20), info.height, "PNG height matches remote screen");
  await writeFile(resolve(evidence, name), bytes);
}

try {
  if (performance) {
    const result = await bounded(exit.promise, "120-frame performance capture", 100000);
    assert.equal(result.code, 0);
  } else if (transport === "http") {
    await bounded(Promise.all([frameReady.promise, listenerReady.promise]), "listener and first decoded frame");
    assert.equal((await http("health")).status, "ok");
    const info = await http("screen/info");
    await http("input/action", { action: "mouse_move", x: "invalid", y: 0 }, 400);
    await image((await http("screen/screenshot?format=png")).base64, "screen-before.png", info);
    await http("input/action", { action: "mouse_move", x: 0.45, y: 0.45, normalized: true });
    await image((await http("screen/screenshot?format=png")).base64, "screen.png", info);
    await http("input/action", { action: "release_all" });
    await http("session/disconnect", {});
    assert.equal((await bounded(exit.promise, "HTTP disconnect")).code, 0);
  } else {
    const initialized = await rpc("initialize", {
      protocolVersion: "2024-11-05", capabilities: {},
      clientInfo: { name: "maho-agent-first-qa", version: "1.0" },
    });
    assert(initialized.result?.capabilities?.tools);
    child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" })}\n`);
    const listed = await rpc("tools/list", {});
    const names = listed.result.tools.map(tool => tool.name);
    for (const name of ["remote_mouse_click", "remote_mouse_move", "remote_mouse_drag",
      "remote_mouse_scroll", "remote_key_press", "remote_hotkey", "remote_type_text",
      "remote_release_all", "remote_get_screen_info", "remote_take_screenshot"]) {
      assert(names.includes(name), `missing documented tool ${name}`);
    }
    const invalid = await rpc("tools/call", {
      name: "remote_mouse_move", arguments: { x: "invalid", y: 0 },
    });
    assert(invalid.error || invalid.result?.isError, "malformed MCP input must fail");
    await bounded(frameReady.promise, "first decoded frame");
    const screen = await rpc("tools/call", { name: "remote_get_screen_info", arguments: {} });
    const info = JSON.parse(screen.result.content.find(item => item.type === "text").text);
    const shot = await rpc("tools/call", { name: "remote_take_screenshot", arguments: {} });
    await image(shot.result.content.find(item => item.type === "image").data, "screen-before.png", info);
    const move = await rpc("tools/call", {
      name: "remote_mouse_move", arguments: { x: 0.45, y: 0.45, normalized: true },
    });
    assert(!move.error && !move.result?.isError, "mouse move must succeed");
    const after = await rpc("tools/call", { name: "remote_take_screenshot", arguments: {} });
    await image(after.result.content.find(item => item.type === "image").data, "screen.png", info);
    await rpc("tools/call", { name: "remote_release_all", arguments: {} });
    child.stdin.end();
    assert.equal((await bounded(exit.promise, "MCP EOF cleanup", 5000)).code, 0);
    assert.equal(protocolError, undefined);
  }
  if (process.env.MAHO_RECEIVER_TRACE_PATH) {
    const [trace] = JSON.parse(await readFile(process.env.MAHO_RECEIVER_TRACE_PATH, "utf8"));
    assert(trace.records.some(record => record.event === "QueueAdmission"),
      "real CLI queue must share the session receiver trace");
  }
  completed = true;
  console.log(`PASS ${transport}${performance ? " performance" : ""} ${host} ${evidence}`);
} catch (error) {
  events.push({ failure: error.message });
  process.exitCode = 1;
  console.error(`FAIL ${error.message}`);
} finally {
  if (child.exitCode === null && child.signalCode === null) {
    child.kill("SIGTERM");
    await bounded(exit.promise, "QA client cleanup", 5000).catch(async () => {
      child.kill("SIGKILL");
      await bounded(exit.promise, "forced QA client cleanup", 5000);
    });
  }
  await writeFile(resolve(evidence, "stdout.log"), stdout);
  await writeFile(resolve(evidence, "stderr.log"), stderr);
  await writeFile(resolve(evidence, "transcript.json"), JSON.stringify(events, null, 2));
  await writeFile(resolve(evidence, "result.json"), JSON.stringify({
    passed: completed, host, transport, performance,
    cleanup: { pid: child.pid, exitCode: child.exitCode, signal: child.signalCode },
  }, null, 2));
}
