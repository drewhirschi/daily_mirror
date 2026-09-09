/* Read only supported JPEG EXIF fields; never infer historical camera settings. */
function readPhotoExif(buffer) {
  const data = new DataView(buffer), fields = {};
  const tags = {0x010f: 'Camera make', 0x0110: 'Camera model', 0x0131: 'Software',
    0x0112: 'EXIF orientation', 0x9003: 'EXIF capture time', 0x829a: 'Exposure (s)',
    0x829d: 'Aperture (f/)', 0x8827: 'ISO', 0x9204: 'Exposure bias (EV)',
    0x9206: 'Reported focus distance (m)', 0x920a: 'Focal length (mm)',
    0xa403: 'White balance mode'};
  if (data.byteLength < 4 || data.getUint16(0) !== 0xffd8) return fields;
  let offset = 2;
  while (offset + 4 <= data.byteLength) {
    if (data.getUint8(offset++) !== 0xff) break;
    while (offset < data.byteLength && data.getUint8(offset) === 0xff) offset++;
    if (offset >= data.byteLength) break;
    const marker = data.getUint8(offset++);
    if (marker === 0xda || marker === 0xd9) break;
    if (marker === 0x01 || (marker >= 0xd0 && marker <= 0xd7)) continue;
    if (offset + 2 > data.byteLength) break;
    const length = data.getUint16(offset), end = offset + length;
    if (length < 2 || end > data.byteLength) break;
    if (marker === 0xe1 && length >= 16 && data.getUint32(offset + 2) === 0x45786966 && data.getUint16(offset + 6) === 0) {
      const base = offset + 8;
      const order = data.getUint16(base);
      if (order !== 0x4949 && order !== 0x4d4d) return fields;
      const little = order === 0x4949;
      const valid = (p, n) => Number.isSafeInteger(p) && p >= base && p + n <= end;
      const u16 = p => data.getUint16(p, little), u32 = p => data.getUint32(p, little);
      if (u16(base + 2) !== 42) return fields;
      const visited = new Set();
      function directory(relative, depth = 0) {
        const p = base + relative;
        if (depth > 2 || visited.has(p) || !valid(p, 2)) return;
        visited.add(p);
        const count = u16(p);
        if (count > 512 || !valid(p + 2, count * 12)) return;
        for (let i = 0; i < count; i++) {
          const entry = p + 2 + i * 12, tag = u16(entry), type = u16(entry + 2), n = u32(entry + 4);
          if (tag === 0x8769 && type === 4 && n === 1) { directory(u32(entry + 8), depth + 1); continue; }
          if (!tags[tag]) continue;
          const size = ({2: 1, 3: 2, 4: 4, 5: 8, 10: 8})[type];
          if (!size || !n || n > 1024) continue;
          const at = n * size <= 4 ? entry + 8 : base + u32(entry + 8);
          if (!valid(at, n * size)) continue;
          let value;
          if (type === 2) value = new TextDecoder().decode(new Uint8Array(buffer, at, n)).replace(/\0.*$/s, '');
          else if (type === 3) value = u16(at);
          else if (type === 4) value = u32(at);
          else {
            const numerator = type === 10 ? data.getInt32(at, little) : u32(at);
            const denominator = type === 10 ? data.getInt32(at + 4, little) : u32(at + 4);
            if (!denominator) continue;
            value = numerator / denominator;
          }
          fields[tags[tag]] = value;
        }
      }
      directory(u32(base + 4));
      return fields;
    }
    offset = end;
  }
  return fields;
}
if (typeof module !== 'undefined') module.exports = {readPhotoExif};
if (typeof document !== 'undefined') {
  const style = document.createElement('style');
  style.textContent = `
    #local-photo-list{display:grid;grid-template-columns:repeat(auto-fill,minmax(150px,1fr));gap:12px}
    .local-photo-tile{padding:0;overflow:hidden;background:#222;border:1px solid #777;border-radius:8px;text-align:left}
    .local-photo-tile img{display:block;width:100%;aspect-ratio:4/3;object-fit:cover}
    .local-photo-tile span{display:block;padding:8px;font-size:12px;overflow-wrap:anywhere;color:white}
    #local-photo-viewer{width:100vw;max-width:none;height:100dvh;max-height:none;margin:0;padding:0;border:0;background:#111;color:#fff}
    #local-photo-viewer[open]{display:grid;grid-template-rows:auto minmax(0,1fr);position:fixed;inset:0;z-index:1000}
    #local-photo-viewer::backdrop{background:#111}
    .local-viewer-toolbar{display:flex;align-items:center;flex-wrap:wrap;gap:8px;padding:10px;background:#222}
    .local-viewer-toolbar button,.local-viewer-toolbar a{padding:8px 12px;font-size:14px;color:white;background:#333;border:1px solid #777;border-radius:6px}
    #local-viewer-title{flex:1;min-width:120px;margin:0;font-size:14px;overflow-wrap:anywhere}
    #local-viewer-stage{position:relative;min-height:0;overflow:auto;display:flex}
    #local-viewer-image{display:block;width:100%;height:100%;object-fit:contain;max-width:none;flex:none}
    #local-viewer-stage.native #local-viewer-image{width:auto;height:auto;object-fit:initial;align-self:flex-start}
    #local-viewer-image[hidden]{display:none}
    #local-viewer-metadata{position:absolute;right:12px;bottom:12px;max-width:min(390px,calc(100vw - 24px));max-height:55dvh;overflow:auto;background:rgba(0,0,0,.82);padding:12px;border-radius:8px;font:13px/1.5 system-ui;z-index:1}
    #local-viewer-metadata dl{display:grid;grid-template-columns:1fr 1fr;gap:4px 12px;margin:0}
    #local-viewer-metadata dt,#local-viewer-metadata dd{margin:0;overflow-wrap:anywhere}
    #local-viewer-metadata p{margin:8px 0 0;color:#ccc}
    #local-viewer-status{position:absolute;top:80px;left:20px;background:#111;padding:8px;pointer-events:none}
    #local-viewer-status:empty{display:none}
  `;
  document.head.append(style);
  const dialog = document.createElement('dialog');
  dialog.id = 'local-photo-viewer';
  dialog.setAttribute('aria-labelledby', 'local-viewer-title');
  dialog.innerHTML = `<div class="local-viewer-toolbar">
    <button type="button" id="local-viewer-prev" aria-label="Newer photo">←</button>
    <button type="button" id="local-viewer-next" aria-label="Older photo">→</button>
    <h2 id="local-viewer-title">Local photo</h2>
    <button type="button" id="local-viewer-zoom" aria-pressed="false">100%</button>
    <button type="button" id="local-viewer-info" aria-pressed="true">Metadata</button>
    <a id="local-viewer-download" download>Download original</a>
    <button type="button" id="local-viewer-close">Close</button>
    </div><div id="local-viewer-stage"><img id="local-viewer-image" alt="" hidden></div>
    <aside id="local-viewer-metadata" aria-label="Capture metadata"></aside>
    <div id="local-viewer-status" role="status" aria-live="polite"></div>`;
  document.body.append(dialog);
  const el = id => document.getElementById('local-viewer-' + id);
  let names = [], latestNames = [], selected = null, controller, objectUrl, generation = 0, opener, previousOverflow;
  const metadata = (values) => {
    const dl = document.createElement('dl');
    for (const [key, value] of Object.entries(values)) {
      const dt = document.createElement('dt'), dd = document.createElement('dd');
      dt.textContent = key; dd.textContent = value; dl.append(dt, dd);
    }
    const note = document.createElement('p');
    note.textContent = 'EXIF time has no assumed timezone. Focus distance is camera-reported. Settings not saved with this photo are unavailable.';
    el('metadata').replaceChildren(dl, note);
  };
  function resetImage() {
    el('image').onload = null; el('image').onerror = null;
    el('image').hidden = true; el('image').removeAttribute('src');
    if (objectUrl) URL.revokeObjectURL(objectUrl);
    objectUrl = null;
  }
  function zoom(native) {
    el('stage').classList.toggle('native', native);
    el('zoom').textContent = native ? 'Fit' : '100%';
    el('zoom').setAttribute('aria-pressed', String(native));
    el('stage').scrollTo(native ? Math.max(0, (el('image').naturalWidth - el('stage').clientWidth) / 2) : 0, native ? Math.max(0, (el('image').naturalHeight - el('stage').clientHeight) / 2) : 0);
  }
  async function show(name) {
    selected = name;
    const ticket = ++generation;
    controller?.abort(); controller = new AbortController(); resetImage(); zoom(false);
    const index = names.indexOf(name);
    el('title').textContent = `${index + 1} / ${names.length} · ${name}`;
    el('prev').disabled = index <= 0; el('next').disabled = index >= names.length - 1;
    const url = `/api/local/photos/${encodeURIComponent(name)}`;
    el('download').href = url;
    el('image').alt = `Local capture ${name}`;
    el('metadata').replaceChildren(); el('status').textContent = 'Loading original…';
    try {
      const response = await fetch(url, {signal: controller.signal});
      if (!response.ok) throw new Error(response.status === 404 ? 'This photo is no longer available.' : 'Could not load this photo.');
      const buffer = await response.arrayBuffer();
      if (ticket !== generation) return;
      let exif = {};
      try { exif = readPhotoExif(buffer); } catch { /* A malformed EXIF block must not prevent viewing. */ }
      const values = {'File size': `${(buffer.byteLength / 1048576).toFixed(2)} MiB`,
        'Exposure': exif['Exposure (s)'] > 0 ? `${(exif['Exposure (s)'] * 1000).toFixed(3)} ms (1/${Math.round(1 / exif['Exposure (s)'])} s)` : 'Unavailable',
        'ISO': exif.ISO ?? 'Unavailable', 'Reported focus distance (m)': exif['Reported focus distance (m)']?.toFixed(3) ?? 'Unavailable',
        'White balance mode': exif['White balance mode'] === 0 ? 'Auto' : exif['White balance mode'] === 1 ? 'Manual' : 'Unavailable',
        'Analogue / digital gain': 'Unavailable', 'AF mode / window': 'Unavailable', 'Denoise / sharpening': 'Unavailable'};
      for (const [key,value] of Object.entries(exif)) if (!(key in values) && key !== 'Exposure (s)') values[key] = value;
      el('image').onload = () => {
        if (ticket !== generation) return;
        values['Displayed dimensions'] = `${el('image').naturalWidth} × ${el('image').naturalHeight}`;
        metadata(values); el('image').hidden = false; el('status').textContent = '';
      };
      el('image').onerror = () => { if (ticket === generation) el('status').textContent = 'The original could not be decoded.'; };
      objectUrl = URL.createObjectURL(new Blob([buffer], {type:'image/jpeg'}));
      el('image').src = objectUrl;
    } catch (error) {
      if (ticket === generation && error.name !== 'AbortError') el('status').textContent = error.message;
    }
  }
  function move(delta) { const i = names.indexOf(selected) + delta; if (i >= 0 && i < names.length) show(names[i]); }
  el('prev').onclick = () => move(-1); el('next').onclick = () => move(1);
  el('close').onclick = () => dialog.close();
  el('zoom').onclick = () => zoom(!el('stage').classList.contains('native'));
  el('info').onclick = () => { el('metadata').hidden = !el('metadata').hidden; el('info').setAttribute('aria-pressed', String(!el('metadata').hidden)); };
  dialog.addEventListener('keydown', event => {
    if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') { event.preventDefault(); move(event.key === 'ArrowLeft' ? -1 : 1); }
  });
  dialog.addEventListener('close', () => {
    generation++; controller?.abort(); resetImage(); selected = null;
    document.body.style.overflow = previousOverflow;
    if (opener?.isConnected) opener.focus();
  });
  window.localPhotoViewer = {
    setPhotos(list) { latestNames = list.slice(); if (!dialog.open) names = latestNames.slice(); },
    open(name, button) {
      opener = button; names = latestNames.slice();
      if (!dialog.open) { previousOverflow = document.body.style.overflow; document.body.style.overflow = 'hidden'; dialog.showModal(); }
      show(name);
    }
  };
}
