#!/usr/bin/env python3
"""Summarize captured model API fixtures into stable trajectory shape JSON.

The capture proxy stores full redacted request/response payloads so developers
can debug real provider behavior. This summarizer keeps only structural shape:
message roles, content block kinds, tool names, schema field names, tool-call
edges, and streaming event types. It intentionally omits prompt text and tool
output text by default so the output can be checked in or shared more safely.
"""

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any
from typing import Optional


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Summarize trajectory capture fixtures without prompt text."
    )
    parser.add_argument(
        "paths",
        nargs="+",
        help="Capture dump directories or individual fixture JSON files.",
    )
    parser.add_argument("--output", help="Write summary JSON to this path.")
    parser.add_argument(
        "--include-text",
        action="store_true",
        help="Include text snippets. Off by default to keep summaries shareable.",
    )
    return parser.parse_args()


def fixture_paths(paths: list[str]) -> list[Path]:
    fixtures: list[Path] = []
    for raw_path in paths:
        path = Path(raw_path)
        if path.is_dir():
            fixtures.extend(sorted(path.glob("*.json")))
        else:
            fixtures.append(path)
    return sorted(fixtures)


def maybe_text_shape(value: Any, include_text: bool) -> dict[str, Any]:
    if value is None:
        return {"kind": "none", "chars": 0}
    if isinstance(value, str):
        shape: dict[str, Any] = {"kind": "text", "chars": len(value)}
        if include_text:
            shape["text"] = value
        return shape
    return {"kind": type(value).__name__}


def schema_field_shape(schema: Any) -> dict[str, Any]:
    if not isinstance(schema, dict):
        return {"type": type(schema).__name__, "properties": [], "required": []}
    properties = schema.get("properties")
    required = schema.get("required")
    return {
        "type": schema.get("type"),
        "properties": sorted(properties.keys()) if isinstance(properties, dict) else [],
        "required": sorted(required) if isinstance(required, list) else [],
    }


def summarize_tool(tool: Any) -> dict[str, Any]:
    if not isinstance(tool, dict):
        return {"kind": type(tool).__name__}

    if isinstance(tool.get("function"), dict):
        function = tool["function"]
        return {
            "name": function.get("name"),
            "description_chars": len(function.get("description") or ""),
            "schema": schema_field_shape(function.get("parameters")),
        }

    return {
        "name": tool.get("name"),
        "description_chars": len(tool.get("description") or ""),
        "schema": schema_field_shape(tool.get("input_schema")),
    }


def summarize_chat_tool_call(call: Any) -> dict[str, Any]:
    if not isinstance(call, dict):
        return {"kind": type(call).__name__}
    function = call.get("function") if isinstance(call.get("function"), dict) else {}
    arguments = function.get("arguments")
    argument_keys: list[str] = []
    if isinstance(arguments, str):
        try:
            parsed = json.loads(arguments)
            if isinstance(parsed, dict):
                argument_keys = sorted(parsed.keys())
        except json.JSONDecodeError:
            argument_keys = []
    elif isinstance(arguments, dict):
        argument_keys = sorted(arguments.keys())
    return {
        "id": call.get("id"),
        "type": call.get("type"),
        "name": function.get("name"),
        "argument_keys": argument_keys,
    }


def summarize_anthropic_block(block: Any, include_text: bool) -> dict[str, Any]:
    if not isinstance(block, dict):
        return {"type": type(block).__name__}

    block_type = block.get("type")
    summary: dict[str, Any] = {"type": block_type}
    if block_type in {"text", "thinking"}:
        text = block.get("text") or block.get("thinking")
        summary.update(maybe_text_shape(text, include_text))
    elif block_type == "tool_use":
        tool_input = block.get("input")
        summary.update(
            {
                "id": block.get("id"),
                "name": block.get("name"),
                "input_keys": sorted(tool_input.keys())
                if isinstance(tool_input, dict)
                else [],
            }
        )
    elif block_type == "tool_result":
        content = block.get("content")
        summary.update(
            {
                "tool_use_id": block.get("tool_use_id"),
                "is_error": block.get("is_error"),
                "content": summarize_content(content, include_text),
            }
        )
    elif block_type == "image":
        source = block.get("source") if isinstance(block.get("source"), dict) else {}
        summary.update(
            {"source_type": source.get("type"), "media_type": source.get("media_type")}
        )
    return summary


def summarize_content(content: Any, include_text: bool) -> dict[str, Any]:
    if isinstance(content, list):
        blocks = [summarize_anthropic_block(block, include_text) for block in content]
        return {
            "kind": "blocks",
            "count": len(blocks),
            "types": [block.get("type") for block in blocks],
            "blocks": blocks,
        }
    if isinstance(content, str) or content is None:
        return maybe_text_shape(content, include_text)
    return {"kind": type(content).__name__}


