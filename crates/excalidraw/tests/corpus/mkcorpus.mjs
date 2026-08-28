// Builds a corpus in exactly the shape Excalidraw writes: JSON.stringify(data, null, 2),
// element keys in the order restoreElementWithProperties leaves them.
import { writeFileSync } from 'node:fs';

const base = (over) => ({
  id: over.id,
  type: over.type,
  x: over.x, y: over.y,
  width: over.width, height: over.height,
  angle: over.angle ?? 0,
  strokeColor: over.strokeColor ?? '#1e1e1e',
  backgroundColor: over.backgroundColor ?? 'transparent',
  fillStyle: over.fillStyle ?? 'solid',
  strokeWidth: over.strokeWidth ?? 2,
  strokeStyle: over.strokeStyle ?? 'solid',
  roughness: over.roughness ?? 1,
  opacity: over.opacity ?? 100,
  groupIds: over.groupIds ?? [],
  frameId: over.frameId ?? null,
  index: over.index,
  roundness: over.roundness ?? null,
  seed: over.seed,
  version: over.version ?? 42,
  versionNonce: over.versionNonce ?? 1972238841,
  isDeleted: false,
  boundElements: over.boundElements ?? null,
  updated: over.updated ?? 1756304871234,
  link: null,
  locked: false,
  ...over.extra,
});

const file = (elements, appState = {}, files = {}) => JSON.stringify({
  type: 'excalidraw',
  version: 2,
  source: 'https://excalidraw.com',
  elements,
  appState: {
    gridSize: 20, gridStep: 5, gridModeEnabled: false,
    viewBackgroundColor: '#ffffff', lockedMultiSelections: {},
    ...appState,
  },
  files,
}, null, 2);

// --- shapes: every kind, every fill, every stroke style, rounded and sharp -----------------
const shapes = [
  base({ id: 'rect-adaptive', type: 'rectangle', x: 328, y: 216, width: 220, height: 128,
    index: 'a0', seed: 1263748391, backgroundColor: '#a5d8ff',
    roundness: { type: 3 }, boundElements: [{ id: 'arrow1', type: 'arrow' }] }),
  base({ id: 'rect-hachure', type: 'rectangle', x: 600, y: 216, width: 120, height: 90,
    index: 'a1', seed: 884517263, backgroundColor: '#ffc9c9', fillStyle: 'hachure',
    strokeStyle: 'dashed', strokeWidth: 1, roughness: 2 }),
  base({ id: 'diamond1', type: 'diamond', x: 328, y: 400, width: 160, height: 110,
    index: 'a2', seed: 431889201, backgroundColor: '#b2f2bb', fillStyle: 'cross-hatch',
    roundness: { type: 2 } }),
  base({ id: 'ellipse1', type: 'ellipse', x: 560, y: 400, width: 140, height: 140,
    index: 'a3', seed: 77120044, backgroundColor: '#ffec99', fillStyle: 'zigzag',
    strokeStyle: 'dotted', angle: 0.5235987755982988 }),
];

// --- linear: line, polygon line, arrow with binding, elbow arrow --------------------------
const linear = [
  base({ id: 'line1', type: 'line', x: 100, y: 600, width: 180, height: 60,
    index: 'b0', seed: 1590278022, roundness: { type: 2 },
    extra: { points: [[0, 0], [90, -60], [180, 0]], lastCommittedPoint: null,
      startBinding: null, endBinding: null, startArrowhead: null, endArrowhead: null,
      polygon: false } }),
  base({ id: 'arrow1', type: 'arrow', x: 553, y: 280, width: 176, height: 42,
    index: 'b1', seed: 884517263, strokeColor: '#e03131', roundness: { type: 2 },
    extra: { points: [[0, 0], [92, -26], [176, 16]], lastCommittedPoint: null,
      startBinding: { elementId: 'rect-adaptive', fixedPoint: [1.0227, 0.5001], mode: 'orbit' },
      endBinding: null, startArrowhead: 'circle', endArrowhead: 'triangle', elbowed: false } }),
  base({ id: 'elbow1', type: 'arrow', x: 800, y: 300, width: 120, height: 80,
    index: 'b2', seed: 66127410,
    extra: { points: [[0, 0], [60, 0], [60, 80], [120, 80]],
      startBinding: null, endBinding: null, startArrowhead: null, endArrowhead: 'arrow',
      elbowed: true, fixedSegments: [{ start: [60, 0], end: [60, 80], index: 1 }],
      startIsSpecial: null, endIsSpecial: null } }),
];

