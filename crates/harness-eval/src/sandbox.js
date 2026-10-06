(() => {
  const host = globalThis.__host, emit = globalThis.__emit;
  delete globalThis.__host;
  delete globalThis.__emit;
  const send = event => emit(JSON.stringify(event));
  const call = (operation, args) => {
    const result = JSON.parse(host(operation, JSON.stringify(args)));
    if (result.error) throw new Error(result.error);
    return result.value;
  };
  globalThis.tool = globalThis.tools = new Proxy(Object.create(null), {
    get(_, name) {
      if (typeof name !== 'string' || name === 'then') return;
      return async (parameters = {}) => call('tool', {name, parameters});
    },
  });
  globalThis.tool_schema = name => call('schema', name == null ? {} : {name});
  globalThis.print = globalThis.text = (...values) => send({type:'text', stream:'stdout', data:values.map(v => typeof v === 'string' ? v : JSON.stringify(v)).join(' ') + '\n'});
  globalThis.console = {log: print, info: print, warn: print, error: print};
  globalThis.image = value => {
    if (!value || typeof value.mimeType !== 'string' || !value.mimeType.startsWith('image/') || typeof (value.dataBase64 ?? value.data) !== 'string') throw new TypeError('image() expects mimeType and base64 data');
    send({type:'display', mimeType:value.mimeType, dataBase64:value.dataBase64 ?? value.data});
  };
  globalThis.display = value => {
    if (value?.mimeType?.startsWith('image/')) return image(value);
    if (Array.isArray(value?.images)) {
      if (value.text) print(value.text);
      value.images.forEach(image);
      return;
    }
    print(value);
  };
  globalThis.store = () => { throw new Error('isolated eval has no persistent storage'); };
  globalThis.load = () => undefined;
})();
