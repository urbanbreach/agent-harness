const fs = require('node:fs');
const path = require('node:path');
const { StringDecoder } = require('node:string_decoder');

module.exports = root => {
  const bytes = Buffer.alloc(8192);
  const streams = ['stdout', 'stderr'].map(name => ({
    name, fd: fs.openSync(path.join(root, name), 'r+'), position: 0,
    decoder: new StringDecoder('utf8'),
  }));
  return {
    clear() {
      for (const stream of streams) {
        fs.ftruncateSync(stream.fd, 0);
        stream.position = 0;
        stream.decoder = new StringDecoder('utf8');
      }
    },
    drain(emit) {
      for (const stream of streams) {
        for (;;) {
          const count = fs.readSync(stream.fd, bytes, 0, bytes.length, stream.position);
          if (!count) break;
          stream.position += count;
          const data = stream.decoder.write(bytes.subarray(0, count));
          if (data) emit(stream.name, data);
        }
      }
    },
  };
};
