# Wander

A reader for [ZIM](https://openzim.org) archives served by
[cairn](https://github.com/muhnschein/cairn). Point it at a cairn instance,
browse what it has open, read the entries.

Where cairn is the stack of stones that marks the way, Wander is the walk
along it.

GTK4 + libadwaita, written in Rust. Pre-alpha.

## What it does

- Connects to a cairn daemon over plain HTTP (`IP:PORT`), optionally with a
  bearer token; the address is configured in-app and stored under
  `~/.config/wander/settings.conf`, which is written `0600` because the token
  is kept in plain text.
- Lists every open archive (`GET /v1/archives`) as a library.
- Renders entries in a sandboxed WebKit view through a private `cairn://`
  URI scheme: `cairn://{uuid}/{path}` is fetched from the daemon on demand,
  so archived pages keep their relative links, images and stylesheets.
- Back and forward through the entries you have followed within an archive.
- A table of contents for the open entry. JavaScript is disabled, so the
  outline is read out of the stored HTML rather than asked of the page;
  MediaWiki's `[edit]` links are kept out of the headings.
- Find on page (Ctrl+F), with a match count.
- Title-prefix search in the open archive, wired to cairn's `/suggest`.
- One random article, whenever you feel lucky.
- External links are blocked by default and only opened after an explicit
  confirmation, via the system's URI launcher.
- JavaScript is disabled in the reader. Archived pages are untrusted markup;
  cairn does not sanitize, so Wander keeps them on an isolated origin with
  scripting off and no persistent website data (ephemeral network session).
- Nothing in the reader reaches the network. Blocking external *navigation*
  is not enough — an archived page carrying an absolute `https://` image or
  stylesheet would load it as a subresource, which no navigation policy sees
  — so the reader's network session is additionally pointed at a dead local
  proxy. Entries still arrive, because a registered `cairn://` handler is
  served inside the web process and never reaches the networking stack.

## What it does not do (yet)

- A history list, bookmarks, tabs, print.
- Full-text search: cairn 1.x offers none; Wander inherits that gap.
- Anything but reading. There is no download manager, no catalogue, no OPDS.

## Building

Native build needs GTK4 ≥ 4.10, libadwaita ≥ 1.5 and WebKitGTK 6.0:

```console
$ cargo build --release
```

If your host lacks the libraries, `ci/container-test.sh` builds and tests
the whole workspace inside a Debian trixie container via podman:

```console
$ ci/container-test.sh
```

`cairn-client` has no GTK linkage, so it builds and tests on its own anywhere
Rust runs:

```console
$ cargo test -p cairn-client
```

## Running

Start `wander`, open the settings (gear icon) and enter the host and port of
your cairn instance. On the same machine as a TCP-configured cairnd that is
usually `127.0.0.1:8080`; adjust to wherever the stones stand.

## Layout

| Crate | Responsibility |
|---|---|
| `crates/cairn-client` | Headless HTTP client for the `cairn-api(7)` surface. No GTK linkage, fully unit-tested against a fake server. |
| `crates/app` | The GTK4/libadwaita application: library, reader, settings, `cairn://` scheme handler. |

## Licence

GPL-3.0-or-later, see [LICENSE](LICENSE).
