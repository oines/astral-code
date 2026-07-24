#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import https from "node:https";
import { pathToFileURL } from "node:url";
import zlib from "node:zlib";

const DEFAULT_HOST = "127.0.0.1";
const DEFAULT_PORT = 18080;
const DEFAULT_UPSTREAM_ORIGIN = "https://api.deepseek.com";

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

export function decodeBody(buffer, encoding) {
  if (encoding === "gzip") return zlib.gunzipSync(buffer);
  if (encoding === "deflate") return zlib.inflateSync(buffer);
  if (encoding === "br") return zlib.brotliDecompressSync(buffer);
  return buffer;
}

export function commonPrefixLength(left, right) {
  const limit = Math.min(left.length, right.length);
  let index = 0;
  while (index < limit && left[index] === right[index]) index += 1;
  return index;
}

function toolName(tool) {
  return (
    tool?.function?.name ?? tool?.name ?? tool?.custom?.name ?? "<unknown>"
  );
}

function mergeUsage(target, candidate) {
  if (candidate && typeof candidate === "object")
    Object.assign(target, candidate);
}

export function parseUsage(protocol, responseBuffer, encoding) {
  let text;
  try {
    text = decodeBody(responseBuffer, encoding).toString("utf8");
  } catch {
    return null;
  }

  const usage = {};
  const trimmed = text.trim();
  if (trimmed.startsWith("{")) {
    try {
      const response = JSON.parse(trimmed);
      mergeUsage(usage, response.usage);
    } catch {
      // Streaming responses are parsed line by line below.
    }
  }

  for (const line of text.split(/\r?\n/)) {
    if (!line.startsWith("data:")) continue;
    const payload = line.slice(5).trim();
    if (!payload || payload === "[DONE]") continue;
    let event;
    try {
      event = JSON.parse(payload);
    } catch {
      continue;
    }

    if (protocol === "chat_completions") {
      mergeUsage(usage, event.usage);
    } else {
      mergeUsage(usage, event.message?.usage);
      mergeUsage(usage, event.usage);
    }
  }
  return Object.keys(usage).length > 0 ? usage : null;
}

export function summarizeRequest({
  requestId,
  testCase,
  protocol,
  method,
  upstreamPath,
  encodedBytes,
  decodedBody,
  body,
  previous,
}) {
  const messageHashes = Array.isArray(body.messages)
    ? body.messages.map((message) => sha256(JSON.stringify(message)))
    : [];
  const tools = Array.isArray(body.tools) ? body.tools : [];
  const systemValue =
    protocol === "anthropic_messages"
      ? (body.system ?? null)
      : (body.messages ?? []).filter((message) => message?.role === "system");
  const rawBody = decodedBody.toString("utf8");

  return {
    summary: {
      request_id: requestId,
      test_case: testCase,
      protocol,
      method,
      path: upstreamPath,
      model: body.model ?? null,
      request_bytes: encodedBytes,
      decoded_request_bytes: decodedBody.length,
      request_sha256: sha256(decodedBody),
      system_sha256: sha256(JSON.stringify(systemValue)),
      tools_sha256: sha256(JSON.stringify(tools)),
      tool_names: tools.map(toolName),
      message_count: messageHashes.length,
      message_hashes: messageHashes,
      common_prefix_messages: previous
        ? commonPrefixLength(previous.messageHashes, messageHashes)
        : 0,
      common_prefix_bytes: previous
        ? commonPrefixLength(previous.rawBody, rawBody)
        : 0,
      previous_request_id: previous?.requestId ?? null,
    },
    current: { requestId, rawBody, messageHashes },
  };
}

function parseArgs(argv) {
  const options = {
    host: DEFAULT_HOST,
    port: DEFAULT_PORT,
    upstreamOrigin: DEFAULT_UPSTREAM_ORIGIN,
    logPath: null,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    const value = argv[index + 1];
    if (argument === "--help") {
      process.stdout.write(
        "Usage: just cache-proxy --log PATH [--port PORT] [--host HOST] [--upstream-origin URL]\n\n" +
          "Configure provider base URLs as http://HOST:PORT/TEST_CASE/<upstream base path>.\n" +
          "The first path segment labels the test case and is removed before forwarding.\n",
      );
      return null;
    }
    if (argument === "--host") options.host = value;
    else if (argument === "--port") options.port = Number(value);
    else if (argument === "--upstream-origin") options.upstreamOrigin = value;
    else if (argument === "--log") options.logPath = value;
    else throw new Error(`unknown argument: ${argument}`);
    index += 1;
  }
  if (!options.logPath) throw new Error("--log PATH is required");
  if (
    !Number.isInteger(options.port) ||
    options.port < 1 ||
    options.port > 65535
  ) {
    throw new Error(`invalid port: ${options.port}`);
  }
  return options;
}

