import type { Metadata } from "@farm.js/core";
import { Heading, L, Rule, Tilde } from "../../components/buffer";
import { RegistryStore } from "../../components/registry-store";
import { Tabline } from "../../components/tabline";
import { VimChrome } from "../../components/vim-chrome";

// nothing here reads the request, so it renders once at build time
export const dynamic = "force-static";

export const metadata: Metadata = {
  title: "vigent registry",
  description: "Themes, templates, keymaps, engines and setups for vigent. Each one is a small file you can read before you add it.",
};

export default function RegistryPage() {
  return (
    <>
      <Tabline active="registry" />
      <main className="buffer" id="top">
        <L className="row">
          <span>
            <span className="tag">*registry.txt*</span> <span className="cm">themes, prompts, keys and recipes for vigent</span>
          </span>
        </L>
        <section className="hero rhero" aria-label="registry">
          <div className="hero-copy">
            <L className="eyebrow">registry</L>
            <div className="l">
              <h1 className="t lede">Make vigent yours. Read every file before you add it.</h1>
            </div>
            <div className="l">
              <p className="t sub">
                Each entry is one small file in the vigent repo, checked by vigent's own loader before it's listed. Themes, templates and keymaps are only data. Engines and setups run commands, so the card lists every one, <span className="kw">vigent add</span> shows them
                before it asks, and a setup's wait for <span className="kw">:trust</span>.
              </p>
            </div>
          </div>
          <L className="install">
            <code>vigent add &lt;name&gt;</code>
            <span className="cm">shows the file and asks first</span>
          </L>
        </section>
        <div className="l wide store-line">
          <RegistryStore />
        </div>
        <L />
        <Rule />
        <Heading id="submit" title="Add yours" tag="registry-submit" />
        <L />
        <L>Open a pull request with one folder:</L>
        <L className="indent">
          <span className="str">registry/&lt;kind&gt;/&lt;name&gt;/entry.toml</span> <span className="cm">the file people add</span>
        </L>
        <L className="indent">
          <span className="str">registry/&lt;kind&gt;/&lt;name&gt;/meta.toml</span> <span className="cm">description, author, license, tags</span>
        </L>
        <L />
        <L>The kind is theme, template, keymap, engine or setup, and the name is lowercase words joined by -. entry.toml is what vigent writes:</L>
        <L className="indent">
          <span className="kw">theme</span> <span className="cm">a theme file, as in ~/.config/vigent/themes/&lt;name&gt;.toml: extends a built-in, [highlights], [style]</span>
        </L>
        <L className="indent">
          <span className="kw">template</span> <span className="cm">one [templates.&lt;name&gt;], merged into config.toml</span>
        </L>
        <L className="indent">
          <span className="kw">keymap</span> <span className="cm">[keys.normal] and the other modes, merged into config.toml</span>
        </L>
        <L className="indent">
          <span className="kw">engine</span> <span className="cm">one [engines.&lt;name&gt;] with a command, merged into config.toml</span>
        </L>
        <L className="indent">
          <span className="kw">setup</span> <span className="cm">a whole .vigent.toml, written at the repository's root</span>
        </L>
        <L />
        <L>
          Then run <span className="kw">npm run registry</span>: it loads every entry with vigent's own code, lists every problem (an unknown kind, a bad name, missing meta, a file that doesn't load, any warning), and writes the index this page and{" "}
          <span className="kw">vigent add</span> read. Commit the index with your entry; CI runs <span className="kw">npm run registry:check</span>. What an entry runs is read from the file, never written by hand.
        </L>
        <L />
        <L>
          Read <a className="link" href="https://github.com/farming-labs/vigent/blob/main/docs/registry.md">docs/registry.md</a> for the safety model and the details.
        </L>
        <L />
        <Rule />
        <L className="cm">vim:tw=78:ts=8:ft=help:norl:</L>
        <Tilde />
      </main>
      <VimChrome file="registry.txt" />
    </>
  );
}
