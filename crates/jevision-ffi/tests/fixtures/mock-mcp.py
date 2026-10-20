"""An MCP server over standard input and output for the tests: an issue tracker with one tool
that reads and one that changes. Every tool call is appended to the file given as the first
argument, so a test can tell which calls really happened."""

import json
import sys

LOG = sys.argv[1]

TOOLS = [
    {
        "name": "find_issue",
        "title": "Find Issue",
        "description": "Finds the issue about a test.",
        "inputSchema": {"type": "object", "properties": {"test": {"type": "string"}}},
        "annotations": {"readOnlyHint": True},
    },
    {
        "name": "create_issue",
        "description": "Files a new issue.",
        "inputSchema": {"type": "object", "properties": {"title": {"type": "string"}}},
    },
]

ANSWERS = {
    "find_issue": "ISSUE-7: the Save button moved to the toolbar",
    "create_issue": "Created ISSUE-8",
}

# Noise a program may print before it speaks the protocol.
print("mock tracker starting", flush=True)
print("starting on stderr", file=sys.stderr, flush=True)

for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    if "id" not in message:
        continue
    if method == "initialize":
        result = {
            "protocolVersion": message["params"]["protocolVersion"],
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "tracker", "version": "1"},
        }
    elif method == "tools/list":
        cursor = message.get("params", {}).get("cursor")
        # Two pages, to check that the client follows the cursor.
        if cursor is None:
            result = {"tools": TOOLS[:1], "nextCursor": "2"}
        else:
            result = {"tools": TOOLS[1:]}
    elif method == "tools/call":
        name = message["params"]["name"]
        with open(LOG, "a") as log:
            log.write(name + " " + json.dumps(message["params"].get("arguments", {})) + "\n")
        result = {"content": [{"type": "text", "text": ANSWERS[name]}], "isError": False}
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": message["id"],
                          "error": {"code": -32601, "message": "Method not found"}}), flush=True)
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": message["id"], "result": result}), flush=True)
