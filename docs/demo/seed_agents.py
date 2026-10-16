#!/usr/bin/env python3
"""Makes two agent changes through `agd mcp`, as Claude Code and Codex would,
so the demo's Agent review screen has something to show.

usage: seed_agents.py AGD CONFIG
"""
import json
import subprocess
import sys

agd, config = sys.argv[1], sys.argv[2]


def session(client, calls):
    process = subprocess.Popen(
        [agd, "--config", config, "mcp"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
    )
    messages = [
        {"jsonrpc": "2.0", "id": 0, "method": "initialize",
         "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                    "clientInfo": {"name": client, "version": "1.0"}}},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 1, "method": "tools/call",
         "params": {"name": "agenda", "arguments": {"span": "week"}}},
    ]
    for message in messages:
        process.stdin.write(json.dumps(message) + "\n")
    process.stdin.flush()
    entries = []
    while True:
        message = json.loads(process.stdout.readline())
        if message.get("id") == 1:
            entries = json.loads(message["result"]["content"][0]["text"])
            break
    refs = {entry["item"]["title"]: entry["item"]["ref"] for entry in entries}
    for index, (tool, arguments) in enumerate(calls, start=2):
        if "ref" in arguments:
            arguments["ref"] = refs[arguments["ref"]]
        process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": index, "method": "tools/call",
                                        "params": {"name": tool, "arguments": arguments}}) + "\n")
        process.stdin.flush()
        while (message := json.loads(process.stdout.readline())).get("id") != index:
            pass
        # A refused call answers agd.error/v1 with isError set; stop the demo on it.
        if message["result"].get("isError"):
            sys.exit(f"{tool} failed: {message['result']['content'][0]['text']}")
    process.stdin.close()
    process.wait(timeout=10)


session("claude-code", [("capture", {
    "template": "t", "title": "Benchmark the agenda on a 5k-heading file", "tags": ["perf"],
    "properties": {"REPO": "agd", "SOURCE": "[[file:agd-core/src/agenda/engine.rs::41]]"}})])
# Codex's own client name; agd journals it as mcp:codex.
session("codex-mcp-client", [("set_state", {"ref": "Rotate staging certs", "state": "DONE"})])
