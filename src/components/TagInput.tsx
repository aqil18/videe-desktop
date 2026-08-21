import { useState } from "react";
import { colorForLabel, withAlpha } from "../lib/color";

interface TagInputProps {
  tags: string[];
  onChange: (tags: string[]) => void;
}

export function TagInput({ tags, onChange }: TagInputProps) {
  const [draft, setDraft] = useState("");

  function commitDraft() {
    const value = draft.trim();
    setDraft("");
    if (value && !tags.includes(value)) {
      onChange([...tags, value]);
    }
  }

  function removeTag(tag: string) {
    onChange(tags.filter((t) => t !== tag));
  }

  return (
    <div className="flex flex-wrap items-center gap-1.5 rounded-md border border-neutral-800 bg-neutral-900 p-2">
      {tags.map((tag) => {
        const color = colorForLabel(tag);
        return (
          <span
            key={tag}
            className="flex items-center gap-1.5 rounded-full px-2 py-0.5 text-xs text-neutral-200"
            style={{ backgroundColor: withAlpha(color, 0.15), border: `1px solid ${withAlpha(color, 0.45)}` }}
          >
            <span className="h-1.5 w-1.5 shrink-0 rounded-full" style={{ backgroundColor: color }} />
            {tag}
            <button
              onClick={() => removeTag(tag)}
              aria-label={`Remove tag ${tag}`}
              className="text-neutral-500 transition hover:text-neutral-200"
            >
              ×
            </button>
          </span>
        );
      })}
      <input
        value={draft}
        onChange={(e) => setDraft(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === ",") {
            e.preventDefault();
            commitDraft();
          } else if (e.key === "Backspace" && draft === "" && tags.length > 0) {
            removeTag(tags[tags.length - 1]);
          }
        }}
        onBlur={commitDraft}
        placeholder="Add tag…"
        className="min-w-[80px] flex-1 bg-transparent text-xs text-neutral-100 outline-none placeholder:text-neutral-600"
      />
    </div>
  );
}
