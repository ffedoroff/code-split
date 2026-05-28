const CHIP_CLASSES = {
  'nodes-added':     [null, 'hide-nodes-added'],
  'nodes-removed':   [null, 'hide-nodes-removed'],
  'nodes-affected':  [null, 'hide-nodes-affected'],
  'nodes-unchanged': [null, 'hide-nodes-unchanged'],
  'edges-added':     [null, 'hide-edges-added'],
  'edges-removed':   [null, 'hide-edges-removed'],
  'edges-affected':  [null, 'hide-edges-affected'],
  'edges-unchanged': [null, 'hide-edges-unchanged'],
  'cycle-before':    ['show-cycle-before', null],
  'cycle-after':     ['show-cycle-after',  null],
};

const PRESETS = {
  before: {
    'nodes-added':false,'nodes-removed':true,'nodes-affected':true,'nodes-unchanged':true,
    'edges-added':false,'edges-removed':true,'edges-affected':true,'edges-unchanged':true,
    'cycle-before':true,'cycle-after':false,
  },
  after: {
    'nodes-added':true,'nodes-removed':false,'nodes-affected':true,'nodes-unchanged':true,
    'edges-added':true,'edges-removed':false,'edges-affected':true,'edges-unchanged':true,
    'cycle-before':false,'cycle-after':true,
  },
  diff: {
    'nodes-added':true,'nodes-removed':true,'nodes-affected':true,'nodes-unchanged':false,
    'edges-added':true,'edges-removed':true,'edges-affected':false,'edges-unchanged':false,
    'cycle-before':true,'cycle-after':true,
  },
  cycles: {
    'nodes-added':false,'nodes-removed':false,'nodes-affected':false,'nodes-unchanged':false,
    'edges-added':false,'edges-removed':false,'edges-affected':false,'edges-unchanged':false,
    'cycle-before':true,'cycle-after':true,
  },
};

const TOGGLE_CLASSES = [
  'hide-nodes-added','hide-nodes-removed','hide-nodes-affected','hide-nodes-unchanged',
  'hide-edges-added','hide-edges-removed','hide-edges-affected','hide-edges-unchanged',
  'show-cycle-before','show-cycle-after',
];

