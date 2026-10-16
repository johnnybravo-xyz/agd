#!/usr/bin/env python3
"""Two earlier Codex changes through `agd mcp`, for the Agents tab. Codex
names its MCP client codex-mcp-client, which agd journals as codex.

usage: seed_codex.py AGD
"""
import json
import subprocess
import sys

agd = sys.argv[1]
process = subprocess.Popen(
    [agd, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
)


def call(index, method, params):
    process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": index, "method": method, "params": params}) + "\n")
    process.stdin.flush()
    while True:
        message = json.loads(process.stdout.readline())
        if message.get("id") != index:
            continue
        if message.get("result", {}).get("isError"):
            sys.exit(f"{params.get('name', method)} failed: {message['result']['content'][0]['text']}")
        return message


call(0, "initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                       "clientInfo": {"name": "codex-mcp-client", "version": "1.0"}})
process.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
agenda = call(1, "tools/call", {"name": "agenda", "arguments": {"span": "week"}})
entries = json.loads(agenda["result"]["content"][0]["text"])
refs = {entry["item"]["title"]: entry["item"]["ref"] for entry in entries}
call(2, "tools/call", {"name": "capture", "arguments": {
    "template_key": "t", "title": "Pin pyarrow below 17 until the CSV writer fix lands", "tags": ["deps"],
    "body": "pyarrow 17 quotes empty strings in the CSV writer; the nightly export diff shows it."}})
call(3, "tools/call", {"name": "set_state", "arguments": {"ref": refs["Rotate staging certs"], "todo": "DONE"}})
process.stdin.close()
process.wait(timeout=10)
