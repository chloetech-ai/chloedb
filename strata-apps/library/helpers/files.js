// title: Files
// description: File names, types and sizes as people see them: readable sizes, the extension and base of a name, the MIME type of common files and the kind of file (image, video, pdf, spreadsheet, ...), for upload lists, icons and previews.
// api: size(bytes, places = 1) -> '0 B', '512 B', '1.5 KB', '1.2 MB', '3 GB' (1024 per step, trailing zeros dropped; null for a missing, negative or non-numeric size);
//   extension(name) -> the last extension, lower case, without the dot ('Report.PDF' -> 'pdf', 'archive.tar.gz' -> 'gz'; '' when none, and for '.bashrc');
//   base(name) -> the name without its folder and last extension ('/docs/archive.tar.gz' -> 'archive.tar', '.bashrc' -> '.bashrc'; '' when missing);
//   names may be paths ('a/b.pdf', 'C:\\x\\b.pdf') or URLs (the ?query and #hash are ignored);
//   mime(name or extension) -> the MIME type of a common file ('a.pdf' -> 'application/pdf', 'png' -> 'image/png'), or null when unknown;
//   kind(name, extension or MIME type) -> 'image' | 'video' | 'audio' | 'pdf' | 'doc' (word processing: doc, docx, odt, rtf, pages, epub) | 'spreadsheet'
//   | 'presentation' | 'archive' | 'text' | 'code' | 'font' | 'other' ('text/csv; charset=utf-8' -> 'spreadsheet', 'clip.mov' -> 'video', unknown -> 'other')
function helper() {
  const DOCU = 'doc' + 'ument';
  const OX = 'application/vnd.openxmlformats-office' + DOCU + '.';
  const OD = 'application/vnd.oasis.open' + DOCU + '.';
  const T = {};
  const add = (kind, list) => { for (const [exts, type] of list) for (const e of exts.split(' ')) T[e] = { type, kind }; };
  add('image', [['jpg jpeg jpe', 'image/jpeg'], ['png', 'image/png'], ['gif', 'image/gif'], ['webp', 'image/webp'], ['svg', 'image/svg+xml'],
    ['bmp', 'image/bmp'], ['ico', 'image/x-icon'], ['tif tiff', 'image/tiff'], ['heic', 'image/heic'], ['heif', 'image/heif'], ['avif', 'image/avif']]);
  add('video', [['mp4', 'video/mp4'], ['m4v', 'video/x-m4v'], ['mov', 'video/quicktime'], ['webm', 'video/webm'], ['mkv', 'video/x-matroska'],
    ['avi', 'video/x-msvideo'], ['mpeg mpg', 'video/mpeg'], ['ogv', 'video/ogg'], ['3gp', 'video/3gpp']]);
  add('audio', [['mp3', 'audio/mpeg'], ['wav', 'audio/wav'], ['ogg oga', 'audio/ogg'], ['m4a', 'audio/mp4'], ['aac', 'audio/aac'],
    ['flac', 'audio/flac'], ['opus', 'audio/opus'], ['weba', 'audio/webm'], ['mid midi', 'audio/midi']]);
  add('pdf', [['pdf', 'application/pdf']]);
  add('doc', [['doc', 'application/msword'], ['docx', OX + 'wordprocessingml.' + DOCU], ['odt', OD + 'text'],
    ['rtf', 'application/rtf'], ['pages', 'application/vnd.apple.pages'], ['epub', 'application/epub+zip']]);
  add('spreadsheet', [['xls', 'application/vnd.ms-excel'], ['xlsx', OX + 'spreadsheetml.sheet'], ['ods', OD + 'spreadsheet'],
    ['csv', 'text/csv'], ['tsv', 'text/tab-separated-values'], ['numbers', 'application/vnd.apple.numbers']]);
  add('presentation', [['ppt', 'application/vnd.ms-powerpoint'], ['pptx', OX + 'presentationml.presentation'], ['odp', OD + 'presentation'],
    ['key', 'application/vnd.apple.keynote']]);
  add('archive', [['zip', 'application/zip'], ['rar', 'application/vnd.rar'], ['7z', 'application/x-7z-compressed'], ['tar', 'application/x-tar'],
    ['gz tgz', 'application/gzip'], ['bz2', 'application/x-bzip2'], ['xz', 'application/x-xz']]);
  add('text', [['txt log', 'text/plain'], ['md markdown', 'text/markdown'], ['ics', 'text/calendar'], ['vcf', 'text/vcard']]);
  add('code', [['html htm', 'text/html'], ['css', 'text/css'], ['js mjs', 'text/javascript'], ['json', 'application/json'],
    ['xml', 'application/xml'], ['yaml yml', 'application/yaml'], ['py', 'text/x-python'], ['sh', 'application/x-sh'], ['sql', 'application/sql']]);
  add('font', [['woff', 'font/woff'], ['woff2', 'font/woff2'], ['ttf', 'font/ttf'], ['otf', 'font/otf']]);
  const BY_TYPE = {};
  for (const e of Object.keys(T)) if (!BY_TYPE[T[e].type]) BY_TYPE[T[e].type] = T[e].kind;
  const TOPS = { image: 'image', video: 'video', audio: 'audio', text: 'text', font: 'font', application: 'other', model: 'other', multipart: 'other', message: 'other' };
  const UNITS = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];

  function size(bytes, places = 1) {
    if (bytes === null || bytes === undefined || bytes === '' || typeof bytes === 'boolean') return null;
    const n = Number(bytes);
    if (!isFinite(n) || n < 0) return null;
    const p = Math.max(0, Math.min(6, Math.floor(Number(places) || 0)));
    const f = Math.pow(10, p);
    const round = (v) => Math.round(v * f) / f;
    let v = n;
    let u = 0;
    while (v >= 1024 && u < UNITS.length - 1) { v /= 1024; u++; }
    if (u === 0) return `${Math.round(v)} B`;
    let r = round(v);
    // 1023.96 KB rounds to 1024: say 1 MB instead
    if (r >= 1024 && u < UNITS.length - 1) { u++; r = round(v / 1024); }
    return `${r} ${UNITS[u]}`;
  }
  function leaf(name) {
    if (typeof name !== 'string') return '';
    let s = name.trim();
    if (s.includes('://')) s = s.split('#')[0].split('?')[0];
    s = s.replace(/[\\/]+$/, '');
    const parts = s.split(/[\\/]/);
    return parts[parts.length - 1] || '';
  }
  function split(name) {
    const s = leaf(name);
    const i = s.lastIndexOf('.');
    // no dot, a leading dot (.bashrc) or a trailing dot: no extension
    if (i <= 0 || i === s.length - 1) return { base: s, ext: '' };
    return { base: s.slice(0, i), ext: s.slice(i + 1).toLowerCase() };
  }
  function extension(name) { return split(name).ext; }
  function base(name) { return split(name).base; }
  function lookup(name) {
    if (typeof name !== 'string') return null;
    const ext = extension(name);
    if (ext) return T[ext] || null;
    const bare = name.trim().toLowerCase().replace(/^\./, '');
    return T[bare] || null;
  }
  function mime(name) { const t = lookup(name); return t ? t.type : null; }
  function asMime(s) {
    const m = /^([a-z]+)\/([a-z0-9.+-]+)$/.exec(s.split(';')[0].trim().toLowerCase());
    return m && TOPS[m[1]] ? m : null;
  }
  function kind(value) {
    if (typeof value !== 'string' || !value.trim()) return 'other';
    const m = asMime(value);
    if (m) {
      const type = `${m[1]}/${m[2]}`;
      if (BY_TYPE[type]) return BY_TYPE[type];
      if (/(^|[.+-])(zip|compressed|tar|rar|gzip)($|[.+-])/.test(m[2])) return 'archive';
      if (/(^|\+)(json|xml)$/.test(m[2])) return 'code';
      return TOPS[m[1]];
    }
    const t = lookup(value);
    return t ? t.kind : 'other';
  }
  return { size, extension, base, mime, kind };
}
