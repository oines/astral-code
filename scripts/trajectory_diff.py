#!/usr/bin/env python3
"""Compare two summarized model API trajectories.

Use `trajectory_capture_proxy.py` to capture raw requests, then
`trajectory_summarize.py` to redact and normalize them. This script compares
two summary JSON files so Astral request shapes can be checked against Claude
Code or another reference harness without sharing prompt or tool output text.
"""

import argparse
import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any


MODEL_API_KINDS = {"anthropic_messages", "chat_completions", "model_request"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Diff two trajectory summary JSON files by structural shape."
    )
    parser.add_argument("--left", required=True, help="Left summary JSON path.")
    parser.add_argument("--right", required=True, help="Right summary JSON path.")
    parser.add_argument("--left-label", default="left")
    parser.add_argument("--right-label", default="right")
    parser.add_argument("--output", help="Write diff output to this path.")
    parser.add_argument(
        "--format",
        choices=["json", "markdown"],
        default="json",
        help="Output format.",
    )
    parser.add_argument(
        "--fail-on-diff",
        action="store_true",
        help="Exit with status 1 when structural differences are present.",
    )
    return parser.parse_args()


def load_summary(path: str) -> dict[str, Any]:
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    if not isinstance(data, dict) or not isinstance(data.get("fixtures"), list):
        raise ValueError(f"{path} is not a trajectory summary JSON file")
    return data


def model_fixtures(summary: dict[str, Any]) -> list[dict[str, Any]]:
    return [
        fixture
        for fixture in summary.get("fixtures", [])
        if isinstance(fixture, dict) and fixture.get("api_kind") in MODEL_API_KINDS
    ]


def counter_to_dict(counter: Counter[str]) -> dict[str, int]:
    return dict(sorted(counter.items()))


def content_types(content: Any) -> list[str]:
    if not isinstance(content, dict):
        return [type(content).__name__]
    if isinstance(content.get("types"), list):
        return [str(item) for item in content["types"]]
    kind = content.get("kind")
    return [str(kind)] if kind is not None else []


def message_block_types(message: dict[str, Any]) -> list[str]:
    content = message.get("content")
    if not isinstance(content, dict):
        return []
    blocks = content.get("blocks")
    if not isinstance(blocks, list):
        return content_types(content)
    block_types = []
    for block in blocks:
        if isinstance(block, dict) and block.get("type") is not None:
            block_types.append(str(block["type"]))
    return block_types


def request_messages(request: dict[str, Any]) -> list[dict[str, Any]]:
    messages = request.get("messages")
    if not isinstance(messages, list):
        return []
    return [message for message in messages if isinstance(message, dict)]


def tool_schema_key(tool: dict[str, Any]) -> tuple[tuple[str, ...], tuple[str, ...]]:
    schema = tool.get("schema")
    if not isinstance(schema, dict):
        return ((), ())
    properties = schema.get("properties")
    required = schema.get("required")
    return (
        tuple(str(item) for item in properties) if isinstance(properties, list) else (),
        tuple(str(item) for item in required) if isinstance(required, list) else (),
    )


