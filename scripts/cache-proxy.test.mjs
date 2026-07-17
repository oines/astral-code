import assert from "node:assert/strict";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  commonPrefixLength,
  createProxyServer,
  parseUsage,
  summarizeRequest,
} from "./cache-proxy.mjs";

function listen(server) {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => resolve(server.address()));
  });
}

function close(server) {
  return new Promise((resolve, reject) => {
    server.close((error) => (error ? reject(error) : resolve()));
  });
}

test("commonPrefixLength compares strings and arrays", () => {
  assert.equal(commonPrefixLength("abcdef", "abcxyz"), 3);
  assert.equal(commonPrefixLength(["a", "b"], ["a", "c"]), 1);
});

test("parseUsage reads DeepSeek Chat Completions cache counters", () => {
  const response = Buffer.from(
    'data: {"usage":{"prompt_tokens":100,"prompt_cache_hit_tokens":80,"prompt_cache_miss_tokens":20}}\n\n' +
      "data: [DONE]\n\n",
  );
  assert.deepEqual(parseUsage("chat_completions", response), {
    prompt_tokens: 100,
    prompt_cache_hit_tokens: 80,
    prompt_cache_miss_tokens: 20,
  });
});

test("parseUsage merges Anthropic message start and delta counters", () => {
  const response = Buffer.from(
    'data: {"type":"message_start","message":{"usage":{"input_tokens":100,"cache_read_input_tokens":64}}}\n\n' +
      'data: {"type":"message_delta","usage":{"output_tokens":12}}\n\n',
  );
  assert.deepEqual(parseUsage("anthropic_messages", response), {
    input_tokens: 100,
    cache_read_input_tokens: 64,
    output_tokens: 12,
  });
});

test("summarizeRequest records stable prefixes without storing message text", () => {
  const firstBody = {
    model: "model-a",
    messages: [
      { role: "system", content: "stable" },
      { role: "user", content: "first" },
    ],
    tools: [{ type: "function", function: { name: "shell", parameters: {} } }],
  };
  const firstDecoded = Buffer.from(JSON.stringify(firstBody));
  const first = summarizeRequest({
    requestId: 1,
    testCase: "chat-direct",
    protocol: "chat_completions",
    method: "POST",
    upstreamPath: "/v1/chat/completions",
    encodedBytes: firstDecoded.length,
    decodedBody: firstDecoded,
    body: firstBody,
    previous: null,
  });

  const secondBody = {
    ...firstBody,
    messages: [...firstBody.messages, { role: "assistant", content: "second" }],
  };
  const secondDecoded = Buffer.from(JSON.stringify(secondBody));
  const second = summarizeRequest({
    requestId: 2,
    testCase: "chat-direct",
    protocol: "chat_completions",
    method: "POST",
    upstreamPath: "/v1/chat/completions",
    encodedBytes: secondDecoded.length,
    decodedBody: secondDecoded,
    body: secondBody,
    previous: first.current,
  });

  assert.equal(second.summary.common_prefix_messages, 2);
  assert.equal(second.summary.tools_sha256, first.summary.tools_sha256);
  assert.equal(second.summary.system_sha256, first.summary.system_sha256);
  assert.equal(JSON.stringify(second.summary).includes("stable"), false);
  assert.equal(JSON.stringify(second.summary).includes("first"), false);
  assert.equal(JSON.stringify(second.summary).includes("second"), false);
});

test("summarizeRequest accepts an empty model-discovery body", () => {
  const request = summarizeRequest({
    requestId: 1,
    testCase: "chat-direct",
    protocol: "chat_completions",
    method: "GET",
    upstreamPath: "/v1/models",
    encodedBytes: 0,
    decodedBody: Buffer.alloc(0),
    body: {},
    previous: null,
  });

  assert.equal(request.summary.request_bytes, 0);
  assert.equal(request.summary.message_count, 0);
  assert.deepEqual(request.summary.tool_names, []);
});

test("proxy forwards redacted requests and records early upstream closes", async (t) => {
  const directory = fs.mkdtempSync(
    path.join(os.tmpdir(), "astral-cache-proxy-"),
  );
  const logPath = path.join(directory, "requests.jsonl");
  let forwardedRequest;
  const upstream = http.createServer((request, response) => {
    if (request.url === "/v1/aborted") {
      request.resume();
      request.on("end", () => {
        response.writeHead(200, { "content-type": "text/event-stream" });
        response.flushHeaders();
        response.write('data: {"usage":{"prompt_cache_hit_tokens":64}}\n\n');
        setImmediate(() => response.socket.destroy());
      });
      return;
    }
    const chunks = [];
    request.on("data", (chunk) => chunks.push(chunk));
    request.on("end", () => {
      forwardedRequest = {
        method: request.method,
        url: request.url,
        body: Buffer.concat(chunks).toString("utf8"),
      };
      response.writeHead(200, { "content-type": "text/event-stream" });
      response.end(
        'data: {"usage":{"prompt_cache_hit_tokens":80,"prompt_cache_miss_tokens":20}}\n\ndata: [DONE]\n\n',
      );
    });
  });
  const upstreamAddress = await listen(upstream);
  const proxy = createProxyServer({
    upstreamOrigin: `http://127.0.0.1:${upstreamAddress.port}`,
    logPath,
  });
  const proxyAddress = await listen(proxy);
  t.after(async () => {
    await close(proxy);
    await close(upstream);
    fs.rmSync(directory, { recursive: true, force: true });
  });

  const requestBody = JSON.stringify({
    model: "model-a",
    messages: [{ role: "user", content: "private source text" }],
    tools: [],
  });
  await new Promise((resolve, reject) => {
    const request = http.request(
      `http://127.0.0.1:${proxyAddress.port}/chat-direct/v1/chat/completions?include_usage=1`,
      {
        method: "POST",
        headers: {
          "authorization": "Bearer private-token",
          "content-type": "application/json",
        },
      },
      (response) => {
        response.resume();
        response.on("end", resolve);
      },
    );
    request.on("error", reject);
    request.end(requestBody);
  });

  assert.deepEqual(forwardedRequest, {
    method: "POST",
    url: "/v1/chat/completions?include_usage=1",
    body: requestBody,
  });
  const successRecordText = fs.readFileSync(logPath, "utf8");
  const successRecord = JSON.parse(successRecordText);
  assert.deepEqual(
    {
      test_case: successRecord.test_case,
      path: successRecord.path,
      status: successRecord.status,
      usage: successRecord.usage,
    },
    {
      test_case: "chat-direct",
      path: "/v1/chat/completions?include_usage=1",
      status: 200,
      usage: {
        prompt_cache_hit_tokens: 80,
        prompt_cache_miss_tokens: 20,
      },
    },
  );
  assert.equal(successRecordText.includes("private source text"), false);
  assert.equal(successRecordText.includes("private-token"), false);

  await new Promise((resolve) => {
    const request = http.request(
      `http://127.0.0.1:${proxyAddress.port}/aborted/v1/aborted`,
      { method: "POST", headers: { "content-type": "application/json" } },
      (response) => {
        response.resume();
        response.on("aborted", resolve);
        response.on("error", resolve);
        response.on("close", resolve);
      },
    );
    request.on("error", resolve);
    request.end('{"model":"test","messages":[]}');
  });

  const records = fs
    .readFileSync(logPath, "utf8")
    .trim()
    .split("\n")
    .map((line) => JSON.parse(line));
  assert.equal(records[1].test_case, "aborted");
  assert.equal(records[1].status, 200);
  assert.match(records[1].error, /upstream response (aborted|closed early)/);
});
