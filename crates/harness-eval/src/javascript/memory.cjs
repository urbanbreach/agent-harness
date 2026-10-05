const { getHeapStatistics } = require('node:v8');
const collectGarbage = globalThis.gc;
const estimate = () => {
  const stats = getHeapStatistics();
  return stats.used_heap_size + stats.external_memory;
};

module.exports = settings => {
  let lastLive = 0, noticed = 0, lastCollection = 0, collectionCost = 0, idle;
  const collect = () => {
    const start = performance.now();
    collectGarbage();
    collectGarbage();
    collectionCost = performance.now() - start;
    lastCollection = performance.now();
    lastLive = estimate();
    if (lastLive < settings.notice_mb * 1024 * 1024 / 2) noticed = 0;
    return lastLive;
  };
  return {
    begin() { clearTimeout(idle); },
    afterCell() {
      const bytes = estimate(), reached = limit => limit > 0 && bytes >= limit * 1024 * 1024;
      const gcRan = reached(settings.ceiling_mb)
        || reached(settings.gc_watermark_mb) && bytes > lastLive + Math.max(64 * 1024 * 1024, lastLive / 4)
        || reached(settings.notice_mb) && (lastLive < settings.notice_mb * 1024 * 1024 || !noticed || bytes >= noticed * 1.25);
      const liveBytes = gcRan ? collect() : bytes, report = {liveBytes, measure: 'heap'};
      if (gcRan) {
        report.gcRan = true;
        if (settings.notice_mb > 0 && liveBytes >= settings.notice_mb * 1024 * 1024) noticed = liveBytes;
        if (noticed || settings.ceiling_mb > 0 && liveBytes >= settings.ceiling_mb * 1024 * 1024) report.globals = __harness_globals();
      }
      if (settings.gc_watermark_mb > 0 && lastLive >= settings.gc_watermark_mb * 1024 * 1024) {
        idle = setTimeout(collect, Math.max(1000, collectionCost * 20 - (performance.now() - lastCollection)));
        idle.unref();
      }
      return report;
    },
  };
};
