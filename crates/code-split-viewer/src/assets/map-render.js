// map-render.js — the main-map render entry points: drawSVG (big-graph guard)
// and renderSVGNow (DOT -> SVG via graphviz, then wire pan/zoom + interactions).
// Split out of diagram.js.

// Above this many nodes, laying out the graph with graphviz is slow, so we ask
// for explicit confirmation before rendering (once per frame).
const SVG_NODE_LIMIT = 500;

function drawSVG(svgFrame, nodes, edges, level) {
  const drillGroup = window.drillGroup || null;

  // Group view (drillGroup=null) is always fast — just one node per group.
  // Only warn when drilled into a very large group.
  if (drillGroup !== null) {
    const gOf = grouperForDig(level, window.drillDig ?? 0);
    const drillCount = nodes.filter(n => gOf(n) === drillGroup).length;
    if (drillCount > SVG_NODE_LIMIT && svgFrame.dataset.bigConfirmed !== '1') {
      svgFrame.innerHTML =
        `<div class="too-many">` +
          `<div class="too-many-title">too many nodes: ${drillCount}</div>` +
          `<div class="too-many-sub">Rendering the full diagram may be slow. Render it anyway?</div>` +
          `<button class="too-many-btn" type="button">Render diagram</button>` +
        `</div>`;
      svgFrame.querySelector('.too-many-btn').addEventListener('click', () => {
        svgFrame.dataset.bigConfirmed = '1';
        const loading = svgFrame.closest('[data-view]')?.querySelector('.loading-indicator');
        if (loading) { loading.textContent = 'Computing layout…'; loading.classList.add('on'); }
        setTimeout(() => {
          renderSVGNow(svgFrame, nodes, edges, level);
          if (loading) loading.classList.remove('on');
        }, 30);
      });
      return;
    }
  }
  renderSVGNow(svgFrame, nodes, edges, level);
}

function renderSVGNow(svgFrame, nodes, edges, level) {
  const vpW = svgFrame.offsetWidth  || svgFrame.clientWidth  || 0;
  const vpH = svgFrame.offsetHeight || svgFrame.clientHeight || 0;
  const viewport = (vpW > 0 && vpH > 0) ? { w: vpW, h: vpH } : null;
  const dot = buildDOT(nodes, edges, level, viewport);
  const svgStr = window.gv.dot(dot);
  svgFrame.innerHTML = svgStr;
  const svg = svgFrame.querySelector('svg');
  if (svg) {
    svg.setAttribute('width', '100%');
    svg.setAttribute('height', '100%');
    svg.style.display = 'block';
    setupPanZoom(svgFrame, svg);
    // Status bar: one persistent element per frame-wrap, reused across re-renders.
    const fw = svgFrame.parentElement;
    let statusBar = fw.querySelector(':scope > .svg-status-bar');
    if (!statusBar) {
      statusBar = document.createElement('div');
      statusBar.className = 'svg-status-bar';
      fw.appendChild(statusBar);
    }
    statusBar.hidden = true;
    statusBar.textContent = '';
    svgFrame._statusBar = statusBar;
    setupEdgeHighlight(svgFrame, level);   // reads titles before setupTooltips removes them
    setupTooltips(svgFrame, level);
    normalizeArrows(svgFrame, svg);
  }
}

// Arrowheads are FILLED polygons, so `vector-effect: non-scaling-stroke` (which
// only affects strokes) can't keep them a constant size. Counter-scale each
// arrowhead by the SVG's current fit factor (viewBox width ÷ frame px width)
// AROUND ITS TIP (the apex touching the target node) so the tip stays on the
// node — no gap — while the arrow keeps a ~constant on-screen size. Also exposes
// `--zk` for the blue hover drop-shadow. Re-run on render AND on every zoom (the
// viewBox width changes), since the fit factor changes with zoom.
function normalizeArrows(svgFrame, svg) {
  const vbw = svg.viewBox?.baseVal?.width || 0;
  const px  = svgFrame.clientWidth || svgFrame.offsetWidth || 0;
  if (!vbw || !px) return;
  const k = vbw / px;                       // user units per screen px (fit factor)
  svgFrame.style.setProperty('--zk', k.toFixed(4));
  const reset = Math.abs(k - 1) < 0.02;     // ~1:1 — strip any prior adjustment
  for (const poly of svg.querySelectorAll('g.edge > polygon')) {
    const path = poly.parentNode.querySelector('path');
    if (reset) {
      poly.removeAttribute('transform');
      if (path && path.dataset.d0) { path.setAttribute('d', path.dataset.d0); delete path.dataset.d0; }
      continue;
    }
    const seen = new Set();
    const pts = (poly.getAttribute('points') || '').trim().split(/\s+/)
      .map(p => p.split(',').map(Number)).filter(p => p.length === 2)
      // Unique vertices only — graphviz closes the polygon by repeating the first
      // vertex (`P1 P2 P3 P1`); counting it twice would skew the base centre and
      // shift the line/arrow junction by a pixel or two.
      .filter(p => { const key = p[0] + ',' + p[1]; if (seen.has(key)) return false; seen.add(key); return true; });
    if (pts.length < 2) continue;
    // Tip = the vertex farthest from the midpoint of the others (triangle apex,
    // touching the target node); base centre = midpoint of the other vertices.
    let tipIdx = 0, best = -1;
    for (let i = 0; i < pts.length; i++) {
      const o = pts.filter((_, j) => j !== i);
      const mx = o.reduce((s, p) => s + p[0], 0) / o.length;
      const my = o.reduce((s, p) => s + p[1], 0) / o.length;
      const d = (pts[i][0] - mx) ** 2 + (pts[i][1] - my) ** 2;
      if (d > best) { best = d; tipIdx = i; }
    }
    const tip  = pts[tipIdx];
    const rest = pts.filter((_, j) => j !== tipIdx);
    const bcx  = rest.reduce((s, p) => s + p[0], 0) / rest.length;
    const bcy  = rest.reduce((s, p) => s + p[1], 0) / rest.length;
    // Scale the arrow around its TIP (tip stays on the node — no target gap),
    // then extend the edge line to the scaled arrow's new base centre so the
    // line still meets the arrow.
    poly.setAttribute('transform', `translate(${tip[0]} ${tip[1]}) scale(${k.toFixed(4)}) translate(${-tip[0]} ${-tip[1]})`);
    if (path) {
      if (!path.dataset.d0) path.dataset.d0 = path.getAttribute('d');
      const bx = tip[0] + k * (bcx - tip[0]);
      const by = tip[1] + k * (bcy - tip[1]);
      // Replace the path's final endpoint (the arrow-side end) with the new base.
      path.setAttribute('d', path.dataset.d0.replace(/(-?[\d.]+)[ ,](-?[\d.]+)\s*$/, `${bx.toFixed(2)},${by.toFixed(2)}`));
    }
  }
}
window.normalizeArrows = normalizeArrows;
