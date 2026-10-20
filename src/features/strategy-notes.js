// Personal annotations belong to the WebView profile, never the replaceable core.
const NOTES_KEY = 'zapret.strategyNotes';
export const MAX_STRATEGY_NOTE_LENGTH = 50;

function noteKey(name) {
  return String(name).trim().replace(/\.bat$/i, '').trim().toLowerCase();
}

function readNotes(storage) {
  const raw = storage.getItem(NOTES_KEY);
  if (!raw) return Object.create(null);
  const data = JSON.parse(raw);
  if (data?.version !== 1 || !data.notes || typeof data.notes !== 'object' || Array.isArray(data.notes)) {
    throw new Error('Unsupported strategy notes format');
  }
  const notes = Object.create(null);
  for (const [key, value] of Object.entries(data.notes)) {
    if (typeof value !== 'string') throw new Error('Invalid strategy note');
    notes[key] = value;
  }
  return notes;
}

export function readStrategyNotes(storage) {
  try { return readNotes(storage || globalThis.localStorage); }
  catch { return Object.create(null); }
}

export function getStrategyNote(name, notes = readStrategyNotes()) {
  return notes[noteKey(name)] || '';
}

export function saveStrategyNote(name, value, storage) {
  const key = noteKey(name);
  const note = String(value).replace(/\s+/g, ' ').trim();
  if (!key || note.length > MAX_STRATEGY_NOTE_LENGTH) throw new Error('Invalid strategy note');
  const target = storage || globalThis.localStorage;
  // Refuse to overwrite unreadable data; do not silently discard other notes.
  const notes = readNotes(target);
  if (note) notes[key] = note;
  else delete notes[key];
  target.setItem(NOTES_KEY, JSON.stringify({ version: 1, notes }));
}
