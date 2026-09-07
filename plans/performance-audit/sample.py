"""GDB wall-stack sampling, not hardware CPU counters. Run with gdb -x."""
import collections
import json
import os
import signal
import threading
import time

import gdb

gdb.execute("set pagination off")
gdb.execute("set debuginfod enabled off")
gdb.execute("set print thread-events off")
gdb.execute("handle SIGINT stop nopass")
gdb.execute("set logging file /dev/null")
gdb.execute("set logging redirect on")
gdb.execute("set logging enabled on")
gdb.execute("start", to_string=True)
pid = gdb.selected_inferior().pid
tick = threading.Event()


def interrupt():
    while tick.wait():
        tick.clear()
        time.sleep(0.007)
        try:
            os.kill(pid, signal.SIGINT)
        except ProcessLookupError:
            return


threading.Thread(target=interrupt, daemon=True).start()
inclusive = collections.Counter()
leaves = collections.Counter()
count = 0
while gdb.selected_inferior().threads():
    tick.set()
    gdb.execute("continue", to_string=True)
    if not gdb.selected_inferior().threads():
        break
    frame = gdb.newest_frame()
    stack = []
    while frame is not None:
        name = frame.name() or "unknown"
        sal = frame.find_sal()
        if sal.symtab:
            name += " @ " + sal.symtab.filename + ":" + str(sal.line)
        stack.append(name)
        frame = frame.older()
    inclusive.update(set(stack))
    leaves.update(stack[:1])
    count += 1
gdb.execute("set logging enabled off")
print(json.dumps({"samples": count, "inclusive": inclusive.most_common(40), "leaves": leaves.most_common(30)}))