def aggregate(summary: dict[str, Any]) -> dict[str, Any]:
    fixtures = model_fixtures(summary)
    body_keys: Counter[str] = Counter()
    models: Counter[str] = Counter()
    request_api_kinds: Counter[str] = Counter()
    request_paths: Counter[str] = Counter()
    tool_names: Counter[str] = Counter()
    tool_schema: dict[str, set[tuple[tuple[str, ...], tuple[str, ...]]]] = {}
    message_role_sequences: list[list[str]] = []
    message_field_sets: Counter[str] = Counter()
    message_block_type_counts: Counter[str] = Counter()
    tool_call_names: Counter[str] = Counter()
    tool_result_count = 0
    reasoning_content_messages = 0
    thinking_blocks = 0
    response_event_counts: Counter[str] = Counter()
    response_data_type_counts: Counter[str] = Counter()
    response_content_block_type_counts: Counter[str] = Counter()

    for fixture in fixtures:
        request_api_kinds.update([str(fixture.get("api_kind"))])
        request_paths.update([str(fixture.get("client_path"))])
        request = (
            fixture.get("request") if isinstance(fixture.get("request"), dict) else {}
        )
        model = request.get("model")
        if model is not None:
            models.update([str(model)])
        for key in request.get("body_keys") or []:
            body_keys.update([str(key)])

        for tool in request.get("tools") or []:
            if not isinstance(tool, dict):
                continue
            name = tool.get("name")
            if name is None:
                continue
            tool_name = str(name)
            tool_names.update([tool_name])
            tool_schema.setdefault(tool_name, set()).add(tool_schema_key(tool))

        roles = []
        for message in request_messages(request):
            role = str(message.get("role"))
            roles.append(role)
            message_field_sets.update(
                ["|".join(str(key) for key in message.get("fields") or [])]
            )
            for block_type in message_block_types(message):
                message_block_type_counts.update([f"{role}:{block_type}"])
                if block_type == "thinking":
                    thinking_blocks += 1
            if "reasoning_content" in message:
                reasoning_content_messages += 1
            if message.get("tool_call_id") is not None:
                tool_result_count += 1
            for tool_call in message.get("tool_calls") or []:
                if isinstance(tool_call, dict) and tool_call.get("name") is not None:
                    tool_call_names.update([str(tool_call["name"])])
            content = message.get("content")
            if isinstance(content, dict):
                for block in content.get("blocks") or []:
                    if not isinstance(block, dict):
                        continue
                    if (
                        block.get("type") == "tool_use"
                        and block.get("name") is not None
                    ):
                        tool_call_names.update([str(block["name"])])
                    if block.get("type") == "tool_result":
                        tool_result_count += 1
        if roles:
            message_role_sequences.append(roles)

        response = (
            fixture.get("response") if isinstance(fixture.get("response"), dict) else {}
        )
        response_event_counts.update(response.get("event_counts") or {})
        response_data_type_counts.update(response.get("data_type_counts") or {})
        for name in response.get("tool_call_names") or []:
            tool_call_names.update([str(name)])
        for block_type in response.get("content_block_types") or []:
            response_content_block_type_counts.update([str(block_type)])

    return {
        "request_count": len(fixtures),
        "api_kind_counts": counter_to_dict(request_api_kinds),
        "request_path_counts": counter_to_dict(request_paths),
        "models": counter_to_dict(models),
        "body_keys": counter_to_dict(body_keys),
        "tool_names": counter_to_dict(tool_names),
        "tool_schema": {
            name: [
                {"properties": list(properties), "required": list(required)}
                for properties, required in sorted(shapes)
            ]
            for name, shapes in sorted(tool_schema.items())
        },
        "message_role_sequences": message_role_sequences,
        "message_field_sets": counter_to_dict(message_field_sets),
        "message_block_type_counts": counter_to_dict(message_block_type_counts),
        "tool_call_names": counter_to_dict(tool_call_names),
        "tool_result_count": tool_result_count,
        "reasoning_content_messages": reasoning_content_messages,
        "thinking_blocks": thinking_blocks,
        "response_event_counts": counter_to_dict(response_event_counts),
        "response_data_type_counts": counter_to_dict(response_data_type_counts),
        "response_content_block_type_counts": counter_to_dict(
            response_content_block_type_counts
        ),
    }


def set_diff(left: set[str], right: set[str]) -> dict[str, list[str]]:
    return {
        "common": sorted(left & right),
        "only_left": sorted(left - right),
        "only_right": sorted(right - left),
    }


def key_set(value: dict[str, Any], key: str) -> set[str]:
    item = value.get(key)
    if isinstance(item, dict):
        return set(item.keys())
    return set()


def changed_tool_schemas(
    left: dict[str, Any], right: dict[str, Any]
) -> dict[str, dict[str, Any]]:
    left_schema = (
        left.get("tool_schema") if isinstance(left.get("tool_schema"), dict) else {}
    )
    right_schema = (
        right.get("tool_schema") if isinstance(right.get("tool_schema"), dict) else {}
    )
    changed = {}
    for name in sorted(set(left_schema) & set(right_schema)):
        if left_schema[name] != right_schema[name]:
            changed[name] = {"left": left_schema[name], "right": right_schema[name]}
    return changed


