import codecs
import os
import tempfile


class _NativeIo:
    def __init__(self):
        self.wire = os.fdopen(os.dup(1), "w", encoding="utf-8", buffering=1)
        self.input = os.fdopen(os.dup(0), "r", encoding="utf-8")
        self.directory = tempfile.TemporaryDirectory(prefix="python-", dir=os.environ.get("HARNESS_EVAL_CAPTURE_DIR"))
        self.streams = []
        for fd, name in [(1, "stdout"), (2, "stderr")]:
            path = os.path.join(self.directory.name, name)
            writer = open(path, "ab", buffering=0)
            reader = open(path, "rb", buffering=0)
            os.dup2(writer.fileno(), fd)
            self.streams.append((name, writer, reader, codecs.getincrementaldecoder("utf-8")("replace")))
        with open(os.devnull, "rb") as empty:
            os.dup2(empty.fileno(), 0)

    def drain(self):
        for name, _, reader, decoder in self.streams:
            while data := reader.read(8192):
                text = decoder.decode(data)
                if text:
                    yield name, text

    def clear(self):
        for _, writer, reader, decoder in self.streams:
            writer.truncate(0)
            reader.seek(0)
            decoder.reset()
