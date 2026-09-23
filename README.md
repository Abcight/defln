# defln
A public, exported subset of my personal monorepo containing software projects,
tools, and shared libraries.

You can still contribute, but your changes will be merged into
the private repository and re-exported into this one.

Additionally, this repository has two *public mirrors*:
- [maws](https://maws.gay/abcight/defln/) /
  [abcight](https://git.abcight.com/abcight/defln/)
- [github](https://github.com/abcight/defln)

I strongly recommend maws for contributions, but GitHub's fine too.

## Structure

This repository is **not** a single top-level Rust workspace. It is a flat
collection of Rust crates, workspaces, and tools.

Refer to each project's individual directory for build and execution
instructions.

## Contributing & License

All contributions are governed by [CONTRIBUTING](CONTRIBUTING),
[DCO](DCO), and [CLA](CLA).

This repository intentionally has no blanket outbound license. Each project
directory has its own `LICENSE`; that license applies to the project's
first-party material. Unless a root-level file says otherwise, do not assume
that repository-level files or material outside a project directory are
licensed for reuse.

Third-party code, fonts, assets, and notices retain their own terms. If a
project is missing a `LICENSE`, or if unsure about an item's provenance,
assume that no rights are granted and open an issue for clarification.