function appendLog(logPath, record) {
  fs.appendFileSync(logPath, `${JSON.stringify(record)}\n`, { mode: 0o600 });
}

export function createProxyServer({ upstreamOrigin, logPath }) {
  const upstreamUrl = new URL(upstreamOrigin);
  const transport = upstreamUrl.protocol === "https:" ? https : http;
  const previousByCase = new Map();
  let nextRequestId = 1;

  return http.createServer((request, response) => {
    const startedAt = Date.now();
    const requestId = nextRequestId++;
    const chunks = [];

    request.on("data", (chunk) => chunks.push(chunk));
    request.on("end", () => {
      const encodedRequest = Buffer.concat(chunks);
      let decodedRequest;
      let body;
      try {
        decodedRequest =
          encodedRequest.length === 0
            ? Buffer.alloc(0)
            : decodeBody(encodedRequest, request.headers["content-encoding"]);
        body =
          decodedRequest.length === 0
            ? {}
            : JSON.parse(decodedRequest.toString("utf8"));
      } catch (error) {
        response.writeHead(400, { "content-type": "text/plain" });
        response.end("proxy could not decode request body");
        appendLog(logPath, { request_id: requestId, error: String(error) });
        return;
      }

      const pathParts = (request.url ?? "/").split("/").filter(Boolean);
      const testCase = pathParts.shift() ?? "unknown";
      const upstreamPath = `${upstreamUrl.pathname.replace(/\/$/, "")}/${pathParts.join("/")}`;
      const protocol = upstreamPath.endsWith("/messages")
        ? "anthropic_messages"
        : "chat_completions";
      const { summary, current } = summarizeRequest({
        requestId,
        testCase,
        protocol,
        method: request.method,
        upstreamPath,
        encodedBytes: encodedRequest.length,
        decodedBody: decodedRequest,
        body,
        previous: previousByCase.get(testCase),
      });
      previousByCase.set(testCase, current);

      const headers = { ...request.headers, host: upstreamUrl.host };
      delete headers.connection;
      const responseChunks = [];
      let responseEncoding;
      let responseStatus = null;
      let finalized = false;
      const finalize = (error) => {
        if (finalized) return;
        finalized = true;
        const responseBuffer = Buffer.concat(responseChunks);
        appendLog(logPath, {
          ...summary,
          status: responseStatus,
          duration_ms: Date.now() - startedAt,
          response_bytes: responseBuffer.length,
          usage: parseUsage(protocol, responseBuffer, responseEncoding),
          ...(error ? { error: String(error) } : {}),
        });
      };
      const upstream = transport.request(
        {
          protocol: upstreamUrl.protocol,
          hostname: upstreamUrl.hostname,
          port: upstreamUrl.port || undefined,
          method: request.method,
          path: upstreamPath,
          headers,
        },
        (upstreamResponse) => {
          responseStatus = upstreamResponse.statusCode ?? 502;
          responseEncoding = upstreamResponse.headers["content-encoding"];
          response.writeHead(responseStatus, upstreamResponse.headers);
          upstreamResponse.on("data", (chunk) => {
            responseChunks.push(chunk);
            response.write(chunk);
          });
          upstreamResponse.on("end", () => {
            response.end();
            finalize();
          });
          const abort = (error) => {
            finalize(error);
            if (!response.destroyed) response.destroy(error);
          };
          upstreamResponse.on("aborted", () =>
            abort(new Error("upstream response aborted")),
          );
          upstreamResponse.on("error", abort);
          upstreamResponse.on("close", () => {
            if (!upstreamResponse.complete)
              abort(new Error("upstream response closed early"));
          });
        },
      );

      upstream.on("error", (error) => {
        if (!response.headersSent) {
          response.writeHead(502, { "content-type": "text/plain" });
        }
        response.end("upstream proxy error");
        finalize(error);
      });
      upstream.end(encodedRequest);
    });
  });
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (!options) return;
  const server = createProxyServer(options);
  server.listen(options.port, options.host, () => {
    process.stdout.write(
      `cache proxy listening on http://${options.host}:${options.port}; upstream=${options.upstreamOrigin}\n`,
    );
  });
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  main().catch((error) => {
    process.stderr.write(`${error.stack ?? error}\n`);
    process.exitCode = 1;
  });
}
