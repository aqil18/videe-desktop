import { useEffect, useRef, useState } from "react";

const TAG_TEMPLATES = [
  { name: "Types of shots", tags: ["Cinematic", "Opening", "Wide", "Closing", "Over the shoulder"] },
  { name: "Good or bad shots", tags: ["Good shot", "Bad shot", "Needs review"] },
];

interface TagTemplatesProps {
  /** The project's tag list -- templates add to this, not to a clip. */
  tags: string[];
  onChange: (tags: string[]) => void;
}

export function TagTemplates({ tags, onChange }: TagTemplatesProps) {
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

  // Case-insensitive so an existing "cinematic" isn't duplicated by "Cinematic".
  const existing = new Set(tags.map((t) => t.toLowerCase()));

  return (
    <div ref={containerRef} className="relative">
      <button
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        className="flex items-center gap-1.5 rounded-md border border-neutral-800 bg-neutral-900 px-2.5 py-1 text-xs text-neutral-300 transition hover:border-neutral-600 hover:text-neutral-100"
      >
        Tag Templates
        <span aria-hidden className={`transition-transform ${open ? "rotate-90" : ""}`}>
          ›
        </span>
      </button>

      {open && (
        <ul className="absolute left-0 top-full z-20 mt-1 flex w-48 flex-col gap-0.5 rounded-lg border border-neutral-800 bg-neutral-900 p-1 shadow-xl">
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