def summarize_message(message: Any, include_text: bool) -> dict[str, Any]:
    if not isinstance(message, dict):
        return {"kind": type(message).__name__}

    summary: dict[str, Any] = {
        "role": message.get("role"),
        "content": summarize_content(message.get("content"), include_text),
    }
    if message.get("tool_call_id") is not None:
        summary["tool_call_id"] = message.get("tool_call_id")
    if isinstance(message.get("tool_calls"), list):
        summary["tool_calls"] = [
            summarize_chat_tool_call(call) for call in message["tool_calls"]
        ]
    return summary


def detect_api_kind(path: str, body: Any) -> str:
    if "/chat/completions" in path:
        return "chat_completions"
    if "/messages" in path:
        return "anthropic_messages"
    if "/models" in path:
        return "models"
    if isinstance(body, dict) and "messages" in body and "tools" in body:
        return "model_request"
    return "other"


def summarize_sse_response(body: Any) -> dict[str, Any]:
    if not isinstance(body, str) or "data:" not in body:
        return {"kind": type(body).__name__}

    event_names: list[str] = []
    data_types: list[str] = []
    tool_call_names: list[str] = []
    block_types: list[str] = []
    current_event: Optional[str] = None

    for raw_line in body.splitlines():
        line = raw_line.strip()
        if line.startswith("event:"):
            current_event = line.split(":", 1)[1].strip()
            event_names.append(current_event)
            continue
        if not line.startswith("data:"):
            continue
        payload = line.split(":", 1)[1].strip()
        if not payload or payload == "[DONE]":
            continue
        try:
            data = json.loads(payload)
        except json.JSONDecodeError:
            continue
        if isinstance(data, dict):
            data_type = data.get("type") or current_event or data.get("object")
            if data_type:
                data_types.append(str(data_type))
            for choice in data.get("choices") or []:
                if not isinstance(choice, dict):
                    continue
                delta = (
                    choice.get("delta") if isinstance(choice.get("delta"), dict) else {}
                )
                message = (
                    choice.get("message")
                    if isinstance(choice.get("message"), dict)
                    else {}
                )
                for call in delta.get("tool_calls") or message.get("tool_calls") or []:
                    if isinstance(call, dict) and isinstance(
                        call.get("function"), dict
                    ):
                        name = call["function"].get("name")
                        if name:
                            tool_call_names.append(name)
            content_block = data.get("content_block")
            if isinstance(content_block, dict) and content_block.get("type"):
                block_types.append(content_block["type"])

    return {
        "kind": "sse",
        "event_counts": dict(sorted(Counter(event_names).items())),
        "data_type_counts": dict(sorted(Counter(data_types).items())),
        "tool_call_names": sorted(set(tool_call_names)),
        "content_block_types": block_types,
    }


def summarize_fixture(path: Path, include_text: bool) -> dict[str, Any]:
    data = json.loads(path.read_text(encoding="utf-8"))
    request_body = data.get("request", {}).get("body")
    response_body = data.get("response", {}).get("body")
    client_path = data.get("client_path") or ""
    tools = request_body.get("tools") if isinstance(request_body, dict) else None
    messages = request_body.get("messages") if isinstance(request_body, dict) else None

    request: dict[str, Any] = {"body_kind": type(request_body).__name__}
    if isinstance(request_body, dict):
        request.update(
            {
                "model": request_body.get("model"),
                "body_keys": sorted(request_body.keys()),
                "system": summarize_content(request_body.get("system"), include_text)
                if "system" in request_body
                else None,
                "max_tokens": request_body.get("max_tokens"),
                "stream": request_body.get("stream"),
                "tool_choice": request_body.get("tool_choice"),
                "messages": [
                    summarize_message(message, include_text) for message in messages
                ]
                if isinstance(messages, list)
                else [],
                "tools": [summarize_tool(tool) for tool in tools]
                if isinstance(tools, list)
                else [],
            }
        )

    return {
        "fixture": path.name,
        "sequence": data.get("sequence"),
        "method": data.get("method"),
        "client_path": client_path,
        "api_kind": detect_api_kind(client_path, request_body),
        "upstream_base": data.get("upstream_base"),
        "response_status": data.get("response", {}).get("status"),
        "error": data.get("error"),
        "request": request,
        "response": summarize_sse_response(response_body),
    }


def main() -> None:
    args = parse_args()
    fixtures = [
        summarize_fixture(path, args.include_text) for path in fixture_paths(args.paths)
    ]
    summary = {
        "fixture_count": len(fixtures),
        "api_kind_counts": dict(
            sorted(Counter(item["api_kind"] for item in fixtures).items())
        ),
        "fixtures": fixtures,
    }
    text = json.dumps(summary, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        Path(args.output).write_text(text, encoding="utf-8")
    else:
        print(text, end="")


if __name__ == "__main__":
    main()
