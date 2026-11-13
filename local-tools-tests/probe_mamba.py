"""Generate specialist/reviewer probes; never execute generated code here."""
import json
import time
import urllib.request
from pathlib import Path


ROOT = Path.home() / "colibri-models/tool-test-results/codestral-mamba-7b"
MODEL = str(Path.home() / "colibri-models/mlx-codestral-mamba-7b")
PROBES = {
    "merge_intervals": "Return only complete Python code, no explanation. Implement merge_intervals(intervals): accept a list of (start,end) pairs, return sorted merged pairs, merging overlaps AND touching intervals. Do not mutate input. Empty input returns []. Reject any start>end with ValueError. Support negative endpoints. Return a list of tuples. Use standard library only.",
    "lru_cache": "Return only complete Python code, no explanation. Implement class LRUCache(capacity), capacity must be positive or raise ValueError. get(key) returns value or -1 when missing, marking a hit most recently used. put(key,value) inserts or updates, marking most recently used, evicting least recently used when over capacity. Updates must not evict another key. Use collections.OrderedDict. No tests needed.",
    "task_store": "Return only complete Python code, no explanation. Use sqlite3. Implement TaskStore with an in-memory SQLite database: add(title) returns integer id; list_tasks() returns (id,title,done) tuples ordered by id; complete(id) marks done and raises KeyError for unknown id; delete(id) removes task and raises KeyError for unknown id. Reject blank or whitespace-only titles with ValueError. Parameterize SQL, preserve title text, commit writes, and provide close(). No tests needed.",
    "review_sql": '''Return ONLY a JSON object with keys "choice" (A or B) and "reason". Choose the safer SQL implementation for an untrusted username.
A: conn.execute("SELECT id FROM users WHERE name = '" + username + "'")
B: conn.execute("SELECT id FROM users WHERE name = ?", (username,))''',
    "review_lru": '''Return ONLY a JSON object with keys "choice" (A or B) and "reason". Choose correct LRU get implementation. self.data is OrderedDict. Hits must become most recently used, misses return -1.
A: return self.data.get(key, -1)
B: if key not in self.data: return -1
   self.data.move_to_end(key)
   return self.data[key]''',
    "review_range": '''Return ONLY a JSON object with keys "choice" (A or B) and "reason". Choose correct implementation of is_adult(age), where 18 is an adult.
A: return age >= 18
B: return age > 18''',
}


def main():
    ROOT.mkdir(parents=True, exist_ok=True)
    summary = []
    for name, instruction in PROBES.items():
        body = {"model": MODEL, "prompt": "<s>[INST] " + instruction + " [/INST]",
                "temperature": 0, "max_tokens": 1000 if not name.startswith("review_") else 256}
        request = urllib.request.Request("http://127.0.0.1:8081/v1/completions",
                                         data=json.dumps(body).encode(),
                                         headers={"Content-Type": "application/json"})
        start = time.monotonic()
        with urllib.request.urlopen(request, timeout=180) as response:
            result = json.load(response)
        seconds = time.monotonic() - start
        text = result["choices"][0]["text"]
        row = {"name": name, "seconds": round(seconds, 3),
               "usage": result.get("usage"), "finish_reason": result["choices"][0]["finish_reason"]}
        (ROOT / (name + ".json")).write_text(json.dumps({"request": body, "response": result, "metrics": row}, indent=2))
        (ROOT / (name + ".txt")).write_text(text)
        summary.append(row)
        (ROOT / "specialist-summary.json").write_text(json.dumps(summary, indent=2))
        print(json.dumps(row), flush=True)


if __name__ == "__main__":
    main()
