interface FilterBarProps {
  searchQuery: string;
  onSearchQueryChange: (value: string) => void;
  allTags: string[];
  activeTags: Set<string>;
  onToggleTag: (tag: string) => void;
}

export function FilterBar({
  searchQuery,
  onSearchQueryChange,
  allTags,
  activeTags,
  onToggleTag,
}: FilterBarProps) {
  return (
    <div className="flex flex-wrap items-center gap-3 border-b border-neutral-800 px-4 py-2">
      <input
        value={searchQuery}
        onChange={(e) => onSearchQueryChange(e.currentTarget.value)}
        placeholder="Search filename or tag…"
        className="w-56 rounded-md border border-neutral-800 bg-neutral-900 px-2.5 py-1 text-xs text-neutral-100 outline-none placeholder:text-neutral-600 focus:border-neutral-600"
      />
      {allTags.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5">
          {allTags.map((tag) => {
            const active = activeTags.has(tag);
            return (
              <button
                key={tag}
                onClick={() => onToggleTag(tag)}
                className={`rounded-full px-2.5 py-0.5 text-xs transition ${
                  active
                    ? "bg-neutral-100 text-neutral-900"
                    : "bg-neutral-800 text-neutral-300 hover:bg-neutral-700"
                }`}
              >
                {tag}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
