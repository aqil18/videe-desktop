import { colorForLabel, withAlpha } from "../lib/color";
import { TagTemplates } from "./TagTemplates";

interface FilterBarProps {
  searchQuery: string;
  onSearchQueryChange: (value: string) => void;
  allTags: string[];
  activeTags: Set<string>;
  onToggleTag: (tag: string) => void;
  projectTags: string[];
  onProjectTagsChange: (tags: string[]) => void;
}

export function FilterBar({
  searchQuery,
  onSearchQueryChange,
  allTags,
  activeTags,
  onToggleTag,
  projectTags,
  onProjectTagsChange,
}: FilterBarProps) {
  const projectTagSet = new Set(projectTags);

  return (
    <div className="flex flex-wrap items-center gap-3 border-b border-neutral-800 px-4 py-2">
      <input
        value={searchQuery}
        onChange={(e) => onSearchQueryChange(e.currentTarget.value)}
        placeholder="Search filename or tag…"
        className="w-56 rounded-md border border-neutral-800 bg-neutral-900 px-2.5 py-1 text-xs text-neutral-100 outline-none placeholder:text-neutral-600 focus:border-neutral-600"
      />
      <TagTemplates tags={projectTags} onChange={onProjectTagsChange} />
      {allTags.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5">
          {allTags.map((tag) => {
            const active = activeTags.has(tag);
            const color = colorForLabel(tag);
            const isProjectTag = projectTagSet.has(tag);
            return (
              <span
                key={tag}
                className={`flex items-center rounded-full text-xs transition ${
                  active
                    ? "bg-neutral-100 text-neutral-900"
                    : "bg-neutral-800 text-neutral-300 hover:bg-neutral-700"
                }`}
                style={{ border: `1px solid ${withAlpha(color, active ? 0.7 : 0.45)}` }}
              >
                <button
                  onClick={() => onToggleTag(tag)}
                  className={`flex items-center gap-1.5 py-0.5 pl-2.5 ${isProjectTag ? "pr-1" : "pr-2.5"}`}
                >
                  <span className="h-1.5 w-1.5 shrink-0 rounded-full" style={{ backgroundColor: color }} />
                  {tag}
                </button>
                {isProjectTag && (
                  <button
                    onClick={() => onProjectTagsChange(projectTags.filter((t) => t !== tag))}
                    aria-label={`Remove ${tag} from project`}
                    title="Remove from project"
                    className="rounded-r-full py-0.5 pl-1 pr-2 opacity-0 transition hover:opacity-100 focus-visible:opacity-100"
                  >
                    ×
                  </button>
                )}
              </span>
            );
          })}
        </div>
      )}
    </div>
  );
}
