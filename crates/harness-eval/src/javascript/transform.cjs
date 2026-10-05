const { parse } = require('./acorn.cjs');

const program = source => parse(source, {
  ecmaVersion: 'latest', sourceType: 'module', allowReturnOutsideFunction: true,
});
const apply = (source, edits) => edits.sort((a, b) => b[0] - a[0]).reduce(
  (text, [start, end, replacement]) => text.slice(0, start) + replacement + text.slice(end), source,
);

function importDeclaration(node, source) {
  const attributes = node.attributes?.length
    ? `, {with: {${node.attributes.map(a => `${source.slice(a.key.start, a.key.end)}: ${source.slice(a.value.start, a.value.end)}`).join(',')}}}` : '';
  const load = `await import(${JSON.stringify(node.source.value)}${attributes})`;
  const namespace = node.specifiers.find(s => s.type === 'ImportNamespaceSpecifier');
  const named = node.specifiers.filter(s => s !== namespace).map(s =>
    `${JSON.stringify(s.type === 'ImportDefaultSpecifier' ? 'default' : s.imported.name ?? s.imported.value)}: ${s.local.name}`,
  );
  if (namespace) return `const ${namespace.local.name} = ${load};${named.length ? `const {${named}} = ${namespace.local.name};` : ''}`;
  return named.length ? `const {${named}} = ${load};` : `${load};`;
}

function pattern(node, source, protectedNames) {
  switch (node.type) {
    case 'Identifier':
      if (protectedNames.has(node.name)) throw new SyntaxError(`eval cell declares top-level \`${node.name}\`, which would replace an existing kernel global; rename the binding or assign globalThis.${node.name} explicitly`);
      return `globalThis[${JSON.stringify(node.name)}]`;
    case 'ArrayPattern':
      return `[${node.elements.map(n => n ? pattern(n, source, protectedNames) : '').join(',')}${node.elements.at(-1) === null ? ',' : ''}]`;
    case 'ObjectPattern':
      return `{${node.properties.map(p => p.type === 'RestElement' ? pattern(p, source, protectedNames)
        : `${p.computed ? '[' : ''}${source.slice(p.key.start, p.key.end)}${p.computed ? ']' : ''}: ${pattern(p.value, source, protectedNames)}`).join(',')}}`;
    case 'AssignmentPattern':
      return `${pattern(node.left, source, protectedNames)} = ${source.slice(node.right.start, node.right.end)}`;
    case 'RestElement':
      return `...${pattern(node.argument, source, protectedNames)}`;
    default: throw new SyntaxError('unsupported declaration binding');
  }
}

function hasReturn(node) {
  if (!node || typeof node !== 'object') return false;
  if (node.type === 'ReturnStatement') return true;
  if (/^(Function|ArrowFunction|Class)/.test(node.type)) return false;
  return Object.values(node).some(value => Array.isArray(value) ? value.some(hasReturn) : hasReturn(value));
}

exports.cell = (source, protectedNames) => {
  source = apply(source, program(source).body.filter(n => n.type === 'ImportDeclaration')
    .map(n => [n.start, n.end, importDeclaration(n, source)]));
  const nodes = program(source).body, returns = nodes.some(hasReturn), edits = [];
  for (const node of nodes) {
    const capture = !returns && node === nodes.at(-1);
    if (node.type === 'VariableDeclaration') {
      const expressions = node.declarations.map(d => `(${pattern(d.id, source, protectedNames)} = ${d.init ? source.slice(d.init.start, d.init.end) : 'undefined'})`);
      edits.push([node.start, node.end, expressions.map((text, i) => `${capture && i === expressions.length - 1 ? 'return ' : ''}${text};`).join('\n')]);
    } else if (node.type === 'ExpressionStatement' && capture) {
      edits.push([node.start, node.end, `return (${source.slice(node.expression.start, node.expression.end)});`]);
    }
  }
  return `(async () => {\n${apply(source, edits)}\n})()`;
};

exports.parseFunction = source => {
  const nodes = program(source).body, fn = nodes[0];
  if (nodes.length !== 1 || fn.type !== 'FunctionDeclaration' || !fn.id) throw new Error('tool() requires a named function');
  if (fn.generator) throw new Error('generator tools are unsupported');
  if (fn.params.some(p => p.type !== 'Identifier')) throw new Error('tool parameters must be simple identifiers without defaults or rest');
  const params = fn.params.map(p => p.name);
  if (new Set(params).size !== params.length) throw new Error('tool parameters must have distinct names');
  return {name: fn.id.name, params};
};
