import { useEffect, useRef, useState } from 'react';
import type { Run } from '../native';
interface Chunk {
  start: number;
  end: number;
  time: number;
  duration: number;
}

/** Append-only ink animation: geometry and non-ink decorations stay unchanged. */
export function useStreamingVeil(
  text: string,
  runs: Run[],
  ink: string,
  streaming: boolean,
  reducedMotion: boolean,
): Run[] {
  const state = useRef({ text, streaming, interval: 160, last: 0, chunks: [] as Chunk[] });
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const previous = state.current;
    const time = Date.now();
    if (!streaming || reducedMotion || !previous.streaming || !text.startsWith(previous.text))
      previous.chunks = [];
    else if (text.length > previous.text.length) {
      if (previous.last)
        previous.interval = previous.interval * 0.7 + Math.min(time - previous.last, 1000) * 0.3;
      previous.chunks.push({
        start: previous.text.length,
        end: text.length,
        time,
        duration: Math.min(400, Math.max(120, previous.interval * 3)),
      });
      previous.last = time;
    }
    previous.text = text;
    previous.streaming = streaming;
    if (!previous.chunks.length) return;
    setNow(time);
    const timer = setInterval(() => {
      const tick = Date.now();
      const speed = 1 + Math.max(0, previous.chunks.length - 2) * 0.3;
      previous.chunks = previous.chunks.filter(
        (chunk) => (tick - chunk.time) * speed < chunk.duration,
      );
      setNow(tick);
      if (!previous.chunks.length) clearInterval(timer);
    }, 16);
    return () => clearInterval(timer);
  }, [text, streaming, reducedMotion]);
  if (!streaming || reducedMotion) return runs;
  const output = [...runs];
  const speed = 1 + Math.max(0, state.current.chunks.length - 2) * 0.3;
  for (const chunk of state.current.chunks) {
    const opacity =
      1 - (1 - Math.min(1, Math.max(0, ((now - chunk.time) * speed) / chunk.duration))) ** 1.6;
    const boundaries = new Set([chunk.start, chunk.end]);
    for (const run of runs)
      if (run.start < chunk.end && run.end > chunk.start) {
        boundaries.add(Math.max(run.start, chunk.start));
        boundaries.add(Math.min(run.end, chunk.end));
      }
    const sorted = [...boundaries].sort((a, b) => a - b);
    for (let i = 0; i < sorted.length - 1; i++) {
      const start = sorted[i]!;
      const end = sorted[i + 1]!;
      const style = Object.assign(
        {},
        ...runs.filter((run) => run.start <= start && run.end >= end),
      ) as Run;
      const faded = (color: string) =>
        /^#[\da-f]{6}$/i.test(color)
          ? `${color}${Math.round(opacity * 255)
              .toString(16)
              .padStart(2, '0')}`
          : color;
      output.push({
        start,
        end,
        color: faded(style.color ?? ink),
        underline: style.underline && {
          ...style.underline,
          color: faded(style.underline.color ?? ink),
        },
        strikethrough: style.strikethrough && {
          ...style.strikethrough,
          color: faded(style.strikethrough.color ?? ink),
        },
      });
    }
  }
  return output;
}
