export interface FormulaImage {
  url: string;
  width: number;
  height: number;
  baseline: number;
}
const cache = new Map<string, Promise<FormulaImage | null>>();
let engine:
  | Promise<
      (
        source: string,
        display: boolean,
        size: number,
        ink: string,
        scale: number,
      ) => FormulaImage | null
    >
  | undefined;

async function loadEngine() {
  const [{ mathjax }, { TeX }, { SVG }, { liteAdaptor }, { RegisterHTMLHandler }, { AllPackages }] =
    await Promise.all([
      import('mathjax-full/js/mathjax.js'),
      import('mathjax-full/js/input/tex.js'),
      import('mathjax-full/js/output/svg.js'),
      import('mathjax-full/js/adaptors/liteAdaptor.js'),
      import('mathjax-full/js/handlers/html.js'),
      import('mathjax-full/js/input/tex/AllPackages.js'),
    ]);
  const adaptor = liteAdaptor();
  RegisterHTMLHandler(adaptor);
  const input = new TeX({
    packages: AllPackages.filter(
      (name) => !['autoload', 'require', 'html', 'noerrors', 'noundefined'].includes(name),
    ),
    maxBuffer: 8192,
    maxMacros: 1000,
  });
  const document = mathjax.document('', {
    InputJax: input,
    OutputJax: new SVG({ fontCache: 'none' }),
  });
  return (
    source: string,
    display: boolean,
    size: number,
    ink: string,
    scale: number,
  ): FormulaImage | null => {
    try {
      const node = document.convert(source, {
        display,
        em: size,
        ex: size / 2,
        containerWidth: 100000,
      });
      let svg = adaptor.innerHTML(node);
      if (svg.includes('data-mjx-error') || svg.includes('data-mml-node="merror"')) return null;
      const viewBox = /viewBox="([\d.eE+\- ]+)"/.exec(svg)?.[1]?.split(/\s+/).map(Number);
      if (!viewBox || viewBox.length !== 4 || viewBox.some((value) => !Number.isFinite(value)))
        return null;
      const width = (viewBox[2]! * size) / 1000 + 2;
      const height = (viewBox[3]! * size) / 1000 + 2;
      if (
        width <= 2 ||
        height <= 2 ||
        width * scale > 4096 ||
        height * scale > 4096 ||
        width * height * scale * scale > 2_000_000 ||
        (svg.match(/<(?:path|use|rect|line|polygon|text|g)\b/g)?.length ?? 0) > 8192
      )
        return null;
      const padding = 1000 / size;
      svg = svg
        .replace(
          /viewBox="[^"]*"/,
          `viewBox="${viewBox[0]! - padding} ${viewBox[1]! - padding} ${viewBox[2]! + 2 * padding} ${viewBox[3]! + 2 * padding}"`,
        )
        .replace(/width="[^"]*"/, `width="${width}px"`)
        .replace(/height="[^"]*"/, `height="${height}px"`)
        .replace(/currentColor/g, ink);
      return {
        url: `data:image/svg+xml;base64,${Buffer.from(svg).toString('base64')}`,
        width,
        height,
        baseline: (-viewBox[1]! * size) / 1000 + 1,
      };
    } catch {
      return null;
    }
  };
}

/** Failures are cached too; TeX never reaches the native image loader as markup. */
export function renderFormula(
  source: string,
  display: boolean,
  fontSize: number,
  ink: string,
  monitorScale = 1,
): Promise<FormulaImage | null> {
  const size = Math.min(96, Math.max(6, fontSize));
  const scale = Math.min(3, Math.max(1, monitorScale));
  const key = JSON.stringify([source, display, size, ink, scale]);
  const prior = cache.get(key);
  if (prior) return prior;
  if (!source.trim() || new TextEncoder().encode(source).length > 8192)
    return Promise.resolve(null);
  const result = new Promise<FormulaImage | null>((resolve) => {
    setTimeout(() => {
      void (engine ??= loadEngine())
        .then((convert) => resolve(convert(source, display, size, ink, scale)))
        .catch(() => resolve(null));
    }, 0);
  });
  cache.set(key, result);
  if (cache.size > 256) cache.delete(cache.keys().next().value!);
  return result;
}
