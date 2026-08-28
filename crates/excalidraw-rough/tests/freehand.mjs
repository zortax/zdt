import { getStroke } from './node_modules/perfect-freehand/dist/esm/index.js';

const round = (v) => Math.round(v * 1e6) / 1e6;
const easeOutSine = (t) => Math.sin((t * Math.PI) / 2);

const points = [
  [0, 0], [8.25, -4.5], [21.75, -12.25], [39.5, -19.75], [58, -24],
  [75.25, -23.5], [89.5, -17.25], [99.75, -6.5], [107.25, 8.5],
  [113.5, 24.25], [121.75, 33.75], [133, 36.25], [143.25, 33],
  [148.5, 27.5], [148.5, 27.5],
];
const pressures = [0.15, 0.32, 0.48, 0.61, 0.7, 0.74, 0.77, 0.79, 0.78, 0.72, 0.63, 0.51, 0.37, 0.21, 0.05];

const opts = (simulate, size) => ({
  simulatePressure: simulate,
  size,
  thinning: 0.6,
  smoothing: 0.5,
  streamline: 0.5,
  easing: easeOutSine,
  last: true,
});

const cases = {};
cases.simulated = getStroke(points, opts(true, 1 * 4.25)).map((p) => p.map(round));
cases.recorded = getStroke(points.map(([x, y], i) => [x, y, pressures[i]]), opts(false, 2 * 4.25)).map((p) => p.map(round));
cases.dot = getStroke([[0, 0, 0.5]], opts(false, 2 * 4.25)).map((p) => p.map(round));
cases.two_points = getStroke([[0, 0, 0.5], [30, 10, 0.9]], opts(false, 2 * 4.25)).map((p) => p.map(round));

console.log(JSON.stringify(cases, null, 1));