// --- freedraw ------------------------------------------------------------------------------
const freedraw = [
  base({ id: 'draw1', type: 'freedraw', x: 372, y: 412, width: 148.5, height: 61.25,
    index: 'c0', seed: 2038471925, strokeColor: '#1971c2', strokeWidth: 1,
    extra: { points: [[0, 0], [8.25, -4.5], [21.75, -12.25], [39.5, -19.75], [58, -24],
      [75.25, -23.5], [89.5, -17.25], [99.75, -6.5], [107.25, 8.5], [113.5, 24.25],
      [121.75, 33.75], [133, 36.25], [143.25, 33], [148.5, 27.5], [148.5, 27.5]],
      pressures: [0.15, 0.32, 0.48, 0.61, 0.7, 0.74, 0.77, 0.79, 0.78, 0.72, 0.63, 0.51, 0.37, 0.21, 0.05],
      simulatePressure: false,
      strokeOptions: { variability: 'variable', streamline: 0.5 } } }),
];

// --- text: free, bound, and in a frame -----------------------------------------------------
const text = [
  base({ id: 'frame1', type: 'frame', x: 60, y: 60, width: 400, height: 300,
    index: 'd0', seed: 12345, extra: { name: 'Overview' } }),
  base({ id: 'label1', type: 'text', x: 100, y: 100, width: 96.5, height: 25,
    index: 'd1', seed: 1010101, frameId: 'frame1',
    extra: { fontSize: 20, fontFamily: 5, text: 'A caption', textAlign: 'left',
      verticalAlign: 'top', containerId: null, originalText: 'A caption',
      autoResize: true, lineHeight: 1.25 } }),
  base({ id: 'boxed', type: 'rectangle', x: 700, y: 600, width: 200, height: 100,
    index: 'd2', seed: 999333, backgroundColor: '#d0bfff',
    boundElements: [{ id: 'inner', type: 'text' }] }),
  base({ id: 'inner', type: 'text', x: 720, y: 637.5, width: 160, height: 25,
    index: 'd3', seed: 444555,
    extra: { fontSize: 20, fontFamily: 8, text: 'inside', textAlign: 'center',
      verticalAlign: 'middle', containerId: 'boxed', originalText: 'inside',
      autoResize: false, lineHeight: 1.25 } }),
];

// --- image + files -------------------------------------------------------------------------
const png = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
const images = [
  base({ id: 'pic1', type: 'image', x: 1000, y: 100, width: 120, height: 120,
    index: 'e0', seed: 5150, strokeColor: 'transparent',
    roundness: null,
    extra: { fileId: 'a94a8fe5ccb19ba61c4c0873d391e987982fbbd3', status: 'saved',
      scale: [1, -1], crop: null } }),
];
const files = {
  a94a8fe5ccb19ba61c4c0873d391e987982fbbd3: {
    mimeType: 'image/png',
    id: 'a94a8fe5ccb19ba61c4c0873d391e987982fbbd3',
    dataURL: `data:image/png;base64,${png}`,
    created: 1756304871234,
    lastRetrieved: 1756304899000,
  },
};

// --- groups and a deleted element ----------------------------------------------------------
const groups = [
  base({ id: 'g1', type: 'rectangle', x: 0, y: 0, width: 50, height: 50, index: 'f0',
    seed: 111, groupIds: ['inner-group', 'outer-group'] }),
  base({ id: 'g2', type: 'ellipse', x: 60, y: 0, width: 50, height: 50, index: 'f1',
    seed: 222, groupIds: ['inner-group', 'outer-group'] }),
  { ...base({ id: 'gone', type: 'rectangle', x: 0, y: 100, width: 10, height: 10,
    index: 'f2', seed: 333 }), isDeleted: true },
];

// --- a file with unknown keys and an old schema ---------------------------------------------
const legacy = JSON.stringify({
  type: 'excalidraw',
  version: 1,
  source: 'https://excalidraw.com',
  elements: [
    { id: 'old', type: 'rectangle', x: 21801, y: 719.5, width: 50, height: 30,
      angle: 0, strokeColor: '#c92a2a', backgroundColor: '#e64980', fillStyle: 'hachure',
      strokeWidth: 1, strokeStyle: 'solid', roughness: 1, opacity: 100,
      groupIds: [], strokeSharpness: 'sharp', seed: 117297479,
      version: 38, versionNonce: 1046419680, isDeleted: false, boundElementIds: [],
      somethingNewer: { nested: [1, 2, 3] } },
  ],
  appState: { viewBackgroundColor: '#ffffff' },
}, null, 2);

writeFileSync('corpus/shapes.excalidraw', file(shapes));
writeFileSync('corpus/linear.excalidraw', file(linear));
writeFileSync('corpus/freedraw.excalidraw', file(freedraw));
writeFileSync('corpus/text.excalidraw', file(text));
writeFileSync('corpus/images.excalidraw', file(images, {}, files));
writeFileSync('corpus/groups.excalidraw', file(groups));
writeFileSync('corpus/legacy.excalidraw', legacy);
writeFileSync('corpus/empty.excalidraw', file([]));
console.log('written');
