# TODO

## WIP

### Packaging and deployment
Define the canonical build targets for a release (binary, man page, example
config, generated docs). The structure should be straightforward for distro
maintainers to wrap in RPM, DEB, ebuild, XBPS, flatpak, or AppImage without
modification.

What xtgeoip itself will provide:
- Source code and build instructions
- Build configs where they make sense (e.g. a Makefile install target)
- Documentation including runtime dependency list

GitHub releases will ship:
- Pre-built binary
- Man page (`docs/generated/xtgeoip.1`)
- Generated usage docs
- Runtime dependency list

The `xtgeoip-tests` and `xtgeoip-docgen` binaries are development tools and
should not be included in distribution packages.

### CLI codegen from spec (structure-errors WIP)
Generate `src/cli.rs` from `docs/spec/cli.yaml` using a template, so the
spec remains the single source of truth for CLI structure as well as docs and
tests. The `src/bin/structure-errors.rs` binary is the WIP vehicle for this.

Currently cli.rs is maintained by hand and must be kept in sync with the spec
manually. The codegen would eliminate that gap.
