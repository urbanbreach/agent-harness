import ctypes
import gc
import itertools
import sys
import time
from pathlib import Path


class _PythonMemory:
    def __init__(self, options, namespace):
        self.limits = {key: options.get(key, 0) * 1024 * 1024 for key in
                       ("gc_watermark_mb", "notice_mb", "ceiling_mb")}
        self.namespace, self.baseline = namespace, set(namespace)
        self.live = self.noticed = self.cost = 0
        self.collected = float("-inf")
        try:
            self.trim = ctypes.CDLL(None).malloc_trim
            self.trim.argtypes, self.trim.restype = [ctypes.c_size_t], ctypes.c_int
        except (OSError, AttributeError):
            self.trim = None

    def footprint(self):
        try:
            for line in Path("/proc/self/status").read_text().splitlines():
                if line.startswith("RssAnon:"):
                    return int(line.split()[1]) * 1024, False
        except OSError:
            pass
        try:
            import resource
            peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
            return peak * (1 if sys.platform == "darwin" else 1024), True
        except (ImportError, OSError):
            return None

    def report(self):
        reading = self.footprint()
        if reading is None:
            return None
        size, approximate = reading
        watermark, notice, ceiling = (self.limits[key] for key in
                                     ("gc_watermark_mb", "notice_mb", "ceiling_mb"))
        collect = (ceiling and size >= ceiling) or (
            watermark and size >= watermark and size > self.live + max(64 * 1024 * 1024, self.live / 4)
        ) or (notice and size >= notice and (self.live < notice or not self.noticed or size >= self.noticed * 1.25)) or (
            watermark and self.live >= watermark and time.monotonic() - self.collected >= self.cost * 20)
        if collect:
            start = time.monotonic()
            gc.collect()
            if self.trim:
                self.trim(0)
            self.collected = time.monotonic()
            self.cost = self.collected - start
            size, approximate = self.footprint() or reading
        self.live = size
        if size < notice / 2:
            self.noticed = 0
        report = {"liveBytes": size, "measure": "footprint"}
        if approximate:
            report["approximate"] = True
        if collect:
            report["gcRan"] = True
            if (notice and size >= notice) or (ceiling and size >= ceiling):
                self.noticed = size
                globals_ = [{"name": name, "bytes": self.size(value, set()), "approximate": True}
                            for name, value in self.namespace.items() if name not in self.baseline]
                report["globals"] = sorted(globals_, key=lambda entry: -entry["bytes"])[:5]
        return report

    def size(self, value, seen, depth=0):
        if id(value) in seen or len(seen) >= 50000 or depth > 5:
            return 0
        seen.add(id(value))
        size = sys.getsizeof(value, 0)
        if type(value).__module__.split(".")[0] == "numpy":
            return max(size, int(value.nbytes))
        if isinstance(value, dict):
            items = itertools.chain.from_iterable(itertools.islice(value.items(), 1000))
            return size + sum(self.size(item, seen, depth + 1) for item in items)
        if isinstance(value, (list, tuple, set, frozenset)):
            return size + sum(self.size(item, seen, depth + 1) for item in itertools.islice(value, 1000))
        return size
