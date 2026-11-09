// title: Access
// description: Who may do what in an app that several people share: owners, members and the author of a row, as one policy the views and the server functions both check, so a button is hidden where the server would refuse.
// api: can(user, action, row, policy) -> bool; user is app.me() or app.user ({id, role}) or null;
//   policy: {view, add, edit, remove, field = 'author'} where each action is who may: 'anyone' | 'signed-in' | 'members' | 'author' | 'owner'
//   or a list of them ('author' means the row's field holds the user's id; owners may always do everything);
//   default policy: anyone views, members add, the author or an owner edits and removes;
//   allowed(user, rows, action, policy) -> the rows the user may act on; isOwner(user), isMember(user) -> bool;
//   owns(user, row, field = 'author') -> bool; stamp(user, row, field = 'author') -> the row with the user's id put in field when empty;
//   why(user, action, row, policy) -> '' when allowed, else a sentence for the person ('Sign in to edit this')
function helper() {
  const DEFAULT = { view: 'anyone', add: 'members', edit: ['author', 'owner'], remove: ['author', 'owner'], field: 'author' };
  const isOwner = (u) => !!u && u.role === 'owner';
  const isMember = (u) => !!u && (u.role === 'owner' || u.role === 'member' || u.role === 'admin');
  const owns = (u, row, field = 'author') => !!u && !!row && row[field] !== undefined && row[field] !== null && String(row[field]) === String(u.id);
  function rule(user, who, row, field) {
    switch (who) {
      case 'anyone': return true;
      case 'signed-in': return !!user;
      case 'members': return isMember(user);
      case 'author': return owns(user, row, field);
      case 'owner': return isOwner(user);
      default: return false;
    }
  }
  function can(user, action, row, policy) {
    const p = { ...DEFAULT, ...(policy || {}) };
    if (isOwner(user)) return true;
    const who = p[action] === undefined ? 'owner' : p[action];
    return [].concat(who).some((w) => rule(user, w, row, p.field));
  }
  const allowed = (user, rows, action, policy) => (rows || []).filter((r) => can(user, action, r, policy));
  function stamp(user, row, field = 'author') {
    const r = { ...(row || {}) };
    if (user && (r[field] === undefined || r[field] === null || r[field] === '')) r[field] = user.id;
    return r;
  }
  function why(user, action, row, policy) {
    if (can(user, action, row, policy)) return '';
    const verb = { view: 'see', add: 'add', edit: 'change', remove: 'delete' }[action] || action;
    if (!user) return `Sign in to ${verb} this`;
    const p = { ...DEFAULT, ...(policy || {}) };
    const who = [].concat(p[action] === undefined ? 'owner' : p[action]);
    if (who.includes('author')) return `Only the person who added this${who.includes('owner') ? ' or an owner' : ''} can ${verb} it`;
    if (who.includes('members')) return `Only members of this app can ${verb} this`;
    return `Only an owner of this app can ${verb} this`;
  }
  return { can, allowed, isOwner, isMember, owns, stamp, why };
}
