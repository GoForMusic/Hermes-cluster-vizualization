// A search box and pages for a list the admin pages show: everything is in memory (an admin page lists hundreds, not millions), so the
// filter and the pages are made here and the hub is asked for nothing more.
import { useEffect, useMemo, useState } from 'react';

export const PAGE_SIZE = 10;

export interface Paged<T> {
  query: string;
  setQuery: (q: string) => void;
  page: number;
  setPage: (p: number) => void;
  pages: number;
  /** what matches the search, all pages */
  total: number;
  /** the rows of this page */
  shown: T[];
  first: number;
}

/** `matches(item, text)` gets the search text lowercased and trimmed; an empty search matches everything. */
export function usePaged<T>(items: readonly T[], matches: (item: T, text: string) => boolean, size = PAGE_SIZE): Paged<T> {
  const [query, setQuery] = useState('');
  const [page, setPage] = useState(0);
  const text = query.trim().toLowerCase();
  const found = useMemo(() => (text ? items.filter((i) => matches(i, text)) : items), [items, text, matches]);
  const pages = Math.max(1, Math.ceil(found.length / size));
  const at = Math.min(page, pages - 1); // a page that no longer exists (after a delete, or a longer search) falls back to the last one
  useEffect(() => setPage(0), [text]);
  return { query, setQuery, page: at, setPage, pages, total: found.length, shown: found.slice(at * size, at * size + size), first: at * size };
}

/** The search field, and under it where you are in the list. */
export function ListBar<T>({ paged, label, placeholder }: { paged: Paged<T>; label: string; placeholder: string }) {
  const { query, setQuery, page, setPage, pages, total, shown, first } = paged;
  return (
    <div className="list-bar">
      <input type="search" aria-label={label} placeholder={placeholder} value={query} onChange={(e) => setQuery(e.target.value)} />
      <span className="muted">{total ? `${first + 1}–${first + shown.length} of ${total}` : 'nothing found'}</span>
      <button type="button" className="btn xs" disabled={page === 0} onClick={() => setPage(page - 1)}>← Previous</button>
      <button type="button" className="btn xs" disabled={page >= pages - 1} onClick={() => setPage(page + 1)}>Next →</button>
    </div>
  );
}
