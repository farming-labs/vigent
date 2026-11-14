# The vigent registry

Themes, templates, keymaps, engines and setups people share. Each folder here is one entry, and people add it with `vigent add <name>`, after vigent has shown them the file and everything it runs. The website lists them at [vigent.farming-labs.dev/registry](https://vigent.farming-labs.dev/registry).

## Adding an entry

1. Make one folder, `registry/<kind>/<name>/`, with two files:

   ```
   entry.toml   the file people add
   meta.toml    description, author, license, tags
   ```

   `<kind>` is `theme`, `template`, `keymap`, `engine` or `setup`. `<name>` is lowercase letters and digits joined by `-`, at most 40 characters, and not a built-in theme's name.

2. Write `entry.toml`, which holds only what its kind adds:

   | Kind | entry.toml | Example |
   |---|---|---|
   | `theme` | a theme file: `extends` a built-in theme, `[highlights]`, `[style]`; colors are `"#rrggbb"` or `"NONE"` | [theme/harbor](theme/harbor/entry.toml) |
   | `template` | one `[templates.<name>]`, named like the folder | [template/fix-ci](template/fix-ci/entry.toml) |
   | `keymap` | `[keys.normal]` and the other modes; no `:command` | [keymap/leader-land](keymap/leader-land/entry.toml) |
   | `engine` | one `[engines.<name>]` with a `command`, named like the folder | [engine/aider](engine/aider/entry.toml) |
   | `setup` | anything a `.vigent.toml` holds | [setup/pnpm-monorepo](setup/pnpm-monorepo/entry.toml) |

3. Write `meta.toml`:

   ```toml
   description = "one line on what it does"   # at most 120 characters
   author = "your name or GitHub handle"
   license = "MIT"                              # an SPDX id
   tags = ["dark", "layout"]                    # up to 6 short lowercase words
   ```

4. Check it and write the index:

   ```bash
   npm ci
   npm run registry
   ```

   It loads every entry with vigent's own code and lists every problem: an unknown kind, a bad name, missing or unknown meta, a file that doesn't load, or any warning vigent would show. With none, it writes `website/public/registry/index.json`, the index the website and `vigent add` read, including what each entry runs, taken from the file.

5. Try it, then open a pull request with your folder and the index:

   ```bash
   VIGENT_REGISTRY=website/public/registry node dist/cli.js add <name>
   ```

CI runs `npm run registry:check`, which fails when an entry doesn't load or the index is out of date. [docs/registry.md](../docs/registry.md) has how `vigent add` works and its safety model.
