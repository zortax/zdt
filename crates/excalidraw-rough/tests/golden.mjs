import rough from './node_modules/roughjs/bundled/rough.esm.js';

const round = (v) => Math.round(v * 1e6) / 1e6;
const dump = (d) => d.sets.map((s) => ({
  type: s.type,
  ops: s.ops.map((o) => ({ op: o.op, data: o.data.map(round) })),
}));

const g = rough.generator();
const cases = {};

const base = { seed: 1263748391, roughness: 1, bowing: 1, strokeWidth: 2, preserveVertices: false };

cases.rectangle_plain = dump(g.rectangle(0, 0, 220, 128, { ...base }));
cases.rectangle_hachure = dump(g.rectangle(0, 0, 220, 128, { ...base, fill: '#a5d8ff', fillStyle: 'hachure', fillWeight: 1, hachureGap: 8 }));
cases.rectangle_solid = dump(g.rectangle(0, 0, 220, 128, { ...base, fill: '#a5d8ff', fillStyle: 'solid', fillWeight: 1, hachureGap: 8 }));
cases.rectangle_crosshatch = dump(g.rectangle(0, 0, 220, 128, { ...base, fill: '#a5d8ff', fillStyle: 'cross-hatch', fillWeight: 1, hachureGap: 8 }));
cases.rectangle_zigzag = dump(g.rectangle(0, 0, 220, 128, { ...base, fill: '#a5d8ff', fillStyle: 'zigzag', fillWeight: 1, hachureGap: 8 }));
cases.rectangle_preserve = dump(g.rectangle(0, 0, 220, 128, { ...base, preserveVertices: true }));
cases.rectangle_rough0 = dump(g.rectangle(0, 0, 220, 128, { ...base, roughness: 0 }));
cases.ellipse_plain = dump(g.ellipse(50, 30, 100, 60, { ...base, curveFitting: 1 }));
cases.ellipse_solid = dump(g.ellipse(50, 30, 100, 60, { ...base, curveFitting: 1, fill: '#a5d8ff', fillStyle: 'solid', fillWeight: 1, hachureGap: 8 }));
cases.ellipse_hachure = dump(g.ellipse(50, 30, 100, 60, { ...base, curveFitting: 1, fill: '#a5d8ff', fillStyle: 'hachure', fillWeight: 1, hachureGap: 8 }));
cases.polygon_diamond = dump(g.polygon([[110, 0], [220, 65], [110, 128], [0, 65]], { ...base }));
cases.linear_path = dump(g.linearPath([[0, 0], [92, -26], [176, 16]], { ...base }));
cases.curve = dump(g.curve([[0, 0], [92, -26], [176, 16]], { ...base }));
cases.path_rounded = dump(g.path('M 32 0 L 188 0 Q 220 0, 220 32 L 220 96 Q 220 128, 188 128 L 32 128 Q 0 128, 0 96 L 0 32 Q 0 0, 32 0', { ...base, preserveVertices: true }));
cases.line = dump(g.line(0, 0, 176, 42, { ...base }));

console.log(JSON.stringify(cases, null, 1));
