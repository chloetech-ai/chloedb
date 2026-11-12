// What a view's frame may do, in one place so the studio and the tests read
// the same policy (audit G-B2).
//
// A view is code a model wrote, and it may show content fetched from the
// web, so the frame is sandboxed without its own origin and its page has a
// Content-Security-Policy with no network: everything it does to data goes to
// the studio as a message. The channels that still left were images and media
// from any https address (a request that can carry rows in its URL) and
// popups that escaped the sandbox. Now:
//
// - images and media come from the engine (files, map tiles), data: and
//   blob: URLs, and the hosts the engine's own photos come from; any https
//   address only once the app's owner has switched on "Web pages and APIs",
//   the consent app.fetch already asks for;
// - no form submits anywhere (views handle submit themselves);
// - no popups: a link opens from the studio, after a click in the frame.
export const PHOTO_HOSTS = ['https://images.unsplash.com', 'https://upload.wikimedia.org', 'https://live.staticflickr.com'];

export function frameCsp(origin, { outward = false } = {}) {
  const media = ['data:', 'blob:', origin, ...(outward ? ['https:'] : PHOTO_HOSTS)].join(' ');
  return `default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src ${media}; media-src ${media}; font-src data:; form-action 'none'`;
}

export const FRAME_SANDBOX = 'allow-scripts allow-forms allow-modals allow-downloads';

// A link a frame asks the studio to open: http or https only.
export function linkToOpen(href) {
  try {
    const u = new URL(String(href || ''));
    return u.protocol === 'http:' || u.protocol === 'https:' ? u.href : null;
  } catch { return null; }
}
