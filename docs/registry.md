# The registry

Themes, templates, keymaps, engines and setups that people share. Each entry is one small file in the vigent repository, checked by vigent's own loader before it's listed, and `vigent add` shows you that file, and everything it runs, before it changes anything. Browse them at [vigent.farming-labs.dev/registry](https://vigent.farming-labs.dev/registry).

| Kind | What it is | Where `vigent add` puts it |
|---|---|---|
| `theme` | a theme file: extends a built-in theme, with its own `[highlights]` and `[style]` ([Themes](themes.md#writing-your-own-theme)) | `~/.config/vigent/themes/<name>.toml` |
| `template` | a prompt to start agents with: `[templates.<name>]` ([templates](config.md#templates)) | merged into `~/.config/vigent/config.toml` |
| `keymap` | key bindings: `[keys.normal]` and the other modes ([keys](config.md#keys)) | merged into `~/.config/vigent/config.toml` |
| `engine` | another agent CLI: `[engines.<name>]` with a command ([your own engines](config.md#your-own-engines)) | merged into `~/.config/vigent/config.toml` |
| `setup` | a whole [`.vigent.toml`](config.md#a-projects-own-config-vigenttoml) for a stack: hooks, commands, permissions | `.vigent.toml` at the repository's root |

`~/.config/vigent` is `$XDG_CONFIG_HOME/vigent` when that's set, as everywhere else in vigent.

## Using it

```bash
vigent add --list                  # every entry, by kind
vigent add --list engines          # one kind
vigent search review               # entries with these words in their name, description or tags
vigent add harbor                  # show the file and what it runs, ask, then add it
vigent add theme/harbor            # the same, with its kind: for when two kinds share a name
vigent add fix-ci --yes            # don't ask
vigent add fix-ci --force          # replace what's there
```

`vigent add <name>` prints the entry (its kind, author, license and description), where it goes, and the file exactly as it will be written. For an entry that runs anything, it then lists every command and every rule that lets Claude Code run a command without asking:

```
setup/pnpm-monorepo by vigent (MIT)
installs each new worktree, tests before merging, lets Claude Code run pnpm test

It writes /home/you/shop/.vigent.toml:

    [hooks]
    worktree_created = "pnpm install --frozen-lockfile"
    ...

It runs:
    $ pnpm install --frozen-lockfile    # hooks.worktree_created
    $ pnpm test    # hooks.before_merge
    $ pnpm test {args}    # commands.Test
It lets Claude Code run, without asking:
    Bash(pnpm test:*)    # engines.claude.allow
    Bash(pnpm lint:*)    # engines.claude.allow

Add it? [y/N]
```

Anything but `y` (or `yes`) adds nothing. Then:

- **A theme** is written to your themes folder. Use it with `:colorscheme <name>`, or `theme = "<name>"` in config.toml. A theme that's already built in has nothing to add: `vigent add nord` says to use `:colorscheme nord`.
- **A template, keymap or engine** goes into config.toml beside what's there. A table you already have (`[keys.normal]`, say) gets the entry's keys at its end; a new one goes at the end of the file. Your comments, order and other settings stay as you wrote them, and the result is read back before it's saved: if it wouldn't read as your settings plus the entry's (a table written inline, for instance), nothing changes and vigent says so. vigent reads it when it starts.
- **A setup** is written as `.vigent.toml` at the root of the repository you're in (from any folder inside it). Its commands don't run until you trust it with `:trust` in vigent, or `vigent trust`.

`vigent add` refuses, and says why, rather than overwrite anything: a theme file or `.vigent.toml` that exists, or a key config.toml already sets (`config.toml already has [keys.normal] "<leader>an" (--force replaces it)`). With `--force` it replaces exactly those, and lists them before it asks. These checks happen before the question, so you never approve something that then fails.

An unknown name gets the closest ones (`no 'harbr' in the registry (did you mean harbor?)`).

### Where the index comes from

`vigent add` and `vigent search` read one file, `https://vigent.farming-labs.dev/registry/index.json`. The npm vigent fetches it with Node; the standalone binary has no HTTP code of its own and runs `curl -fsSL` (Windows 10 and later ship `curl.exe`), or `wget -qO-` without curl. When it can't be fetched, vigent says so and changes nothing.

`VIGENT_REGISTRY` points at another index: a URL, or a local `index.json` (or the folder it's in). Use it offline, or to try entries before they're published:

```bash
npm run registry                                   # in a vigent checkout: writes website/public/registry/index.json
VIGENT_REGISTRY=website/public/registry node dist/cli.js add my-theme
```

## The safety model

- **You read it before it's added.** What `vigent add` prints is the file it writes, byte for byte, from the same index entry.
- **What runs is read from the file, not written by its author.** `npm run registry` loads each entry the way vigent's trust check does and lists every part vigent holds back from an untrusted `.vigent.toml`: hooks, `[commands]` with a shell step, an engine's `command` (with its `model_args` and `resume_args`), `args` for a built-in engine, and Claude Code `allow` rules. The website's cards and `vigent add` show that list; an entry can't hide a command from it.
- **Themes, templates and keymaps are data.** A theme only extends a built-in theme and sets colors and layout. A template is a prompt. A keymap maps keys to actions and ex commands, and may not define commands (`:command`), so it can't carry a shell step.
- **An engine runs a program you name.** It goes into your own config.toml, which vigent trusts, so the question is the check: read the command.
- **A setup waits for `:trust`.** It's a project file like any other `.vigent.toml`: what it runs only counts once you trust it, and trust is for that exact content.
- **Every entry is reviewed and checked.** Entries arrive as pull requests to the vigent repository. CI loads each one with vigent's own code and rejects it with any warning vigent would show.

## Submitting an entry

Open a pull request to [farming-labs/vigent](https://github.com/farming-labs/vigent) with one folder:

```
registry/<kind>/<name>/entry.toml   the file people add
registry/<kind>/<name>/meta.toml    description, author, license, tags
```

- `<kind>` is `theme`, `template`, `keymap`, `engine` or `setup`.
- `<name>` is lowercase letters and digits joined by `-` (`fix-ci`, `pnpm-monorepo`), at most 40 characters, and not a built-in theme's name. For a template or engine, it's also the table's name: `[templates.fix-ci]`, `[engines.aider]`.
- `entry.toml` holds only what the kind adds: a theme file (`extends` a built-in theme, colors as `"#rrggbb"` or `"NONE"`); one `[templates.<name>]`; `[keys.*]` tables; one `[engines.<name>]` with a `command`; or anything a `.vigent.toml` can hold.
- `meta.toml`:

  ```toml
  description = "one line on what it does"   # at most 120 characters
  author = "your name or GitHub handle"
  license = "MIT"                              # an SPDX id
  tags = ["dark", "layout"]                    # up to 6 short lowercase words
  ```

Then, in a vigent checkout:

```bash
npm ci
npm run registry          # check every entry; write website/public/registry/index.json
VIGENT_REGISTRY=website/public/registry node dist/cli.js add <name>   # try it
```

`npm run registry` lists every problem it finds: an unknown kind, a bad name, missing or unknown meta, a file that doesn't load, or any warning vigent would show for it. Commit the index it writes with your entry; `npm run registry:check` (in CI and `npm test`) fails when it's out of date. Once it's merged, the entry is on the website, and in `vigent add`, after the next deploy. [registry/README.md](../registry/README.md) has the same steps for contributors.
