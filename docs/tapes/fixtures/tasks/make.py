"""Write the tasks tape's store (docs/tapes/tasks.tape) into a directory.

The example's own sample uses random uuids and dues at midnight, so its
report changes with the time of day. Here every time is the moment the
tape runs plus or minus a whole number of hours, and the uuids are
fixed, so the report's relative ages and dues, and the urgencies, are
the same on every run. Absolute dates still change; the tape masks them.

    python3 make.py DIR
"""

import json
import sys
import time
from pathlib import Path

HOUR = 3600
DAY = 24 * HOUR
now = int(time.time())


def stamp(t):
    return time.strftime("%Y%m%dT%H%M%SZ", time.gmtime(t))


def task(n, description, entry, **fields):
    data = {
        "uuid": f"5e1f0c2a-0000-4000-8000-{n:012d}",
        "description": description,
        "status": fields.pop("status", "pending"),
        "entry": stamp(now - entry),
        "modified": stamp(now - entry),
    }
    for name in ("start", "end", "due"):
        if name in fields:
            data[name] = stamp(now + fields.pop(name))
    data.update(fields)
    return data


tasks = [
    task(1, "Pay the rent", 3 * DAY, due=DAY + 12 * HOUR,
         project="home.bills", priority="H", tags=["bills"]),
    task(2, "Write the quarterly report", 10 * DAY, start=-2 * HOUR,
         due=5 * DAY + 12 * HOUR, project="work", tags=["next"]),
    task(3, "Review pull requests", 5 * DAY, due=-DAY - 12 * HOUR,
         project="work", tags=["review"],
         annotations=[{"entry": stamp(now - DAY), "description": "Two are from Ada"}]),
    task(4, "Book flights to Lisbon", DAY + 12 * HOUR, due=20 * DAY + 12 * HOUR,
         project="travel"),
    task(5, "Fix the garden fence", 40 * DAY, project="home.garden", priority="L"),
    task(6, "Call the plumber", 2 * HOUR + 30 * 60, priority="M", tags=["phone"]),
    task(7, "Renew the passport", 100 * DAY, due=60 * DAY + 12 * HOUR,
         project="travel"),
    task(8, "Buy groceries", 2 * DAY, status="completed", end=-DAY),
]

out = Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)
(out / "tasks.jsonl").write_text("\n".join(json.dumps(t) for t in tasks) + "\n")
(out / "undo.jsonl").write_text("")
(out / "taskrc").write_text("context.work=project:work\ncontext.home=project:home\n")
