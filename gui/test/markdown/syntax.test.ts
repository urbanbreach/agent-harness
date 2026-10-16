import { expect, test } from 'bun:test';
import { highlightCode } from '../../src/syntax';

test('code colors preserve UTF-16 text, aliases, embedded fences and unfinished lexical state', () => {
  const cases: { language: string; text: string; samples: [string, string, string][] }[] = [
    {
      language: ' RS linenums',
      text: '#[derive(Debug)]\n// 😀 <&>\nfn main() { let n = 42; println!("ready"); }\n/* open\nstill <&> 😀',
      samples: [
        ['#[', '#858585', '#7D7D7D'],
        ['// 😀 <&>', '#A4A4A4', '#575757'],
        ['fn', '#9A4B92', '#C98BC0'],
        ['main', '#2F6690', '#8FB8D9'],
        ['42', '#9A6019', '#D9A05B'],
        ['println!', '#666666', '#A3A3A3'],
        ['still <&> 😀', '#A4A4A4', '#575757'],
      ],
    },
    {
      language: 'NoDe extra-info',
      text: 'function release() { console.log("ready"); }\nconst face = "😀<&>";\nconst tail = `open\nstill 😀<&>',
      samples: [
        ['const', '#9A4B92', '#C98BC0'],
        ['release', '#2F6690', '#8FB8D9'],
        ['log', '#666666', '#A3A3A3'],
        ['😀<&>', '#3F7A36', '#94C08A'],
        ['still 😀<&>', '#3F7A36', '#94C08A'],
      ],
    },
    {
      language: 'PYTHON3',
      text: 'value = """open\n😀<&>',
      samples: [['😀<&>', '#3F7A36', '#94C08A']],
    },
    {
      language: 'C++ title',
      text: 'int answer = 7;\n/* unfinished\n😀<&>',
      samples: [
        ['7', '#9A6019', '#D9A05B'],
        ['😀<&>', '#A4A4A4', '#575757'],
      ],
    },
    {
      language: 'MDOWN',
      text: '~~~TSX title\nconst face = "😀<&>";\n~~~',
      samples: [
        ['const', '#9A4B92', '#C98BC0'],
        ['😀<&>', '#3F7A36', '#94C08A'],
      ],
    },
    {
      language: 'PATCH',
      text: '+added 😀<&>\n-removed',
      samples: [
        ['+added 😀<&>', '#2F8F52', '#62C987'],
        ['-removed', '#C64A42', '#E2726A'],
      ],
    },
  ];

  for (const { text, language, samples } of cases) {
    for (const dark of [false, true]) {
      const ranges = highlightCode(text, language, dark);
      let end = 0;
      for (const range of ranges) {
        expect(range.start).toBe(end);
        expect(range.end).toBeGreaterThan(range.start);
        end = range.end;
      }
      expect(end).toBe(text.length);
      expect(ranges.map((range) => text.slice(range.start, range.end)).join('')).toBe(text);
      for (const [sample, lightColor, darkColor] of samples) {
        const start = text.indexOf(sample);
        expect(start).toBeGreaterThanOrEqual(0);
        for (let offset = start; offset < start + sample.length; offset++) {
          expect(ranges.find((range) => range.start <= offset && offset < range.end)?.color).toBe(
            dark ? darkColor : lightColor,
          );
        }
      }
    }
  }

  for (const language of [undefined, '', 'unrecognized js']) {
    expect(highlightCode('const 😀 = "<&>"', language, false)).toEqual([
      { start: 0, end: 16, color: '#666666' },
    ]);
    expect(highlightCode('const 😀 = "<&>"', language, true)).toEqual([
      { start: 0, end: 16, color: '#A3A3A3' },
    ]);
  }
  expect(highlightCode('', 'rust', true)).toEqual([]);
});
