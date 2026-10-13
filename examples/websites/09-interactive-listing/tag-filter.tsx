// Filters the listing's posts by tag. Receives the rows the lmd cell
// built as `props.posts` and `props.tags`; with `content: none` the
// static year-by-year list is replaced by this component once it mounts
// (and stays in place when JavaScript is off).
import { useMemo, useState } from "react";
import type { QuartoComponentProps } from "@quarto/components";

interface Post {
  title: string;
  href: string;
  date: string;
  year: string;
  tags: string[];
  description: string;
}

interface Props {
  posts: Post[];
  tags: { name: string; count: number }[];
}

export default function TagFilter({ props }: QuartoComponentProps<Props>) {
  const [selected, setSelected] = useState<string[]>([]);

  const toggle = (tag: string) =>
    setSelected((s) => (s.includes(tag) ? s.filter((t) => t !== tag) : [...s, tag]));

  const posts = useMemo(
    () => props.posts.filter((p) => selected.every((t) => p.tags.includes(t))),
    [props.posts, selected],
  );

  return (
    <div className="tag-filter">
      <p className="tag-filter-bar">
        {props.tags.map(({ name, count }) => {
          const on = selected.includes(name);
          return (
            <button
              key={name}
              type="button"
              className={on ? "btn btn-sm btn-primary" : "btn btn-sm btn-outline-secondary"}
              aria-pressed={on}
              onClick={() => toggle(name)}
            >
              {name} <span className="badge text-bg-light">{count}</span>
            </button>
          );
        })}
        {selected.length > 0 && (
          <button type="button" className="btn btn-sm btn-link" onClick={() => setSelected([])}>
            clear
          </button>
        )}
      </p>
      <p className="tag-filter-count">
        {posts.length} of {props.posts.length} posts
        {selected.length > 0 ? ` tagged ${selected.join(" and ")}` : ""}
      </p>
      <ul className="tag-filter-posts">
        {posts.map((p) => (
          <li key={p.href}>
            <a href={p.href}>{p.title}</a> <small>({p.date})</small>
            {p.description ? <div className="text-muted">{p.description}</div> : null}
          </li>
        ))}
      </ul>
    </div>
  );
}
