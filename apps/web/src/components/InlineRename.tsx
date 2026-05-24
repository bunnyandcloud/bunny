import { useEffect, useRef, useState, type KeyboardEvent } from 'react';

interface Props {
  value: string;
  onSave: (next: string) => Promise<void>;
  className?: string;
  inputClassName?: string;
  title?: string;
  disabled?: boolean;
}

export default function InlineRename({
  value,
  onSave,
  className = '',
  inputClassName = '',
  title,
  disabled,
}: Props) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);
  const [saving, setSaving] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!editing) setDraft(value);
  }, [value, editing]);

  useEffect(() => {
    if (editing) inputRef.current?.focus();
  }, [editing]);

  async function commit() {
    const next = draft.trim();
    if (!next || next === value) {
      setDraft(value);
      setEditing(false);
      return;
    }
    setSaving(true);
    try {
      await onSave(next);
      setEditing(false);
    } catch {
      setDraft(value);
      setEditing(false);
    } finally {
      setSaving(false);
    }
  }

  function onKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === 'Enter') {
      e.preventDefault();
      void commit();
    } else if (e.key === 'Escape') {
      setDraft(value);
      setEditing(false);
    }
  }

  if (editing) {
    return (
      <input
        ref={inputRef}
        type="text"
        value={draft}
        maxLength={64}
        disabled={saving}
        className={`bg-bunny-bg border border-bunny-accent rounded px-1 py-0.5 text-inherit font-inherit min-w-[4rem] ${inputClassName}`}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => void commit()}
        onKeyDown={onKeyDown}
      />
    );
  }

  return (
    <span
      role="button"
      tabIndex={0}
      title={title ?? 'Double-click to rename'}
      className={`inline-block text-left truncate hover:underline ${disabled || saving ? 'opacity-50' : ''} ${className}`}
      onDoubleClick={(e) => {
        e.preventDefault();
        e.stopPropagation();
        if (!disabled) setEditing(true);
      }}
      onKeyDown={(e) => {
        if (e.key === 'F2' && !disabled) {
          e.preventDefault();
          e.stopPropagation();
          setEditing(true);
        }
      }}
    >
      {value}
    </span>
  );
}
