"use client";

import { type ReactNode, useMemo, useState } from "react";
import { type Item, type Kind, ITEMS, KINDS } from "../lib/registry";
import { vars } from "./theme-showcase";

type Part = string | [cls: string, text: string];
const SIDE = 15;
/** a few rows of vigent, small enough for a card */
const ROWS: [Part[], Part[]][] = [
  [[["sb", " SESSIONS"]], ["▸ fix-flaky-test"]],
  [["  ", ["sp", "●"], " auth"], [["user", "❯"], " fails 1 run in 10"]],
  [["  ", ["sp", "●"], " flaky-test"], [["cm", "▸ "], ["tool", "Edit"], ["cm", " checkout.test.ts"]]],
  [["  ", ["cm", "fix/flaky +6"]], ["50 of 50 passed. ", ["link", ":diff"]]],
];
const plain = (parts: Part[]) => parts.map((p) => (typeof p === "string" ? p : p[1])).join("");
const paint = (parts: Part[], key: string): ReactNode[] =>
  parts.map((p, i) =>
    typeof p === "string" ? (
      p
    ) : (
      <span className={p[0]} key={`${key}${i}`}>
        {p[1]}
      </span>
    ),
  );

function Preview({ item }: { item: Item }) {
  const t = item.colors!;
  return (
    <div className={`screen mini${t.transparent ? " transparent" : ""}`} style={vars(t)} aria-hidden="true">
      <pre>
        {ROWS.map(([side, main], r) => (
          <span key={r}>
            {paint(side, `s${r}`)}
            {" ".repeat(Math.max(0, SIDE - plain(side).length))}
            <span className="sep">│</span> {paint(main, `m${r}`)}
            {"\n"}
          </span>
        ))}
        <span className="st">
          <span className="mode"> NORMAL </span>
          <span>fix/flaky-test</span>
          <span className="fill" />
          <span>12.4k </span>
        </span>
      </pre>
    </div>
  );
}

function Copy({ text }: { text: string }) {
  const [done, setDone] = useState(false);
  return (
    <button
      type="button"
      className={`yank rcopy${done ? " done" : ""}`}
      aria-label={`Copy ${text}`}
      onClick={() => {
        navigator.clipboard?.writeText(text).then(
          () => {
            setDone(true);
            setTimeout(() => setDone(false), 1400);
          },
          () => {},
        );
      }}
    >
      {done ? "copied" : "copy"}
    </button>
  );
}

function Card({ item }: { item: Item }) {
  const add = item.builtin ? `:colorscheme ${item.name}` : `vigent add ${item.name}`;
  const runs = item.runs?.length ? item.runs : undefined;
  return (
    <article className="rcard" aria-label={`${item.name}, ${item.kind}`}>
      {item.colors && <Preview item={item} />}
      <header className="rhead">
        <h3 className="b">{item.name}</h3>
        <span className="rkind">{item.kind}</span>
      </header>
      <p className="rdesc">{item.description}</p>
      <div className="rmeta">
        <span className={item.builtin ? "badge" : runs ? "badge warn" : "badge ok"}>{item.builtin ? "built in" : runs ? "runs commands" : "data only"}</span>
        <span className="cm">by {item.author}</span>
        {item.tags.map((t) => (
          <span className="cm" key={t}>
            · {t}
          </span>
        ))}
      </div>
      {runs && (
        <ul className="rruns" aria-label="What it runs">
          {runs.map((r, i) => (
            <li key={`${r.from}${i}`} title={r.from}>
              {r.allow ? (
                <>
                  <span className="cm">allows Claude Code:</span> {r.allow}
                </>
              ) : (
                <>
                  <span className="cm">$</span> {r.command}
                </>
              )}
            </li>
          ))}
        </ul>
      )}
      {item.file &&
        (item.kind === "theme" ? (
          <details className="rfile">
            <summary>read the file</summary>
            <pre className="code">{item.file}</pre>
          </details>
        ) : (
          <pre className="code rfile-open">{item.file}</pre>
        ))}
      <div className="radd">
        <code>{add}</code>
        <Copy text={add} />
      </div>
    </article>
  );
}

/** The store: filter by kind or by text, then a grid of entries. */
export function RegistryStore() {
  const [kind, setKind] = useState<Kind | "all">("all");
  const [q, setQ] = useState("");
  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return ITEMS.filter((i) => (kind === "all" || i.kind === kind) && (!needle || `${i.name} ${i.description} ${i.tags.join(" ")}`.toLowerCase().includes(needle)));
  }, [kind, q]);
  const count = (k: Kind | "all") => (k === "all" ? ITEMS.length : ITEMS.filter((i) => i.kind === k).length);

  return (
    <div className="t store">
      <div className="rbar">
        <div className="rkinds" role="group" aria-label="Show">
          {(["all", ...KINDS.map((k) => k.id)] as const).map((k) => (
            <button type="button" key={k} aria-pressed={kind === k} onClick={() => setKind(k)}>
              {k === "all" ? "all" : KINDS.find((x) => x.id === k)!.plural}
              <span className="n">{count(k)}</span>
            </button>
          ))}
        </div>
        <label className="rsearch">
          <span className="cm">/</span>
          <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="filter: gruvbox, pnpm, review…" aria-label="Filter entries" />
        </label>
      </div>
      {kind !== "all" && <p className="cm rwhat">{KINDS.find((k) => k.id === kind)!.what}</p>}
      {shown.length ? (
        <div className="rgrid">
          {shown.map((i) => (
            <Card item={i} key={`${i.kind}/${i.name}`} />
          ))}
        </div>
      ) : (
        <p className="rempty">
          <span className="cm">E486: Pattern not found:</span> {q}
        </p>
      )}
    </div>
  );
}
