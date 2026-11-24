"""A stand-in model for the README's AI screenshots (scripts/shoot_readme.sh).

Speaks just enough of the OpenAI Chat Completions API for the app: GET /v1/models and
POST /v1/chat/completions (one JSON reply, no streaming). Replies are canned, quoting
sentences of scripts/readme/sample-ai.md by their [n] numbers in the request.
"""
import json
import re
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

MODELS = ["llama3.2:3b", "qwen3:32b"]

MARKS = {
    "hard": [
        ("The page holds both versions until you’re ready, and you judge them the only way that works: in context, with the real sentence around them.",
         "Two ideas joined by “and”; the colon hides the point at the end."),
        ("Version control keeps history, but history is the wrong shape: it remembers when you changed something, not what you were choosing between.",
         "Two turns in one sentence; split at the colon."),
        # The demo video's Crime and Punishment (scripts/demo).
        ("The landlady who provided him with garret, dinners, and attendance, lived on the floor below, and every time he went out he was obliged to pass her kitchen, the door of which invariably stood open.",
         "35 words, three clauses; a full stop after “below” lets it breathe."),
    ],
    "tone": [("Honestly", "Chatty next to the plain voice around it.")],
    "mistakes": [("teh", "the", "spelling"), ("dissapear", "disappear", "spelling")],
}

CUTS = [
    "Honestly, it is basically just a kind of safety net, really.",
    "quietly",
    "always",
    "A lower one tells you you’re back.",
    "A week later you can’t remember what they were, only that one of them was better.",
    "Undo takes you back, but only by throwing away everything after.",
    "The page holds both versions until you’re ready, and you judge them the only way that works: in context, with the real sentence around them.",
    "Any word, sentence or paragraph can hold several versions.",
    "Anything that doesn’t belong yet waits beside the page, one keystroke away.",
    "Software forgot this.",
    "Editors have always known this.",
]

IDEAS = {
    "suggestion": ["counterpoint", "margin note", "gentler word", "query", "second opinion"],
    "friction": ["drag", "hesitation", "doubt", "trouble", "stalling"],
    "quietly": ["softly", "one by one", "without a trace", "unnoticed", "slowly"],
    "silently": ["softly", "one by one", "without a trace", "unnoticed", "slowly"],
    "indecision": ["doubt", "reluctance", "a daze", "uncertainty", "dread"],
}
DEFAULT_IDEAS = ["choice", "option", "turn", "version", "path"]


def sentences(user):
    return {int(n): s for n, s in re.findall(r"^\[(\d+)\] (.*)$", user, re.M)}


def where(sents, quote):
    for n, s in sents.items():
        if quote in s:
            return n
    return None


def reply_for(system, user):
    sents = sentences(user)
    if "choose words" in system:
        m = re.search(r"The marked [^:\n]+: (.*)", user)
        word = (m.group(1) if m else "").strip().lower()
        n = int(re.search(r"exactly (\d+) suggestions", system).group(1))
        return "\n".join((IDEAS.get(word) or DEFAULT_IDEAS)[:n])
    if "tightening" in system:
        cuts = [{"sentence": where(sents, q), "quote": q} for q in CUTS if where(sents, q)]
        return json.dumps({"cuts": cuts})
    task = system.lower()
    kind = "mistakes" if "find mistakes" in task else "hard" if "convoluted" in task else "tone"
    marks = []
    for m in MARKS[kind]:
        n = where(sents, m[0])
        if n is None:
            continue
        if kind == "mistakes":
            marks.append({"sentence": n, "quote": m[0], "fix": m[1], "note": m[2]})
        else:
            marks.append({"sentence": n, "quote": m[0], "note": m[1]})
    return json.dumps({"marks": marks})


class Handler(BaseHTTPRequestHandler):
    def send_json(self, obj):
        body = json.dumps(obj).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path.rstrip("/").endswith("/models"):
            self.send_json({"data": [{"id": m} for m in MODELS]})
        else:
            self.send_error(404)

    def do_POST(self):
        req = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))))
        msgs = req.get("messages", [])
        system = next((m["content"] for m in msgs if m["role"] == "system"), "")
        user = next((m["content"] for m in msgs if m["role"] == "user"), "")
        text = reply_for(system, user)
        print(f"--- {req.get('model')}\n{text}", flush=True)
        self.send_json({"choices": [{"index": 0, "message": {"role": "assistant", "content": text}, "finish_reason": "stop"}],
                        "usage": {"prompt_tokens": 0, "completion_tokens": 0}})


if __name__ == "__main__":
    HTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
