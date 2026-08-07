# Packaging

The parts every Linux distribution channel wants, kept once. Flathub, the Snap Store and a `.deb`
in a repository that publishes AppStream all ask for the same four things, so they live here and
the AppImage — the only channel that ships today — is already built from them.

| file | what it is |
| --- | --- |
| `com.manicmicrobes.ManicMicrobes.desktop` | the menu entry: name, icon, categories |
| `com.manicmicrobes.ManicMicrobes.metainfo.xml` | AppStream: what a software centre displays |
| `screenshots/microscope.png` | the screenshot that metadata points at |

Icons are not here. They are drawn by the application at whatever size is asked for:

```sh
cargo run -p mm-app --features render --release -- --emit-icon 256 /tmp/icon.png
```

which is the same code that draws the window icon, the Windows `.ico` and the macOS `.icns`.
Committing PNGs would make a second copy of the mark, and a second copy is the thing that stops
matching the first.

## The application id

`com.manicmicrobes.ManicMicrobes`, reverse-DNS from a domain that is actually ours. Flathub
requires that and checks it. The desktop entry, the metainfo file and every installed icon are
named for it, because that is what a software centre matches on.

`appstreamcli` reports `cid-contains-uppercase-letter` at pedantic level. That is expected and
not a defect: a CamelCase last segment is the Flathub convention, as in `org.gnome.Calculator`.

## Validating a change

Both files are checked by tools that are in most distributions:

```sh
appstreamcli validate --no-net packaging/com.manicmicrobes.ManicMicrobes.metainfo.xml
desktop-file-validate packaging/com.manicmicrobes.ManicMicrobes.desktop
```

Run them after editing either. `desktop-file-validate` caught a real one already: `Education` and
`Science` are both *main* categories, and listing both can put the application in the menu twice.

## What is not done, and why

**Nothing is submitted to any store.** The README calls this pre-release and says the save format
is not stable. A store listing invites people who will judge it as finished software, so the order
is: ship `v0.1.0` on GitHub Releases, let it meet machines that did not build it, then submit.

That order is not theoretical. The first artefacts this repository ever produced looked for their
genomes on the machine that compiled them, and only running one somewhere else found it.

### Flathub, when the time comes

- A manifest building against `org.freedesktop.Platform` with the Rust SDK extension.
- **Flathub builds offline**, so the crates have to be vendored: `flatpak-cargo-generator.py`
  turns the committed `Cargo.lock` into a sources file. Mechanical, but it is the step that
  surprises people.
- Permissions: `--device=dri` for the GPU, `--socket=wayland`, `--socket=fallback-x11`,
  `--share=ipc`, and a filesystem permission for saving slides.
- Submission is a pull request to `flathub/flathub` and a human review.
- The screenshot URL in the metainfo points at `main`. A tag would be steadier and Flathub may
  ask for that.

### Snap, if Ubuntu's own store matters

`snapcraft.yaml` on `core24` with the `graphics-core24` stack for Mesa. Less reach than Flathub,
which Discover and GNOME Software both surface, but it is what appears in Ubuntu's App Centre.

### Either way

`<releases>` in the metainfo has one entry, dated the day the file was written rather than a
shipping date. The release workflow should prepend to it when it cuts a tag; until something does,
it is a manual edit and will be wrong first.
