"""Count calls with a GDB breakpoint. Debugger timings must not be used."""
import json
import os

import gdb

gdb.execute("set pagination off")
gdb.execute("set debuginfod enabled off")
gdb.execute("set print thread-events off")
gdb.execute("start", to_string=True)


class Count(gdb.Breakpoint):
    hits = 0

    def stop(self):
        self.hits += 1
        return False


counter = Count(os.environ["AUDIT_BREAK"])
gdb.execute("continue", to_string=True)
print(json.dumps({"breakpoint": os.environ["AUDIT_BREAK"], "hits": counter.hits}))
