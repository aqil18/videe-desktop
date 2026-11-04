import { useEffect, useRef, useState } from "react";

const TAG_TEMPLATES = [
  { name: "Types of shots", tags: ["Cinematic", "Opening", "Wide", "Closing", "Over the shoulder"] },
  { name: "Good or bad shots", tags: ["Good shot", "Bad shot", "Needs review"] },
];

interface TagTemplatesProps {
  tags: string[];
  onChange: (tags: string[]) => void;
  disabled?: boolean;
}

export function TagTemplates({ tags, onChange, disabled = false }: TagTemplatesProps) {
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function onPointerDown(e: PointerEvent) {
      if (!containerRef.current?.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [open]);

  // Case-insensitive so a hand-typed "cinematic" isn't duplicated by "Cinematic".
  const existing = new Set(tags.map((t) => t.toLowerCase()));

  return (
    <div ref={containerRef} className="relative">
      <button
        onClick={() => setOpen((o) => !o)}
        disabled={disabled}
        aria-expanded={open}
        title={disabled ? "Select a clip to add tag templates" : undefined}
        className="flex items-center gap-1.5 rounded-md bg-neutral-800 px-3 py-1.5 text-sm text-neutral-100 transition hover:bg-neutral-700 disabled:opacity-50 disabled:hover:bg-neutral-800"
      >
        Tag Templates
        <span aria-hidden className={`transition-transform ${open ? "rotate-90" : ""}`}>
          ›
        </span>
      </button>

      {open && (
        <ul className="absolute right-0 top-full z-20 mt-1 flex w-48 flex-col gap-0.5 rounded-lg border border-neutral-800 bg-neutral-900 p-1 shadow-xl">
          {TAG_TEMPLATES.map((template) => {
            const missing = template.tags.filter((t) => !existing.has(t.toLowerCase()));
            return (
              <li key={template.name}>
                <button
                  onClick={() => onChange([...tags, ...missing])}
                  disabled={missing.length === 0}
                  title={template.tags.join(", ")}
                  className="w-full rounded px-2 py-1 text-left text-xs text-neutral-300 transition hover:bg-neutral-800 disabled:cursor-default disabled:text-neutral-600 disabled:hover:bg-transparent"
                >
                  + {template.name}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
