import fs from "node:fs";
import { pathToFileURL } from "node:url";

function percent(numerator, denominator) {
  return denominator === 0 ? null : (numerator / denominator) * 100;
}

export function cacheCounters(record) {
  const usage = record.usage;
  if (!usage || typeof usage !== "object") return null;

  if (record.protocol === "anthropic_messages") {
    const hit = usage.cache_read_input_tokens ?? 0;
    const miss = usage.input_tokens ?? 0;
    const creation = usage.cache_creation_input_tokens ?? 0;
    return { hit, miss, creation, total: hit + miss + creation };
  }

  const hit = usage.prompt_cache_hit_tokens ?? 0;
  const miss = usage.prompt_cache_miss_tokens ?? 0;
  return { hit, miss, creation: 0, total: hit + miss };
}

export function summarizeGroups(records) {
  const groups = new Map();
  for (const record of records) {
    const counters = cacheCounters(record);
    if (!counters || counters.total === 0) continue;
    const key = [
      record.test_case,
      record.protocol,
      record.system_sha256,
      record.tools_sha256,
    ].join("\0");
    let group = groups.get(key);
    if (!group) {
      group = {
        test_case: record.test_case,
        protocol: record.protocol,
        system_sha256: record.system_sha256,
        tools_sha256: record.tools_sha256,
        tool_names: record.tool_names,
        requests: [],
      };
      groups.set(key, group);
    }
    group.requests.push({ record, counters });
  }

  const summaries = [];
  for (const group of groups.values()) {
    const totals = group.requests.reduce(
      (sum, { counters }) => ({
        hit: sum.hit + counters.hit,
        miss: sum.miss + counters.miss,
        creation: sum.creation + counters.creation,
        total: sum.total + counters.total,
      }),
      { hit: 0, miss: 0, creation: 0, total: 0 },
    );
    const warmRequests = group.requests.slice(1);
    const warm = warmRequests.reduce(
      (sum, { counters }) => ({
        hit: sum.hit + counters.hit,
        total: sum.total + counters.total,
      }),
      { hit: 0, total: 0 },
    );
    const last = group.requests.at(-1);
    summaries.push({
      test_case: group.test_case,
      protocol: group.protocol,
      system_sha256: group.system_sha256,
      tools_sha256: group.tools_sha256,
      tool_names: group.tool_names,
      request_count: group.requests.length,
      cache_hit_tokens: totals.hit,
      cache_miss_tokens: totals.miss,
      cache_hit_percent: percent(totals.hit, totals.total),
      warm_cache_hit_percent: percent(warm.hit, warm.total),
      last_cache_hit_percent: percent(last.counters.hit, last.counters.total),
    });
  }

  return summaries.sort(
    (left, right) =>
      left.test_case.localeCompare(right.test_case) ||
      right.request_count - left.request_count,
  );
}

export function readRecords(paths) {
  return paths.flatMap((path) =>
    fs
      .readFileSync(path, "utf8")
      .split(/\r?\n/)
      .filter(Boolean)
      .map((line) => JSON.parse(line)),
  );
}

function abbreviated(hash) {
  return hash?.slice(0, 12) ?? "-";
}

function displayPercent(value) {
  return value === null ? "-" : `${value.toFixed(2)}%`;
}

export function formatReport(summaries) {
  const lines = [
    [
      "test_case",
      "protocol",
      "requests",
      "tools",
      "system",
      "tool_schema",
      "hit",
      "warm",
      "last",
    ].join("\t"),
  ];
  for (const summary of summaries) {
    lines.push(
      [
        summary.test_case,
        summary.protocol,
        summary.request_count,
        summary.tool_names.length,
        abbreviated(summary.system_sha256),
        abbreviated(summary.tools_sha256),
        displayPercent(summary.cache_hit_percent),
        displayPercent(summary.warm_cache_hit_percent),
        displayPercent(summary.last_cache_hit_percent),
      ].join("\t"),
    );
  }
  return `${lines.join("\n")}\n`;
}

function main(argv) {
  if (argv.length === 0 || argv.includes("--help")) {
    process.stdout.write(
      "Usage: just cache-proxy-report LOG.jsonl [MORE.jsonl ...]\n",
    );
    return;
  }
  process.stdout.write(formatReport(summarizeGroups(readRecords(argv))));
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`${error.stack ?? error}\n`);
    process.exitCode = 1;
  }
}
