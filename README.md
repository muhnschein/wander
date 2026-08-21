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
  `~/.config/wander/settings.conf`.
- Lists every open archive (`GET /v1/archives`) as a library.
- Renders entries in a sandboxed WebKit view through a private `cairn://`
  URI scheme: `cairn://{uuid}/{path}` is fetched from the daemon on demand,
  so archived pages keep their relative links, images and stylesheets while
  never touching the real network.
- Title-prefix search in the open archive, wired to cairn's `/suggest`.
- One random article, whenever you feel lucky.
- External links are blocked by default and only opened after an explicit
  confirmation, via the system's URI launcher.
- JavaScript is disabled in the reader. Archived pages are untrusted markup;
  cairn does not sanitize, so Wander keeps them on an isolated origin with
  scripting off and no persistent website data (ephemeral network session).

## What it does not do (yet)

- History, bookmarks, tabs, table of contents, print.
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
