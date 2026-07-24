import assert from "node:assert/strict";
import test from "node:test";

import {
  cacheCounters,
  formatReport,
  summarizeGroups,
} from "./cache-proxy-report.mjs";

function record({
  protocol,
  hit,
  miss,
  system = "system-a",
  tools = "tools-a",
}) {
  const usage =
    protocol === "anthropic_messages"
      ? { cache_read_input_tokens: hit, input_tokens: miss }
      : { prompt_cache_hit_tokens: hit, prompt_cache_miss_tokens: miss };
  return {
    test_case: "case-a",
    protocol,
    system_sha256: system,
    tools_sha256: tools,
    tool_names: ["exec"],
    message_count: 3,
    usage,
  };
}

test("cacheCounters normalizes both provider protocols", () => {
  assert.deepEqual(
    cacheCounters(record({ protocol: "chat_completions", hit: 80, miss: 20 })),
    { hit: 80, miss: 20, creation: 0, total: 100 },
  );
  assert.deepEqual(
    cacheCounters(
      record({ protocol: "anthropic_messages", hit: 64, miss: 36 }),
    ),
    { hit: 64, miss: 36, creation: 0, total: 100 },
  );
});

test("summarizeGroups separates model-visible prefixes and reports warm hit rate", () => {
  const summaries = summarizeGroups([
    record({ protocol: "chat_completions", hit: 0, miss: 100 }),
    record({ protocol: "chat_completions", hit: 90, miss: 10 }),
    record({
      protocol: "chat_completions",
      hit: 5,
      miss: 95,
      system: "system-b",
    }),
  ]);

  assert.equal(summaries.length, 2);
  assert.deepEqual(summaries[0], {
    test_case: "case-a",
    protocol: "chat_completions",
    system_sha256: "system-a",
    tools_sha256: "tools-a",
    tool_names: ["exec"],
    request_count: 2,
    cache_hit_tokens: 90,
    cache_miss_tokens: 110,
    cache_hit_percent: 45,
    warm_cache_hit_percent: 90,
    last_cache_hit_percent: 90,
  });
  assert.match(formatReport(summaries), /case-a\tchat_completions\t2\t1/);
});
