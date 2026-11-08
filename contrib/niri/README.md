# Niri linear CPU source branch

This branch is based on the public RustDesk `1.5.1` tag:

- upstream commit: `23e5bb471a040d62f0a6ad52591fb30784265b73`
- branch: `niri-linear-cpu`
- submodule `libs/hbb_common` pin: `fae47e024dd83cef6aaecde0aa7d11731df11649`

The branch records the source used for the local `429ff176…` non-Flutter
build. It is **not** a claim that a clean checkout reproduces the installed
ELF bit-for-bit: compiler, toolchain path, and link metadata affect the
binary hash.

## Source layout

The branch keeps the upstream RustDesk history and `Cargo.lock` unchanged.
It adds:

- an opt-in linear DMA-BUF CPU capture path and regression fixture;
- an optional GL bridge path retained from the same source line;
- the three-line service-to-user `RUSTDESK_WAYLAND_LINEAR_CPU=1` handoff;
- non-default C1/C2 test-only input gates and lifecycle labels;
- a tracked patch for the pinned `hbb_common` submodule.

The generated UI module `src/ui/inline.rs` and generated
`src/version.rs` are intentionally not tracked. The upstream generator and
the `inline` Cargo feature produce them.

## Rebuild inputs

From the repository root:

```sh
git submodule update --init --depth 1 libs/hbb_common
git -C libs/hbb_common apply ../../contrib/niri/patches/hbb_common-c2-ipc.patch
python3 res/inline-sciter.py
cargo build --locked --release --bin rustdesk --features inline,linux-pkg-config
```

The feature list is `inline,linux-pkg-config` plus the unchanged default
features; do not add `--no-default-features`. The inline generator is
upstream `res/inline-sciter.py`; `build.py` selects `inline` for non-Flutter
builds and runs that generator.

The hbb_common patch supplies the test-mode helpers referenced by the retained
C2 gates, so apply it before building. It does not enable those gates by
default. It leaves the submodule gitlink pinned at `fae47e0…`; it is not
vendored and no change is pushed to `rustdesk/hbb_common`. Applying the patch
produces
`src/config.rs` SHA256
`745ac51ba76a42713ee0782081c189d06f89f5866b53f56be0fce20293f3c234`.

These commands describe the inputs; they were not executed as a clean-checkout
build for this branch. No runtime validation or input test is claimed here.

## Source checkpoints

| File | SHA256 |
|---|---|
| `libs/scrap/src/wayland/pipewire.rs` | `610df23436f923b06357afe2a7f27966f570a232d15e818d160866372e2df016` |
| `libs/scrap/src/wayland/linear_cpu.rs` | `e2340648e7be79fd13c57a84b0146fac371cedbab51b4b3932ae3882a8c694db` |
| `libs/scrap/src/wayland/linear_dmabuf.rs` | `554ba3041ecde232a5c77c8654b77f83856e4ff69b1677539189c05df79d439b` |
| `src/platform/linux.rs` | `4f4ff4329456296149fb35a27c9b370f943ec9d4733f2ede8bc84750020dd44c` |
| `Cargo.lock` | `815206b42a36b2b51cdda3f5d7decbeb365b680d414728caa0586a7a703b2299` |

The installed local build's UI module was generated with the upstream
generator and had SHA256
`bca2bb3bfedd888702bce73c7eef5a4bd696cac1a3a512dc90760ed07b062481`; it is
not part of this branch.

## Packaging

`contrib/niri/PKGBUILD` is a **prebuilt-input packager**, not a clean-checkout
source build. It expects a separately supplied `rustdesk` ELF and
`libsciter-gtk.so`; the expected hashes are in the recipe. The
`429ff176…` value is only the recorded payload, not a reproducibility claim.
A fresh local source build normally produces a different ELF hash: after
verifying the source, inline, and pin inputs, record the new ELF SHA and
update the recipe's input checksum. Never use SKIP or `--skipchecksums`, and
do not describe a fresh-source package as already built. The recipe does not
build RustDesk itself.

For a manual download, save the pinned upstream
`bin.lnx/x64/libsciter-gtk.so` as exactly `libsciter-gtk.so` next to the
recipe. It remains untracked and the recipe checks its SHA.

The Sciter runtime is pinned to `c-smile/sciter-sdk` commit
`0298f1b34e9a0ff1dffb889d82c506a5da8bfb1e`, path
`bin.lnx/x64/libsciter-gtk.so`, SHA256
`a1682fbf55e004f1862d6ace31b5220121d20906bdbf308d0a9237b451e4db86`.
Only the public license and notice text are included here; the runtime binary
is not committed.

The packaging recipe installs the normal root-owned canonical path,
`/usr/share/rustdesk/rustdesk`, and the launcher at `/usr/bin/rustdesk`.
It has no install script that enables or starts the service, and it does not
touch user configuration or authentication data.

## Scope

No private engineering workspace, run logs, profiles, credentials, screenshots,
ELF/Sciter binaries, target directories, or local installation packages are
part of this branch. The test-only gates are inactive unless the explicit
C2 test environment is set.
