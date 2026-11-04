"""Generate realistic demo usage logs for Tokeye screenshots (no real data involved).

    python scripts/demo_data.py <out_dir>            # 30 days of history for every tool
    python scripts/demo_data.py <out_dir> --live 12  # append 12 live Claude Code requests, 1.5 s apart

Point Tokeye at the result with these settings: claude_dir=<out>/claude and
tool_paths = {codex: <out>/codex/sessions, opencode: <out>/opencode, gemini: <out>/gemini/tmp, cursor: <out>/cursor}.
"""
import json
import os
import random
import sys
import time
import uuid
from datetime import datetime, timedelta, timezone

random.seed(7)
OUT = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "demo-data")
NOW = datetime.now(timezone.utc)
PROJECTS = ["acme-web", "mobile-app", "data-pipeline", "api-gateway", "docs-site"]
CLAUDE_MODELS = [("claude-opus-5-5", 0.78), ("claude-sonnet-5-5", 0.17), ("claude-haiku-5-5", 0.05)]


def iso(t):
    return t.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.") + f"{t.microsecond // 1000:03d}Z"


def pick(weighted):
    r, acc = random.random(), 0.0
    for v, w in weighted:
        acc += w
        if r <= acc:
            return v
    return weighted[-1][0]


def claude_line(t, session, cwd, vertex, model=None):
    model = model or pick(CLAUDE_MODELS)
    prefix = "msg_vrtx_" if vertex else "msg_"
    cache_write = random.choice([0, 0, 0, random.randint(8_000, 60_000)])
    return json.dumps({
        "type": "assistant",
        "timestamp": iso(t),
        "sessionId": session,
        "requestId": ("req_vrtx_" if vertex else "req_") + uuid.uuid4().hex[:20],
        "cwd": cwd,
        "message": {
            "id": prefix + uuid.uuid4().hex[:24],
            "model": model,
            "role": "assistant",
            "usage": {
                "input_tokens": random.randint(4, 900),
                "cache_creation_input_tokens": cache_write,
                "cache_creation": {"ephemeral_5m_input_tokens": 0, "ephemeral_1h_input_tokens": cache_write},
                "cache_read_input_tokens": random.randint(60_000, 520_000),
                "output_tokens": random.randint(150, 4_200),
            },
        },
    })


def write_claude():
    for day in range(30, -1, -1):
        date = (NOW - timedelta(days=day)).date()
        weekend = date.weekday() >= 5
        sessions = random.randint(0, 1) if weekend else random.randint(2, 4)
        if day == 0:
            sessions = 3
        for s in range(sessions):
            project = PROJECTS[(day + s) % len(PROJECTS)] if day else ["acme-web", "mobile-app", "api-gateway"][s]
            session = str(uuid.uuid4())
            cwd = f"C:\\Users\\dev\\projects\\{project}"
            vertex = (day + s) % 3 != 0
            start = datetime(date.year, date.month, date.day, 8 + s * 3, random.randint(0, 50), tzinfo=timezone.utc).astimezone()
            if day == 0:
                start = NOW - timedelta(hours=6 - s * 2)
            n = random.randint(40, 170)
            folder = os.path.join(OUT, "claude", "projects", project)
            os.makedirs(folder, exist_ok=True)
            with open(os.path.join(folder, f"{session}.jsonl"), "w", encoding="utf-8", newline="\n") as f:
                t = start
                for _ in range(n):
                    t += timedelta(seconds=random.randint(15, 90))
                    if t > NOW:
                        break
                    f.write(claude_line(t, session, cwd, vertex) + "\n")


def write_codex():
    for day in range(25, -1, -3):
        date = NOW - timedelta(days=day, hours=random.randint(1, 5))
        sid = str(uuid.uuid4())
        folder = os.path.join(OUT, "codex", "sessions", date.strftime("%Y"), date.strftime("%m"), date.strftime("%d"))
        os.makedirs(folder, exist_ok=True)
        lines = [
            {"timestamp": iso(date), "type": "session_meta", "payload": {"id": sid, "cwd": "C:\\Users\\dev\\projects\\data-pipeline"}},
            {"timestamp": iso(date), "type": "turn_context", "payload": {"model": "gpt-5-codex", "cwd": "C:\\Users\\dev\\projects\\data-pipeline"}},
        ]
        tot = [0, 0, 0]
        t = date
        for _ in range(random.randint(20, 60)):
            t += timedelta(seconds=random.randint(20, 80))
            if t > NOW:
                break
            inp = random.randint(20_000, 120_000)
            cached = int(inp * random.uniform(0.6, 0.9))
            out = random.randint(300, 3_000)
            tot = [tot[0] + inp, tot[1] + cached, tot[2] + out]
            usage = {"input_tokens": tot[0], "cached_input_tokens": tot[1], "output_tokens": tot[2], "reasoning_output_tokens": 0, "total_tokens": tot[0] + tot[2]}
            lines.append({"timestamp": iso(t), "type": "event_msg", "payload": {"type": "token_count", "info": {"total_token_usage": usage}}})
        with open(os.path.join(folder, f"rollout-{date.strftime('%Y-%m-%dT%H-%M-%S')}-{sid}.jsonl"), "w", encoding="utf-8", newline="\n") as f:
            f.writelines(json.dumps(l) + "\n" for l in lines)


def write_gemini():
    folder = os.path.join(OUT, "gemini", "tmp", "3f9a1c2b7d", "chats")
    os.makedirs(folder, exist_ok=True)
    for day in range(20, -1, -4):
        t = NOW - timedelta(days=day, hours=3)
        msgs = []
        for i in range(random.randint(10, 30)):
            t += timedelta(seconds=random.randint(30, 120))
            if t > NOW:
                break
            inp = random.randint(8_000, 90_000)
            msgs.append({"id": f"m{i}", "timestamp": iso(t), "type": "gemini", "model": "gemini-2.5-pro",
                         "tokens": {"input": inp, "output": random.randint(200, 2_500), "cached": int(inp * 0.5), "thoughts": random.randint(0, 800), "tool": 0}})
        sid = str(uuid.uuid4())
        with open(os.path.join(folder, f"session-{t.strftime('%Y-%m-%dT%H-%M')}-{sid[:8]}.json"), "w", encoding="utf-8") as f:
            json.dump({"sessionId": sid, "projectHash": "3f9a1c2b7d", "messages": msgs}, f)


def write_opencode():
    for day in range(28, -1, -2):
        sid = "ses_" + uuid.uuid4().hex[:24]
        folder = os.path.join(OUT, "opencode", "storage", "message", sid)
        os.makedirs(folder, exist_ok=True)
        t = NOW - timedelta(days=day, hours=random.randint(2, 7))
        for i in range(random.randint(15, 45)):
            t += timedelta(seconds=random.randint(20, 100))
            if t > NOW:
                break
            inp, out, cr = random.randint(500, 4_000), random.randint(200, 2_500), random.randint(20_000, 180_000)
            ms = int(t.timestamp() * 1000)
            msg = {"id": f"msg_{uuid.uuid4().hex[:20]}", "sessionID": sid, "role": "assistant", "modelID": "kimi-k2", "providerID": "openrouter",
                   "path": {"cwd": "C:\\Users\\dev\\projects\\docs-site"}, "time": {"created": ms, "completed": ms + 4000},
                   "tokens": {"input": inp, "output": out, "reasoning": 0, "cache": {"read": cr, "write": 0}},
                   "cost": round(inp * 0.6e-6 + out * 2.5e-6 + cr * 0.15e-6, 6)}
            with open(os.path.join(folder, msg["id"] + ".json"), "w", encoding="utf-8") as f:
                json.dump(msg, f)


def write_cursor():
    folder = os.path.join(OUT, "cursor")
    os.makedirs(folder, exist_ok=True)
    with open(os.path.join(folder, "usage-events-demo.csv"), "w", encoding="utf-8", newline="\n") as f:
        f.write("Date,Kind,Model,Max Mode,Input (w/ Cache Write),Input (w/o Cache Write),Cache Read,Output,Total Tokens,Cost\n")
        for day in range(29, -1, -1):
            for _ in range(random.randint(2, 8)):
                t = NOW - timedelta(days=day, minutes=random.randint(30, 600))
                wo = random.randint(2_000, 30_000)
                w = wo + random.randint(0, 8_000)
                cr, out = random.randint(10_000, 200_000), random.randint(300, 3_000)
                cost = round(w * 3e-6 + cr * 0.3e-6 + out * 15e-6, 4)
                f.write(f"{iso(t)},Included,claude-4.5-sonnet,No,{w},{wo},{cr},{out},{w + cr + out},${cost}\n")


def live(n):
    """Append fresh requests to the newest Claude session so the companion sees activity."""
    folder = os.path.join(OUT, "claude", "projects", "acme-web")
    newest = max((os.path.join(folder, p) for p in os.listdir(folder)), key=os.path.getmtime)
    session = os.path.splitext(os.path.basename(newest))[0]
    for _ in range(n):
        with open(newest, "a", encoding="utf-8", newline="\n") as f:
            f.write(claude_line(datetime.now(timezone.utc), session, "C:\\Users\\dev\\projects\\acme-web", True, "claude-opus-5-5") + "\n")
        time.sleep(1.5)


if __name__ == "__main__":
    if "--live" in sys.argv:
        live(int(sys.argv[sys.argv.index("--live") + 1]))
    else:
        write_claude()
        write_codex()
        write_gemini()
        write_opencode()
        write_cursor()
        print("demo data written to", OUT)