def compare(
    left_summary: dict[str, Any],
    right_summary: dict[str, Any],
    left_label: str,
    right_label: str,
) -> dict[str, Any]:
    left = aggregate(left_summary)
    right = aggregate(right_summary)
    diff = {
        "body_keys": set_diff(key_set(left, "body_keys"), key_set(right, "body_keys")),
        "tool_names": set_diff(
            key_set(left, "tool_names"), key_set(right, "tool_names")
        ),
        "tool_schema_changed": changed_tool_schemas(left, right),
        "message_field_sets": set_diff(
            key_set(left, "message_field_sets"), key_set(right, "message_field_sets")
        ),
        "message_block_types": set_diff(
            key_set(left, "message_block_type_counts"),
            key_set(right, "message_block_type_counts"),
        ),
        "tool_call_names": set_diff(
            key_set(left, "tool_call_names"), key_set(right, "tool_call_names")
        ),
        "response_event_types": set_diff(
            key_set(left, "response_event_counts"),
            key_set(right, "response_event_counts"),
        ),
        "response_data_types": set_diff(
            key_set(left, "response_data_type_counts"),
            key_set(right, "response_data_type_counts"),
        ),
        "response_content_block_types": set_diff(
            key_set(left, "response_content_block_type_counts"),
            key_set(right, "response_content_block_type_counts"),
        ),
        "scalar_counts": {
            "request_count": {
                left_label: left["request_count"],
                right_label: right["request_count"],
            },
            "tool_result_count": {
                left_label: left["tool_result_count"],
                right_label: right["tool_result_count"],
            },
            "reasoning_content_messages": {
                left_label: left["reasoning_content_messages"],
                right_label: right["reasoning_content_messages"],
            },
            "thinking_blocks": {
                left_label: left["thinking_blocks"],
                right_label: right["thinking_blocks"],
            },
        },
    }
    return {
        "left_label": left_label,
        "right_label": right_label,
        "left": left,
        "right": right,
        "diff": diff,
        "has_structural_diff": has_structural_diff(diff),
    }


def has_structural_diff(diff: dict[str, Any]) -> bool:
    for key, value in diff.items():
        if key == "scalar_counts":
            for counts in value.values():
                if len(set(counts.values())) > 1:
                    return True
            continue
        if key == "tool_schema_changed":
            if value:
                return True
            continue
        if isinstance(value, dict):
            if value.get("only_left") or value.get("only_right"):
                return True
    return False


def render_markdown(report: dict[str, Any]) -> str:
    left_label = report["left_label"]
    right_label = report["right_label"]
    lines = [
        f"# Trajectory Diff: {left_label} vs {right_label}",
        "",
        f"- Structural diff: `{report['has_structural_diff']}`",
        f"- Requests: `{left_label}={report['left']['request_count']}`, `{right_label}={report['right']['request_count']}`",
        "",
    ]
    diff = report["diff"]
    for title, key in [
        ("Tools", "tool_names"),
        ("Body Keys", "body_keys"),
        ("Message Fields", "message_field_sets"),
        ("Message Block Types", "message_block_types"),
        ("Tool Calls", "tool_call_names"),
        ("Response Events", "response_event_types"),
        ("Response Data Types", "response_data_types"),
        ("Response Content Blocks", "response_content_block_types"),
    ]:
        value = diff[key]
        lines.extend(
            [
                f"## {title}",
                "",
                f"- Common: `{', '.join(value['common'])}`",
                f"- Only {left_label}: `{', '.join(value['only_left'])}`",
                f"- Only {right_label}: `{', '.join(value['only_right'])}`",
                "",
            ]
        )

    changed = diff["tool_schema_changed"]
    lines.extend(["## Changed Tool Schemas", ""])
    if changed:
        lines.append("```json")
        lines.append(json.dumps(changed, ensure_ascii=False, indent=2))
        lines.append("```")
    else:
        lines.append("- None")
    lines.append("")
    return "\n".join(lines)


def main() -> int:
    args = parse_args()
    report = compare(
        load_summary(args.left),
        load_summary(args.right),
        args.left_label,
        args.right_label,
    )
    if args.format == "markdown":
        text = render_markdown(report)
    else:
        text = json.dumps(report, ensure_ascii=False, indent=2) + "\n"

    if args.output:
        Path(args.output).write_text(text, encoding="utf-8")
    else:
        print(text, end="")

    if args.fail_on_diff and report["has_structural_diff"]:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
