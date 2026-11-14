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
          <span className="cm">prototype · sample entries are marked</span>
        </L>
        <section className="hero rhero" aria-label="registry">
          <div className="hero-copy">
            <L className="eyebrow">registry</L>
            <div className="l">
              <h1 className="t lede">Make vigent yours. Read every file before you add it.</h1>
            </div>
            <div className="l">
              <p className="t sub">
                Each entry is one small file in the vigent repo, checked by vigent's own loader before it's listed. Themes, templates and keymaps are only data. Engines and setups run commands, so the card lists them and nothing runs until you
                approve them with <span className="kw">:trust</span>.
              </p>
            </div>
          </div>
          <L className="install">
            <code>vigent add &lt;name&gt;</code>
            <span className="cm">proposed: not in vigent yet</span>
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
        <L>CI loads it with vigent's own code and rejects it with every problem listed. Once it's merged, it shows here on the next deploy.</L>
        <L />
        <Rule />
        <L className="cm">vim:tw=78:ts=8:ft=help:norl:</L>
        <Tilde />
      </main>
      <VimChrome file="registry.txt" />
    </>
  );
}