function setupView(section) {
  const level = section.dataset.view;
  const frame = section.querySelector('.svg-frame');

  // Populate chip counts from diff/cycle data
  const { nodes, edges } = window.DIFF[level];
  const cycles = window.CYCLES[level];

  const nc = {added:0,removed:0,affected:0,unchanged:0};
  const ec = {added:0,removed:0,affected:0,unchanged:0};
  nodes.forEach(n => { if (n.status in nc) nc[n.status]++; });
  edges.forEach(e => { if (e.status in ec) ec[e.status]++; });

  const setChip = (id, text, disabled) => {
    const el = section.querySelector(`[data-chip="${id}"]`);
    if (!el) return;
    el.textContent = text;
    el.classList.toggle('disabled', !!disabled);
    if (disabled) el.classList.remove('active');
  };
  setChip('nodes-added',     `+${nc.added} added`,      nc.added     === 0);
  setChip('nodes-removed',   `−${nc.removed} removed`,  nc.removed   === 0);
  setChip('nodes-affected',  `${nc.affected} affected`,  nc.affected  === 0);
  setChip('nodes-unchanged', `${nc.unchanged} unchanged`,nc.unchanged === 0);
  setChip('edges-added',     `+${ec.added} added`,      ec.added     === 0);
  setChip('edges-removed',   `−${ec.removed} removed`,  ec.removed   === 0);
  setChip('edges-affected',  `${ec.affected} affected`,  ec.affected  === 0);
  setChip('edges-unchanged', `${ec.unchanged} unchanged`,ec.unchanged === 0);
  const nBefore = cycles.cycleBefore + cycles.cycleBoth;
  const nAfter  = window.AFTER !== null ? cycles.cycleAfter + cycles.cycleBoth : 0;
  setChip('cycle-before', `${nBefore} removed`, nBefore === 0);
  setChip('cycle-after',  `+${nAfter} added`,  nAfter  === 0 || window.AFTER === null);

  function applyFrameClasses() {
    TOGGLE_CLASSES.forEach(c => frame.classList.remove(c));
    for (const [id, [whenOn, whenOff]] of Object.entries(CHIP_CLASSES)) {
      const chip = section.querySelector(`[data-chip="${id}"]`);
      if (!chip || chip.classList.contains('disabled')) continue;
      const on = chip.classList.contains('active');
      if (on  && whenOn)  frame.classList.add(whenOn);
      if (!on && whenOff) frame.classList.add(whenOff);
    }

    // Hide clusters that contain no visible nodes
    const fc = frame.classList;
    frame.querySelectorAll('g.cluster').forEach(cluster => {
      const visible = [...cluster.querySelectorAll('g.node')].some(n => {
        const nc = n.classList;
        const hiddenByStatus =
          (nc.contains('status-added')     && fc.contains('hide-nodes-added'))     ||
          (nc.contains('status-removed')   && fc.contains('hide-nodes-removed'))   ||
          (nc.contains('status-affected')  && fc.contains('hide-nodes-affected'))  ||
          (nc.contains('status-unchanged') && fc.contains('hide-nodes-unchanged'));
        if (!hiddenByStatus) return true;
        // Cycle override
        if (fc.contains('show-cycle-before') && (nc.contains('cycle-status-before-only') || nc.contains('cycle-status-both'))) return true;
        if (fc.contains('show-cycle-after')  && (nc.contains('cycle-status-after-only')  || nc.contains('cycle-status-both'))) return true;
        return false;
      });
      cluster.style.display = visible ? '' : 'none';
    });
    section._refreshNodeTable?.();
  }

  function chipsMatchPreset(name) {
    const cfg = PRESETS[name];
    for (const [id, want] of Object.entries(cfg)) {
      const chip = section.querySelector(`[data-chip="${id}"]`);
      if (!chip || chip.classList.contains('disabled')) continue;
      if (chip.classList.contains('active') !== want) return false;
    }
    return true;
  }

  function applyPreset(name) {
    const cfg = PRESETS[name];
    for (const [id, want] of Object.entries(cfg)) {
      const chip = section.querySelector(`[data-chip="${id}"]`);
      if (!chip || chip.classList.contains('disabled')) continue;
      chip.classList.toggle('active', want);
    }
    applyFrameClasses();
  }

  section.querySelectorAll('.chip').forEach(chip => {
    chip.addEventListener('click', () => {
      if (chip.classList.contains('disabled')) return;
      chip.classList.toggle('active');
      applyFrameClasses();
      const matched = Object.keys(PRESETS).find(chipsMatchPreset);
      if (matched) setActivePreset(matched);
      else showCustomState();
    });
  });

  section._applyPreset       = applyPreset;
  section._matchesPreset     = chipsMatchPreset;
  section._applyFrameClasses = applyFrameClasses;
  section._detectPreset      = () => Object.keys(PRESETS).find(chipsMatchPreset) ?? null;
  section._refreshCounts     = () => {
    const { nodes, edges } = window.DIFF[level];
    const cycles = window.CYCLES[level];
    const nc = {added:0,removed:0,affected:0,unchanged:0};
    const ec = {added:0,removed:0,affected:0,unchanged:0};
    nodes.forEach(n => { if (n.status in nc) nc[n.status]++; });
    edges.forEach(e => { if (e.status in ec) ec[e.status]++; });
    setChip('nodes-added',     `+${nc.added} added`,      nc.added     === 0);
    setChip('nodes-removed',   `−${nc.removed} removed`,  nc.removed   === 0);
    setChip('nodes-affected',  `${nc.affected} affected`,  nc.affected  === 0);
    setChip('nodes-unchanged', `${nc.unchanged} unchanged`,nc.unchanged === 0);
    setChip('edges-added',     `+${ec.added} added`,      ec.added     === 0);
    setChip('edges-removed',   `−${ec.removed} removed`,  ec.removed   === 0);
    setChip('edges-affected',  `${ec.affected} affected`,  ec.affected  === 0);
    setChip('edges-unchanged', `${ec.unchanged} unchanged`,ec.unchanged === 0);
    const nBefore = cycles.cycleBefore + cycles.cycleBoth;
    const nAfter  = window.AFTER !== null ? cycles.cycleAfter + cycles.cycleBoth : 0;
    setChip('cycle-before', `before: ${nBefore}`, nBefore === 0);
    setChip('cycle-after',  `after: ${nAfter}`,   nAfter  === 0 || window.AFTER === null);
    applyFrameClasses();
  };
  applyFrameClasses();
}
