import { useEffect, useState, type InputHTMLAttributes } from 'react';

/**
 * A text or number field that applies what was typed when the person leaves it or presses Enter, not on every keystroke: what it changes is
 * saved on the hub, and half-typed numbers are not settings.
 */
export function CommitInput({ value, onCommit, ...rest }: { value: string | number; onCommit: (value: string) => void } & Omit<InputHTMLAttributes<HTMLInputElement>, 'value' | 'onChange' | 'onBlur' | 'onKeyDown'>) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);
  const commit = () => { if (draft !== String(value)) onCommit(draft); };
  return <input {...rest} value={draft} onChange={(e) => setDraft(e.target.value)} onBlur={commit} onKeyDown={(e) => { if (e.key === 'Enter') e.currentTarget.blur(); }} />;
}

/** A short list of strings (namespaces, labels, …) as removable chips, with a field to add one — Enter or `,` adds it, so pasting a
 * comma-separated list still works one at a time. */
export function TagList({ value, onChange, placeholder }: { value: readonly string[]; onChange: (value: string[]) => void; placeholder?: string }) {
  const [draft, setDraft] = useState('');
  const add = () => {
    const v = draft.trim();
    if (v && !value.includes(v)) onChange([...value, v]);
    setDraft('');
  };
  return (
    <div className="tag-list">
      <div className="tags">
        {value.map((v) => (
          <span key={v} className="tag">
            {v}
            <button type="button" aria-label={`Remove ${v}`} onClick={() => onChange(value.filter((x) => x !== v))}>×</button>
          </span>
        ))}
      </div>
      <input
        type="text"
        placeholder={placeholder}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={add}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ',') { e.preventDefault(); add(); }
          if (e.key === 'Backspace' && !draft && value.length) onChange(value.slice(0, -1));
        }}
      />
    </div>
  );
}
