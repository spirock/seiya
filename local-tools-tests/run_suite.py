"""Exercise OpenAI tool-call protocol against a local model server.

Only deterministic, allowlisted tools run. No generated shell/code executes.
All file access is confined to a TemporaryDirectory. Raw API replies are saved.
"""
import argparse
import json
import secrets
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path


def tool(name, description, properties, required):
    return {"type": "function", "function": {
        "name": name, "description": description,
        "parameters": {"type": "object", "properties": properties,
                       "required": required, "additionalProperties": False}}}


TOOLS = [
    tool("read_file", "Read a UTF-8 file in the test workspace.",
         {"path": {"type": "string"}}, ["path"]),
    tool("write_file", "Write a UTF-8 file in the test workspace.",
         {"path": {"type": "string"}, "content": {"type": "string"}},
         ["path", "content"]),
    tool("list_files", "List available files in the test workspace.", {}, []),
    tool("lookup_order", "Get current order data from the database by order ID.",
         {"order_id": {"type": "string"}}, ["order_id"]),
    tool("get_shipping", "Look up shipping status by the tracking ID returned by lookup_order.",
         {"tracking_id": {"type": "string"}}, ["tracking_id"]),
]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True)
    parser.add_argument("--url", default="http://127.0.0.1:8081")
    parser.add_argument("--output", required=True)
    parser.add_argument("--repeat", type=int, default=3)
    parser.add_argument("--max-tokens", type=int, default=384)
    parser.add_argument("--reasoning-effort", choices=["none", "low", "medium", "high"])
    parser.add_argument("--temperature", type=float, default=0)
    parser.add_argument("--top-p", type=float, default=1)
    args = parser.parse_args()
    out = Path(args.output)
    out.mkdir(parents=True, exist_ok=True)
    results = []

    def request(messages, tools, *, stream=False):
        body = {"model": args.model, "messages": messages, "tools": tools,
                "temperature": args.temperature, "top_p": args.top_p,
                "max_tokens": args.max_tokens, "stream": stream,
                "chat_template_kwargs": {"enable_thinking": False}}
        if args.reasoning_effort:
            body["reasoning_effort"] = args.reasoning_effort
        req = urllib.request.Request(args.url + "/v1/chat/completions",
                                     data=json.dumps(body).encode(),
                                     headers={"Content-Type": "application/json"})
        start = time.monotonic()
        with urllib.request.urlopen(req, timeout=180) as response:
            if not stream:
                data = json.load(response)
            else:
                chunks = []
                for raw in response:
                    line = raw.decode().strip()
                    if not line.startswith("data: ") or line == "data: [DONE]":
                        continue
                    chunks.append(json.loads(line[6:]))
                calls, content, finish = {}, "", None
                for chunk in chunks:
                    for choice in chunk.get("choices", []):
                        finish = choice.get("finish_reason") or finish
                        delta = choice.get("delta", {})
                        content += delta.get("content") or ""
                        for call in delta.get("tool_calls", []):
                            idx = call.get("index", 0)
                            item = calls.setdefault(idx, {"id": "", "type": "function",
                                                         "function": {"name": "", "arguments": ""}})
                            if call.get("id"):
                                item["id"] = call["id"]
                            for key in ("name", "arguments"):
                                item["function"][key] += call.get("function", {}).get(key, "")
                data = {"choices": [{"finish_reason": finish, "message": {
                    "role": "assistant", "content": content,
                    "tool_calls": [calls[k] for k in sorted(calls)]}}], "chunks": chunks}
        return data, time.monotonic() - start

    for rep in range(args.repeat):
        for case in ("no_tool", "read", "write", "chain", "recover", "stream_read"):
            with tempfile.TemporaryDirectory(prefix="tools-", dir=out) as tmp:
                root = Path(tmp)
                secret = "verified-" + secrets.token_hex(5)
                tracking = "TRACK-" + secrets.token_hex(4)
                status = "arrived-" + secrets.token_hex(4)
                (root / "secret.txt").write_text(secret)
                (root / "available.txt").write_text(secret)
                prompts = {
                    "no_tool": "Responde únicamente HOLA. No necesitas consultar ni modificar archivos.",
                    "read": "Usa read_file para leer secret.txt y responde únicamente con su contenido exacto.",
                    "stream_read": "Usa read_file para leer secret.txt y responde únicamente con su contenido exacto.",
                    "write": 'Guarda en result.txt exactamente este texto: ' + secret + '. Usa write_file; no añadas el punto final al archivo.',
                    "chain": 'Consulta el pedido ORDER-42 con lookup_order. Usa después su tracking_id para get_shipping. Responde únicamente con el status exacto devuelto por get_shipping.',
                    "recover": 'Primero intenta leer missing.txt. Si no existe, usa list_files y lee available.txt. Responde únicamente con el contenido exacto de available.txt.',
                }
                messages = [{"role": "system", "content": "You are testing real tools. Call tools to obtain unknown data; never fabricate results. Use returned tool data exactly. Stop after fulfilling the request."},
                            {"role": "user", "content": prompts[case]}]
                log, calls, errors = [], [], []
                elapsed, tokens, final, valid, completed = 0.0, 0, "", True, False

                def execute(call):
                    nonlocal valid
                    fn = call.get("function", {})
                    name = fn.get("name")
                    values = json.loads(fn.get("arguments", ""))
                    schema = next(t["function"]["parameters"] for t in TOOLS
                                  if t["function"]["name"] == name)
                    if not isinstance(values, dict) or set(values) != set(schema["required"]):
                        raise ValueError("arguments violate schema")
                    if any(not isinstance(value, str) for value in values.values()):
                        raise ValueError("arguments must be strings")
                    calls.append({"name": name, "arguments": values})
                    if not call.get("id") or call.get("type") != "function":
                        valid = False
                        raise ValueError("missing tool call ID/type")
                    if name in ("read_file", "write_file"):
                        path = (root / values["path"]).resolve()
                        if not path.is_relative_to(root.resolve()):
                            raise ValueError("path escapes workspace")
                        if name == "read_file":
                            if not path.is_file():
                                return {"error": "file_not_found", "path": values["path"]}
                            return {"content": path.read_text()}
                        path.write_text(values["content"])
                        return {"written": values["path"], "ok": True}
                    if name == "list_files":
                        return {"files": sorted(p.name for p in root.iterdir())}
                    if name == "lookup_order":
                        return ({"tracking_id": tracking} if values["order_id"] == "ORDER-42"
                                else {"error": "unknown_order"})
                    if name == "get_shipping":
                        return ({"status": status} if values["tracking_id"] == tracking
                                else {"error": "unknown_tracking_id"})

                try:
                    for turn in range(7):
                        response, seconds = request(messages, TOOLS, stream=case == "stream_read")
                        elapsed += seconds
                        tokens += response.get("usage", {}).get("completion_tokens", 0)
                        log.append(response)
                        choice = response["choices"][0]
                        msg = choice["message"]
                        if choice.get("finish_reason") == "length":
                            raise ValueError("generation truncated")
                        tc = msg.get("tool_calls") or []
                        if not tc:
                            final = msg.get("content") or ""
                            if "<tool_call>" in final:
                                valid = False
                            completed = True
                            break
                        if choice.get("finish_reason") != "tool_calls":
                            valid = False
                        messages.append({"role": "assistant", "content": msg.get("content"), "tool_calls": tc})
                        for call in tc:
                            try:
                                value = execute(call)
                            except Exception as error:
                                valid = False
                                errors.append(str(error))
                                value = {"error": str(error)}
                            messages.append({"role": "tool", "tool_call_id": call.get("id", ""),
                                             "content": json.dumps(value, ensure_ascii=False)})
                    names = [c["name"] for c in calls]
                    passed = valid and completed
                    if case == "no_tool":
                        passed &= not calls and final.strip() == "HOLA"
                    elif case in ("read", "stream_read"):
                        passed &= "read_file" in names and final.strip() == secret
                    elif case == "write":
                        path = root / "result.txt"
                        passed &= "write_file" in names and path.is_file() and path.read_text() == secret
                    elif case == "chain":
                        passed &= names == ["lookup_order", "get_shipping"] and final.strip() == status
                    elif case == "recover":
                        passed &= names[:3] == ["read_file", "list_files", "read_file"] and final.strip() == secret
                except Exception as error:
                    errors.append(str(error))
                    passed = False
                row = {"case": case, "repetition": rep + 1, "passed": bool(passed),
                       "protocol_valid": valid, "completed": completed,
                       "seconds": round(elapsed, 3), "completion_tokens": tokens,
                       "calls": calls, "final": final, "errors": errors}
                results.append(row)
                (out / f"{rep+1}-{case}.json").write_text(json.dumps({"result": row, "responses": log}, indent=2, ensure_ascii=False))
                (out / "summary.json").write_text(json.dumps(results, indent=2, ensure_ascii=False))
                print(json.dumps(row, ensure_ascii=False), flush=True)
    print(f"PASS {sum(r['passed'] for r in results)}/{len(results)}", flush=True)


if __name__ == "__main__":
    main()
