// Choose some of many things: what is chosen shows as chips (× takes one away), and a search box finds what is left to add. A list of
// checkboxes stops working at a few dozen; this works for any number.
import { useId, useMemo, useState, type ReactNode } from 'react';

export interface PickItem {
  id: number;
  name: string;
  /** shown under the name in the results */
  hint?: string;
}

export function Picker({ label, icon, items, value, onChange, placeholder, empty }: {
  label: string;
  icon?: ReactNode;
  items: readonly PickItem[];
  value: readonly number[];
  onChange: (value: number[]) => void;
  placeholder: string;
  /** what is shown when there is nothing at all to choose from */
  empty: string;
}) {
  const [query, setQuery] = useState('');
  const [focused, setFocused] = useState(false);
  const listId = useId();
  const byId = useMemo(() => new Map(items.map((i) => [i.id, i])), [items]);
  const text = query.trim().toLowerCase();
  const left = items.filter((i) => !value.includes(i.id) && (!text || `${i.name} ${i.hint ?? ''}`.toLowerCase().includes(text)));
  const SHOWN = 6;
  const add = (id: number) => { onChange([...value, id]); setQuery(''); };
  return (
    <div className="picker">
      <span className="picker-label">{label}</span>
      <div className="picker-chips" aria-label={`${label}: chosen`}>
        {value.length ? value.map((id) => (
          <span key={id} className="pick-chip">
            {icon}{byId.get(id)?.name ?? '?'}
            <button type="button" aria-label={`Remove ${byId.get(id)?.name ?? id}`} onClick={() => onChange(value.filter((v) => v !== id))}>×</button>
          </span>
        )) : <span className="muted">None yet</span>}
      </div>
      {items.length ? (
        <>
          <input
            type="search"
            role="combobox"
            aria-expanded={focused || !!text}
            aria-controls={listId}
            aria-label={`Search ${label.toLowerCase()} to add`}
            placeholder={placeholder}
            value={query}
            autoComplete="off"
            onChange={(e) => setQuery(e.target.value)}
            onFocus={() => setFocused(true)}
            onBlur={() => setFocused(false)}
            onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); if (left[0]) add(left[0].id); } }}
          />
          {focused || text ? (
            <ul className="picker-list" id={listId} role="listbox" aria-label={`${label} that can be added`}>
              {left.slice(0, SHOWN).map((i) => (
                // mouse down, not click: the search box would lose focus first and close the list before the click arrived
                <li key={i.id} role="option" aria-selected={false} onMouseDown={(e) => { e.preventDefault(); add(i.id); }}>
                  <span className="plus" aria-hidden="true">+</span>
                  <span><b>{i.name}</b>{i.hint ? <span className="sub"> {i.hint}</span> : null}</span>
                </li>
              ))}
              {left.length > SHOWN ? <li className="more">{left.length - SHOWN} more: keep typing to narrow it down</li> : null}
              {!left.length ? <li className="more">{text ? 'Nothing matches' : 'Everything is added'}</li> : null}
            </ul>
          ) : null}
        </>
      ) : <span className="muted">{empty}</span>}
    </div>
  );
}
