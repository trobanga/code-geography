<script lang="ts">
  /**
   * The picture for a load in progress: a survey of unknown ground. Contour lines drift, a
   * sweep turns, and each box lights when the sweep goes across it; then its lines draw.
   */
  let { label }: { label: string } = $props();

  /** Period of the sweep in seconds. */
  const T = 3.6;
  const R = 84;

  /** A closed contour line: a circle of radius `r` with two waves on it. */
  function contour(r: number, a: number, p: number): string {
    const n = 72;
    let d = '';
    for (let i = 0; i < n; i++) {
      const t = (i / n) * 2 * Math.PI;
      const q = r * (1 + a * Math.sin(3 * t + p) + 0.6 * a * Math.sin(5 * t - 2 * p));
      d += `${i === 0 ? 'M' : 'L'}${(q * Math.cos(t)).toFixed(1)} ${(q * Math.sin(t)).toFixed(1)}`;
    }
    return d + 'Z';
  }
  const contours = [
    { d: contour(78, 0.05, 0.3), turn: 61 },
    { d: contour(63, 0.07, 1.9), turn: -47 },
    { d: contour(48, 0.09, 3.1), turn: 37 },
    { d: contour(33, 0.11, 4.4), turn: -29 },
    { d: contour(18, 0.14, 5.6), turn: 23 },
  ];

  /** Boxes at an angle (degrees, clockwise from north) and a distance; the colour is the kind of a node. */
  const boxes = [
    { a: 20, r: 58, s: 13, kind: 'module' },
    { a: 75, r: 34, s: 9, kind: 'type' },
    { a: 128, r: 66, s: 11, kind: 'package' },
    { a: 190, r: 42, s: 8, kind: 'function' },
    { a: 245, r: 70, s: 12, kind: 'type' },
    { a: 300, r: 30, s: 8, kind: 'value' },
    { a: 335, r: 62, s: 10, kind: 'package' },
  ].map((b) => {
    const t = (b.a * Math.PI) / 180;
    return { ...b, x: b.r * Math.sin(t), y: -b.r * Math.cos(t), delay: (b.a / 360 - 1) * T };
  });
  /** A line draws when the sweep is at its later end. */
  const lines = [
    [0, 1],
    [1, 2],
    [2, 3],
    [3, 4],
    [4, 5],
    [5, 6],
    [1, 5],
    [0, 6],
  ].map(([i, j]) => ({ from: boxes[i], to: boxes[j], delay: Math.max(boxes[i].delay, boxes[j].delay) }));
  /** The trail behind the sweep line. */
  const trail = Array.from({ length: 14 }, (_, k) => ({ a: -k * 2.6, o: 0.5 * (1 - k / 14) ** 2 }));
</script>

<div class="flex flex-col items-center gap-4" role="status" aria-live="polite" data-survey>
  <svg viewBox="-100 -100 200 200" class="h-56 w-56" aria-hidden="true" style="--T: {T}s">
    <g fill="none" stroke="var(--line)" stroke-width="1">
      {#each contours as c}
        <path d={c.d} class="contour" style="--turn: {c.turn}s" />
      {/each}
    </g>
    <g stroke="var(--edge)" stroke-width="1">
      {#each lines as l}
        <line x1={l.from.x} y1={l.from.y} x2={l.to.x} y2={l.to.y} pathLength="1" class="line" style="animation-delay: {l.delay}s" />
      {/each}
    </g>
    <g class="sweep">
      {#each trail as t}
        <line x1="0" y1="0" x2="0" y2={-R} stroke="var(--accent)" stroke-width="2.2" opacity={t.o} transform="rotate({t.a})" />
      {/each}
      <line x1="0" y1="0" x2="0" y2={-R} stroke="var(--accent)" stroke-width="1.2" />
    </g>
    {#each boxes as b}
      <rect
        x={b.x - b.s / 2}
        y={b.y - b.s / 2}
        width={b.s}
        height={b.s}
        rx="1.5"
        class="box"
        style="--k: var(--k-{b.kind}); animation-delay: {b.delay}s; transform-origin: {b.x}px {b.y}px"
      />
    {/each}
    <circle r="2.2" fill="var(--accent)" />
  </svg>
  <div class="text-[var(--muted)]" data-phase>{label}…</div>
</div>

<style>
  .contour {
    transform-origin: 0 0;
    animation: turn var(--turn) linear infinite;
  }
  .sweep {
    transform-origin: 0 0;
    animation: turn var(--T) linear infinite;
  }
  .box {
    fill: var(--bg);
    stroke: var(--k);
    stroke-width: 1.4;
    animation: found var(--T) ease-out infinite;
  }
  .line {
    stroke-dasharray: 1;
    animation: draw var(--T) ease-out infinite;
  }
  @keyframes turn {
    to {
      transform: rotate(360deg);
    }
  }
  @keyframes found {
    0% {
      fill: var(--k);
      transform: scale(1.7);
      opacity: 1;
    }
    25% {
      fill: var(--bg);
      transform: scale(1);
    }
    100% {
      fill: var(--bg);
      transform: scale(1);
      opacity: 0.3;
    }
  }
  @keyframes draw {
    0% {
      stroke-dashoffset: 1;
      opacity: 1;
    }
    30% {
      stroke-dashoffset: 0;
      opacity: 1;
    }
    100% {
      stroke-dashoffset: 0;
      opacity: 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .contour,
    .box,
    .line {
      animation: none;
    }
    .sweep {
      animation-duration: 12s;
    }
  }
</style>
